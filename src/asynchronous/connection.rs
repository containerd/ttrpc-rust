// Copyright 2022 Alibaba Cloud. All rights reserved.
// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use async_trait::async_trait;
use log::{error, trace};
use tokio::io::split;
use tokio::sync::oneshot;
use tokio::{io::ReadHalf, select, task};

use crate::error::Error;
use crate::proto::{GenMessage, GenMessageError, MessageHeader};

use super::{stream::SendingMessage, transport::Socket};

pub trait Builder {
    type Reader;
    type Writer;

    fn build(&mut self) -> (Self::Reader, Self::Writer);
}

#[async_trait]
pub trait WriterDelegate {
    async fn recv(&mut self) -> Option<SendingMessage>;
    async fn exit(&self);
}

#[async_trait]
pub trait ReaderDelegate {
    async fn wait_shutdown(&self);
    async fn disconnect(&self, e: Error, task: &mut task::JoinHandle<()>);
    async fn exit(&self);
    async fn handle_msg(&self, msg: GenMessage);
    async fn handle_err(&self, header: MessageHeader, e: Error);
}

pub struct Connection<B: Builder> {
    reader: ReadHalf<Socket>,
    writer_task: task::JoinHandle<()>,
    reader_delegate: B::Reader,
    // Delivers a fatal write error from the writer task. Receiving a value
    // means the writer hit an unrecoverable transport error and the whole
    // connection must be torn down; the channel closing without a value means
    // the writer stopped normally.
    writer_error: oneshot::Receiver<Error>,
}

impl<B> Connection<B>
where
    B: Builder,
    B::Reader: ReaderDelegate + Send + Sync + 'static,
    B::Writer: WriterDelegate + Send + Sync + 'static,
{
    pub fn new(conn: Socket, mut builder: B) -> Self {
        let (reader, mut writer) = split(conn);

        let (reader_delegate, mut writer_delegate) = builder.build();
        let (err_tx, err_rx) = oneshot::channel();

        // Long-running sender task
        let writer_task = tokio::spawn(async move {
            while let Some(mut sending_msg) = writer_delegate.recv().await {
                trace!("write message: {:?}", sending_msg.msg);
                if let Err(e) = sending_msg.msg.write_to(&mut writer).await {
                    error!("write_message got error: {:?}", e);
                    // Report the failure to the caller awaiting this send.
                    sending_msg.send_result(Err(e.clone()));
                    // write_to uses write_all internally, so a failed write may
                    // have left a partial frame on the wire, desynchronizing the
                    // frame boundaries of every stream multiplexed on this
                    // connection; it is no longer usable. (This can happen on an
                    // otherwise healthy socket, e.g. ENOMEM when the kernel
                    // cannot satisfy a high-order allocation under memory
                    // fragmentation.) Report the error to Connection::run at once
                    // and exit. Deliberately do NOT wait on writer.shutdown(): it
                    // can block (some transports never complete poll_shutdown)
                    // and would delay or prevent cleanup. run() closes the whole
                    // connection; dropping this task drops the write half.
                    let _ = err_tx.send(e);
                    return;
                }
                sending_msg.send_result(Ok(()));
            }
            // The outbound channel closed: this is a normal shutdown.
            writer_delegate.exit().await;
            trace!("Writer task exit.");
        });

        Self {
            reader,
            writer_task,
            reader_delegate,
            writer_error: err_rx,
        }
    }

    pub async fn run(self) -> std::io::Result<()> {
        let Connection {
            mut reader,
            mut writer_task,
            reader_delegate,
            mut writer_error,
        } = self;
        loop {
            select! {
                // Fixed poll order: a write error wins over the shutdown
                // notification the writer raises as it unwinds, and a pending
                // shutdown wins over further reads. Both are idle during normal
                // operation, so read_from is still reached every iteration.
                biased;

                werr = &mut writer_error => {
                    // Ok(e): the writer hit a fatal transport error. Drive a
                    // connection-wide teardown — fail every registered client
                    // stream / stop the server handlers, and drop the read half
                    // on exit. Err(_): the writer stopped without an error.
                    if let Ok(e) = werr {
                        trace!("Writer failed, tearing down connection: {:?}", e);
                        reader_delegate.disconnect(e, &mut writer_task).await;
                    }
                    break;
                }
                _v = reader_delegate.wait_shutdown() => {
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
                            reader_delegate.disconnect(e, &mut writer_task).await;
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
