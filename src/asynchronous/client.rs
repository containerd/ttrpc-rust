// Copyright 2022 Alibaba Cloud. All rights reserved.
// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use std::collections::HashMap;
use std::convert::TryInto;
#[cfg(unix)]
use std::os::unix::io::RawFd;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::{self, sync::mpsc, task};

use crate::error::{get_rpc_status, Error, Result};
use crate::proto::{
    Code, Codec, GenMessage, Message, MessageHeader, Request, Response, FLAG_NO_DATA,
    FLAG_REMOTE_CLOSED, FLAG_REMOTE_OPEN, MESSAGE_TYPE_DATA, MESSAGE_TYPE_RESPONSE,
};
use crate::r#async::connection::*;
use crate::r#async::shutdown;
use crate::r#async::stream::{
    Kind, MessageReceiver, MessageSender, ResultReceiver, ResultSender, StreamInner,
};

use super::stream::SendingMessage;
use super::transport::Socket;

/// A ttrpc Client (async).
#[derive(Clone)]
pub struct Client {
    req_tx: MessageSender,
    next_stream_id: Arc<AtomicU32>,
    streams: Arc<Mutex<HashMap<u32, ResultSender>>>,
}

impl Client {
    pub async fn connect(sockaddr: &str) -> Result<Client> {
        let socket = Socket::connect(sockaddr)
            .await
            .map_err(err_to_others_err!(e, "Socket::connect error "))?;
        Ok(Self::new(socket))
    }

    #[cfg(unix)]
    /// # Safety
    /// The file descriptor must represent a unix socket.
    pub unsafe fn from_raw_unix_socket_fd(fd: RawFd) -> Client {
        let stream = unsafe { Socket::from_raw_unix_socket_fd(fd) }.unwrap();
        Self::new(stream)
    }

    /// Initialize a new [`Client`].
    pub fn new(stream: Socket) -> Client {
        let (req_tx, rx): (MessageSender, MessageReceiver) = mpsc::channel(100);

        let req_map = Arc::new(Mutex::new(HashMap::new()));
        let delegate = ClientBuilder {
            rx: Some(rx),
            streams: req_map.clone(),
        };

        let conn = Connection::new(stream, delegate);
        // Long-running receiver task
        tokio::spawn(async move { conn.run().await });

        Client {
            req_tx,
            next_stream_id: Arc::new(AtomicU32::new(1)),
            streams: req_map,
        }
    }

    /// Requsts a unary request and returns with response.
    pub async fn request(&self, req: Request) -> Result<Response> {
        let timeout_nano = req.timeout_nano;
        let stream_id = self.next_stream_id.fetch_add(2, Ordering::Relaxed);
        let msg: GenMessage;
        #[cfg(not(feature = "prost"))]
        {
            msg = Message::new_request(stream_id, req)?
                .try_into()
                .map_err(|err: protobuf::Error| Error::Others(err.to_string()))?;
        }
        
        #[cfg(feature = "prost")]
        {
            msg = Message::new_request(stream_id, req)?
                .try_into()
                .map_err(|err: std::io::Error| Error::Others(err.to_string()))?;
        }

        let (tx, mut rx): (ResultSender, ResultReceiver) = mpsc::channel(100);
        
        self.streams
            .lock()
            .map_err(|_| Error::Others("Failed to acquire lock on streams".to_string()))?
            .insert(stream_id, tx);
        
        self.req_tx
            .send(SendingMessage::new(msg))
            .await
            .map_err(|_| Error::LocalClosed)?;
        
        #[allow(clippy::unnecessary_lazy_evaluations)]
        let result = if timeout_nano == 0 {
            rx.recv().await.ok_or_else(|| Error::RemoteClosed)?
        } else {
            tokio::time::timeout(
                std::time::Duration::from_nanos(timeout_nano as u64),
                rx.recv(),
            )
            .await
            .map_err(|e| Error::Others(format!("Receive packet timeout {e:?}")))?
            .ok_or_else(|| Error::RemoteClosed)?
        };

        let msg = result?;

        let res = Response::decode(msg.payload)
            .map_err(err_to_others_err!(e, "Unpack response error "))?;

        #[cfg(not(feature = "prost"))]
        {
            let status = res.status();
            if status.code() != Code::OK {
                return Err(Error::RpcStatus((*status).clone()));
            }
        }
        #[cfg(feature = "prost")]
        {
            let status = res.status.as_ref();
            if let Some(status) = status {
                if status.code != Code::Ok as i32 {
                    return Err(Error::RpcStatus(status.clone()));
                }
            }
        }

        Ok(res)
    }

    /// Creates a StreamInner instance.
    pub async fn new_stream(
        &self,
        req: Request,
        streaming_client: bool,
        streaming_server: bool,
    ) -> Result<StreamInner> {
        let stream_id = self.next_stream_id.fetch_add(2, Ordering::Relaxed);
        let is_req_payload_empty = req.payload.is_empty();

        #[cfg(not(feature = "prost"))]
        let mut msg: GenMessage = Message::new_request(stream_id, req)?
            .try_into()
            .map_err(|err: protobuf::Error| Error::Others(err.to_string()))?;

        #[cfg(feature = "prost")]
        let mut msg: GenMessage = Message::new_request(stream_id, req)?
            .try_into()
            .map_err(|err: std::io::Error| Error::Others(err.to_string()))?;

        if streaming_client {
            if !is_req_payload_empty {
                #[cfg(not(feature = "prost"))]
                return Err(get_rpc_status(
                    Code::INVALID_ARGUMENT,
                    "Creating a ClientStream and sending payload at the same time is not allowed",
                ));
                #[cfg(feature = "prost")]
                return Err(get_rpc_status(
                    Code::Unknown,
                    "Creating a ClientStream and sending payload at the same time is not allowed",
                ));
            }
            msg.header.add_flags(FLAG_REMOTE_OPEN | FLAG_NO_DATA);
        } else {
            msg.header.add_flags(FLAG_REMOTE_CLOSED);
        }

        let (tx, rx): (ResultSender, ResultReceiver) = mpsc::channel(100);
        self.streams
            .lock()
            .map_err(|_| Error::Others("Failed to acquire lock on streams".to_string()))?
            .insert(stream_id, tx);

        self.req_tx
            .send(SendingMessage::new(msg))
            .await
            .map_err(|e| Error::Others(format!("Send packet to sender error {e:?}")))?;

        Ok(StreamInner::new(
            stream_id,
            self.req_tx.clone(),
            rx,
            streaming_client,
            streaming_server,
            Kind::Client,
            self.streams.clone(),
        ))
    }
}

