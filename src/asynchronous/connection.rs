// Copyright 2022 Alibaba Cloud. All rights reserved.
// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use std::os::unix::io::AsRawFd;

use async_trait::async_trait;
use log::{error, trace};
use tokio::{
    io::{split, AsyncRead, AsyncWrite, ReadHalf},
    select, task,
};

use crate::error::Error;
use crate::proto::{GenMessage, GenMessageError, MessageHeader};

pub trait Builder {
    type Reader;
    type Writer;

    fn build(&mut self) -> (Self::Reader, Self::Writer);
}

#[async_trait]
pub trait WriterDelegate {
    async fn recv(&mut self) -> Option<GenMessage>;
    async fn disconnect(&self, msg: &GenMessage, e: Error);
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

pub struct Connection<S, B: Builder> {
    reader: ReadHalf<S>,
    writer_task: task::JoinHandle<()>,
    reader_delegate: B::Reader,
}

impl<S, B> Connection<S, B>
where
    S: AsyncRead + AsyncWrite + AsRawFd + Send + 'static,
    B: Builder,
    B::Reader: ReaderDelegate + Send + Sync + 'static,
    B::Writer: WriterDelegate + Send + Sync + 'static,
{
    pub fn new(conn: S, mut builder: B) -> Self {
        let (reader, mut writer) = split(conn);

        let (reader_delegate, mut writer_delegate) = builder.build();

        let writer_task = tokio::spawn(async move {
            while let Some(msg) = writer_delegate.recv().await {
                trace!("write message: {:?}", msg);
                if let Err(e) = msg.write_to(&mut writer).await {
                    error!("write_message got error: {:?}", e);
                    writer_delegate.disconnect(&msg, e).await;
                }
            }
            writer_delegate.exit().await;
            trace!("Writer task exit.");
        });

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
