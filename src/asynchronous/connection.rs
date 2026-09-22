// Copyright 2022 Alibaba Cloud. All rights reserved.
// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use std::future::Future;
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
    prefix: &mut Vec<u8>,
) -> WriteOutcome {
    let Some(control) = sending_msg.control.as_ref() else {
        trace!("write message: {:?}", sending_msg.msg);
        return WriteOutcome::Complete(sending_msg.msg.write_to_buffered(writer, prefix).await);
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
            result = sending_msg.msg.write_to_buffered(writer, prefix) => WriteOutcome::Complete(result),
            _ = control.cancelled() => WriteOutcome::Cancelled,
            _ = sleep_until(deadline) => WriteOutcome::DeadlineElapsed,
        }
    } else {
        select! {
            biased;
            result = sending_msg.msg.write_to_buffered(writer, prefix) => WriteOutcome::Complete(result),
            _ = control.cancelled() => WriteOutcome::Cancelled,
        }
    }
}

async fn run_writer(
    mut writer: impl AsyncWrite + Unpin,
    mut writer_delegate: impl WriterDelegate,
) -> Result<()> {
    // One bounded prefix buffer per connection, reused across frames.
    let mut prefix = Vec::new();
    let result = loop {
        let Some(mut sending_msg) = writer_delegate.recv().await else {
            break Ok(());
        };

        let failure = match write_message(&mut writer, &sending_msg, &mut prefix).await {
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
            sending_msg.send_result(Err(message_error));
            // Return without waiting for socket shutdown, which may never complete.
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

pub trait WriterDelegate {
    fn recv(&mut self) -> impl Future<Output = Option<SendingMessage>> + Send;
    fn exit(&self) -> impl Future<Output = ()> + Send;
}

pub trait ReaderDelegate {
    fn wait_shutdown(&self) -> impl Future<Output = ()> + Send;
    fn disconnect(&self, e: Error) -> impl Future<Output = ()> + Send;
    fn exit(&self) -> impl Future<Output = ()> + Send;
    fn handle_msg(&self, msg: GenMessage) -> impl Future<Output = ()> + Send;
    fn handle_err(&self, header: MessageHeader, e: Error) -> impl Future<Output = ()> + Send;
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
        let shutdown = reader_delegate.wait_shutdown();
        tokio::pin!(shutdown);
        loop {
            select! {
                // Writer failures take priority, then shutdown, then incoming frames.
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
                _v = &mut shutdown => {
                    trace!("Receive shutdown.");
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
            }
        }
        reader_delegate.exit().await;
        trace!("Reader task exit.");

        Ok(())
    }
}
