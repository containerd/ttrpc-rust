// Copyright 2022 Alibaba Cloud. All rights reserved.
// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use async_trait::async_trait;
use log::{error, trace};
use tokio::io::{split, AsyncWrite};
use tokio::time::{sleep_until, Instant};
use tokio::{io::ReadHalf, select, task};

use crate::error::{Error, Result};
use crate::proto::{GenMessage, GenMessageError, MessageHeader};

use super::{stream::SendingMessage, transport::Socket};

enum WriteOutcome {
    Complete(crate::error::Result<()>),
    Discarded(Error),
    Cancelled,
    DeadlineElapsed,
}

pub(crate) fn request_timeout_error() -> Error {
    Error::Others("Request deadline elapsed".to_string())
}

async fn write_message(
    writer: &mut (impl AsyncWrite + Unpin),
    sending_msg: &SendingMessage,
) -> WriteOutcome {
    let Some(control) = sending_msg.control.as_ref() else {
        trace!("write message: {:?}", sending_msg.msg);
        return WriteOutcome::Complete(sending_msg.msg.write_to(writer).await);
    };
    let deadline = control.deadline();

    let expired = deadline.is_some_and(|deadline| deadline <= Instant::now());
    if control.is_cancelled() {
        return WriteOutcome::Discarded(Error::LocalClosed);
    }
    if expired {
        return WriteOutcome::Discarded(request_timeout_error());
    }

    trace!("write message: {:?}", sending_msg.msg);
    if let Some(deadline) = deadline {
        select! {
            biased;
            result = sending_msg.msg.write_to(writer) => WriteOutcome::Complete(result),
            _ = control.cancelled() => WriteOutcome::Cancelled,
            _ = sleep_until(deadline) => WriteOutcome::DeadlineElapsed,
        }
    } else {
        select! {
            biased;
            result = sending_msg.msg.write_to(writer) => WriteOutcome::Complete(result),
            _ = control.cancelled() => WriteOutcome::Cancelled,
        }
    }
}

async fn run_writer(
    mut writer: impl AsyncWrite + Unpin,
    mut writer_delegate: impl WriterDelegate,
) -> Result<()> {
    let result = loop {
        let Some(mut sending_msg) = writer_delegate.recv().await else {
            break Ok(());
        };

        let failure = match write_message(&mut writer, &sending_msg).await {
            WriteOutcome::Complete(Ok(())) => {
                sending_msg.send_result(Ok(()));
                continue;
            }
            WriteOutcome::Discarded(e) => {
                sending_msg.send_result(Err(e));
                continue;
            }
            WriteOutcome::Complete(Err(e)) => Some((e.clone(), e)),
            WriteOutcome::Cancelled => Some((
                Error::LocalClosed,
                Error::Socket(
                    "connection closed after a request was cancelled during write".to_string(),
                ),
            )),
            WriteOutcome::DeadlineElapsed => Some((
                request_timeout_error(),
                Error::Socket(
                    "connection closed after a request deadline elapsed during write".to_string(),
                ),
            )),
        };

        if let Some((message_error, connection_error)) = failure {
            error!("write_message got error: {:?}", connection_error);
            sending_msg.send_result(Err(message_error.clone()));
            writer_delegate
                .disconnect(&sending_msg.msg, message_error)
                .await;
            break Err(connection_error);
        }
    };

    writer_delegate.exit().await;
    trace!("Writer task exit.");
    result
}

pub trait Builder {
    type Reader;
    type Writer;

    fn build(&mut self) -> (Self::Reader, Self::Writer);
}

#[async_trait]
pub trait WriterDelegate {
    async fn recv(&mut self) -> Option<SendingMessage>;
    async fn disconnect(&self, msg: &GenMessage, e: Error);
    async fn exit(&self);
}

#[async_trait]
pub trait ReaderDelegate {
    async fn wait_shutdown(&self);
    async fn disconnect(&self, e: Error);
    async fn exit(&self);
    async fn handle_msg(&self, msg: GenMessage);
    async fn handle_err(&self, header: MessageHeader, e: Error);
}

pub struct Connection<B: Builder> {
    reader: ReadHalf<Socket>,
    writer_task: task::JoinHandle<Result<()>>,
    reader_delegate: B::Reader,
}

impl<B> Connection<B>
where
    B: Builder,
    B::Reader: ReaderDelegate + Send + Sync + 'static,
    B::Writer: WriterDelegate + Send + Sync + 'static,
{
    pub fn new(conn: Socket, mut builder: B) -> Self {
        let (reader, writer) = split(conn);

        let (reader_delegate, writer_delegate) = builder.build();

        // Long-running sender task
        let writer_task = tokio::spawn(run_writer(writer, writer_delegate));

        Self {
            reader,
            writer_task,
            reader_delegate,
        }
    }

    pub async fn run(self) -> std::io::Result<()> {
        let Connection {
            mut reader,
            mut writer_task,
            reader_delegate,
        } = self;
        loop {
            select! {
                biased;
                writer_result = &mut writer_task => {
                    match writer_result {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => {
                            trace!("Write msg err: {:?}", e);
                            reader_delegate.disconnect(e).await;
                        }
                        Err(e) => {
                            let e = Error::Others(format!("Writer task failed: {e}"));
                            error!("Write task err: {:?}", e);
                            reader_delegate.disconnect(e).await;
                        }
                    }
                    break;
                }
                res = GenMessage::read_from(&mut reader) => {
                    match res {
                        Ok(msg) => {
                            trace!("Got Message {:?}", msg);
                            reader_delegate.handle_msg(msg).await;
                        }
                        Err(GenMessageError::ReturnError(header, e)) => {
                            trace!("Read msg err (can be return): {:?}", e);
                            reader_delegate.handle_err(header, e).await;
                        }

                        Err(GenMessageError::InternalError(e)) => {
                            trace!("Read msg err: {:?}", e);
                            writer_task.abort();
                            let _ = (&mut writer_task).await;
                            reader_delegate.disconnect(e).await;
                            break;
                        }
                    }
                }
                _v = reader_delegate.wait_shutdown() => {
                    trace!("Receive shutdown.");
                    break;
                }
            }
        }
        reader_delegate.exit().await;
        trace!("Reader task exit.");

        Ok(())
    }
}