#[derive(Debug)]
struct ClientBuilder {
    rx: Option<MessageReceiver>,
    streams: Arc<Mutex<HashMap<u32, ResultSender>>>,
}

impl Builder for ClientBuilder {
    type Reader = ClientReader;
    type Writer = ClientWriter;

    fn build(&mut self) -> (Self::Reader, Self::Writer) {
        let (notifier, waiter) = shutdown::new();
        (
            ClientReader {
                shutdown_waiter: waiter,
                streams: self.streams.clone(),
            },
            ClientWriter {
                rx: self.rx.take().unwrap(),
                shutdown_notifier: notifier,

                streams: self.streams.clone(),
            },
        )
    }
}

struct ClientWriter {
    rx: MessageReceiver,
    shutdown_notifier: shutdown::Notifier,

    streams: Arc<Mutex<HashMap<u32, ResultSender>>>,
}

#[async_trait]
impl WriterDelegate for ClientWriter {
    async fn recv(&mut self) -> Option<SendingMessage> {
        self.rx.recv().await
    }

    async fn disconnect(&self, msg: &GenMessage, e: Error) {
        // TODO:
        // At this point, a new request may have been received.
        let resp_tx = {
            let mut map = self.streams.lock().unwrap();
            map.remove(&msg.header.stream_id)
        };

        // TODO: if None
        if let Some(resp_tx) = resp_tx {
            let e = Error::Socket(format!("{e:?}"));
            resp_tx
                .send(Err(e))
                .await
                .unwrap_or_else(|_e| error!("The request has returned"));
        }
    }

    async fn exit(&self) {
        self.shutdown_notifier.shutdown();
    }
}

async fn get_resp_tx(
    req_map: Arc<Mutex<HashMap<u32, ResultSender>>>,
    header: &MessageHeader,
) -> Option<ResultSender> {
    let resp_tx = match header.type_ {
        MESSAGE_TYPE_RESPONSE => match req_map.lock().unwrap().remove(&header.stream_id) {
            Some(tx) => tx,
            None => {
                debug!("Receiver got unknown response packet {:?}", header);
                return None;
            }
        },
        MESSAGE_TYPE_DATA => {
            if (header.flags & FLAG_REMOTE_CLOSED) == FLAG_REMOTE_CLOSED {
                match req_map.lock().unwrap().remove(&header.stream_id) {
                    Some(tx) => tx,
                    None => {
                        debug!("Receiver got unknown data packet {:?}", header);
                        return None;
                    }
                }
            } else {
                match req_map.lock().unwrap().get(&header.stream_id) {
                    Some(tx) => tx.clone(),
                    None => {
                        debug!("Receiver got unknown data packet {:?}", header);
                        return None;
                    }
                }
            }
        }
        _ => {
            let resp_tx = match req_map.lock().unwrap().remove(&header.stream_id) {
                Some(tx) => tx,
                None => {
                    debug!("Receiver got unknown packet {:?}", header);
                    return None;
                }
            };
            resp_tx
                .send(Err(Error::Others(format!(
                    "Receiver got malformed packet {header:?}"
                ))))
                .await
                .unwrap_or_else(|_e| error!("The request has returned"));
            return None;
        }
    };

    Some(resp_tx)
}

struct ClientReader {
    streams: Arc<Mutex<HashMap<u32, ResultSender>>>,
    shutdown_waiter: shutdown::Waiter,
}

#[async_trait]
impl ReaderDelegate for ClientReader {
    async fn wait_shutdown(&self) {
        self.shutdown_waiter.wait_shutdown().await
    }

    async fn disconnect(&self, e: Error, sender: &mut task::JoinHandle<()>) {
        // Abort the request sender task to prevent incoming RPC requests
        // from being processed.
        sender.abort();
        let _ = sender.await;

        // Take all items out of `req_map`.
        let mut map = std::mem::take(&mut *self.streams.lock().unwrap());
        // Terminate undone RPC requests with the error.
        for (_stream_id, resp_tx) in map.drain() {
            if let Err(_e) = resp_tx.send(Err(e.clone())).await {
                warn!("Failed to terminate pending RPC: the request has returned");
            }
        }
    }

    async fn exit(&self) {}

    async fn handle_err(&self, header: MessageHeader, e: Error) {
        let req_map = self.streams.clone();
        tokio::spawn(async move {
            if let Some(resp_tx) = get_resp_tx(req_map, &header).await {
                resp_tx
                    .send(Err(e))
                    .await
                    .unwrap_or_else(|_e| error!("The request has returned"));
            }
        });
    }

    async fn handle_msg(&self, msg: GenMessage) {
        let req_map = self.streams.clone();
        tokio::spawn(async move {
            if let Some(resp_tx) = get_resp_tx(req_map, &msg.header).await {
                resp_tx
                    .send(Ok(msg))
                    .await
                    .unwrap_or_else(|_e| error!("The request has returned"));
            }
        });
    }
}
