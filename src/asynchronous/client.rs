// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use std::collections::HashMap;
use std::convert::TryInto;
use std::os::unix::io::RawFd;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use nix::unistd::close;
use tokio::{self, sync::mpsc, task};

use crate::common::client_connect;
use crate::error::{Error, Result};
use crate::proto::{Code, Codec, GenMessage, Message, Request, Response, MESSAGE_TYPE_RESPONSE};
use crate::r#async::connection::*;
use crate::r#async::shutdown;
use crate::r#async::stream::{ResultReceiver, ResultSender};
use crate::r#async::utils;

type RequestSender = mpsc::Sender<(GenMessage, ResultSender)>;
type RequestReceiver = mpsc::Receiver<(GenMessage, ResultSender)>;

/// A ttrpc Client (async).
#[derive(Clone)]
pub struct Client {
    req_tx: RequestSender,
}

impl Client {
    pub fn connect(sockaddr: &str) -> Result<Client> {
        let fd = unsafe { client_connect(sockaddr)? };
        Ok(Self::new(fd))
    }

    /// Initialize a new [`Client`].
    pub fn new(fd: RawFd) -> Client {
        let stream = utils::new_unix_stream_from_raw_fd(fd);

        let (req_tx, rx): (RequestSender, RequestReceiver) = mpsc::channel(100);

        let delegate = ClientBuilder { rx: Some(rx) };

        let conn = Connection::new(stream, delegate);
        tokio::spawn(async move { conn.run().await });

        Client { req_tx }
    }

    /// Requsts a unary request and returns with response.
    pub async fn request(&self, req: Request) -> Result<Response> {
        let timeout_nano = req.timeout_nano;
        let msg: GenMessage = Message::new_request(0, req)
            .try_into()
            .map_err(|e: protobuf::error::ProtobufError| Error::Others(e.to_string()))?;

        let (tx, mut rx): (ResultSender, ResultReceiver) = mpsc::channel(100);
        self.req_tx
            .send((msg, tx))
            .await
            .map_err(|e| Error::Others(format!("Send packet to sender error {:?}", e)))?;

        let result = if timeout_nano == 0 {
            rx.recv()
                .await
                .ok_or_else(|| Error::Others("Receive packet from receiver error".to_string()))?
        } else {
            tokio::time::timeout(
                std::time::Duration::from_nanos(timeout_nano as u64),
                rx.recv(),
            )
            .await
            .map_err(|e| Error::Others(format!("Receive packet timeout {:?}", e)))?
            .ok_or_else(|| Error::Others("Receive packet from receiver error".to_string()))?
        };

        let msg = result?;
        let res = Response::decode(&msg.payload)
            .map_err(err_to_others_err!(e, "Unpack response error "))?;

        let status = res.get_status();
        if status.get_code() != Code::OK {
            return Err(Error::RpcStatus((*status).clone()));
        }

        Ok(res)
    }
}

struct ClientClose {
    fd: RawFd,
    close_fd: RawFd,
}

impl Drop for ClientClose {
    fn drop(&mut self) {
        close(self.close_fd).unwrap();
        close(self.fd).unwrap();
        trace!("All client is droped");
    }
}

#[derive(Debug)]
struct ClientBuilder {
    rx: Option<RequestReceiver>,
}

impl Builder for ClientBuilder {
    type Reader = ClientReader;
    type Writer = ClientWriter;

    fn build(&mut self) -> (Self::Reader, Self::Writer) {
        let (notifier, waiter) = shutdown::new();
        let req_map = Arc::new(Mutex::new(HashMap::new()));
        (
            ClientReader {
                shutdown_waiter: waiter,
                req_map: req_map.clone(),
            },
            ClientWriter {
                stream_id: 1,
                rx: self.rx.take().unwrap(),
                shutdown_notifier: notifier,
                req_map,
            },
        )
    }
}

struct ClientWriter {
    stream_id: u32,
    rx: RequestReceiver,
    shutdown_notifier: shutdown::Notifier,
    req_map: Arc<Mutex<HashMap<u32, ResultSender>>>,
}

#[async_trait]
impl WriterDelegate for ClientWriter {
    async fn recv(&mut self) -> Option<GenMessage> {
        if let Some((mut msg, resp_tx)) = self.rx.recv().await {
            let current_stream_id = self.stream_id;
            msg.header.set_stream_id(current_stream_id);
            self.stream_id += 2;
            {
                let mut map = self.req_map.lock().unwrap();
                map.insert(current_stream_id, resp_tx);
            }
            return Some(msg);
        } else {
            return None;
        }
    }

    async fn disconnect(&self, msg: &GenMessage, e: Error) {
        let resp_tx = {
            let mut map = self.req_map.lock().unwrap();
            map.remove(&msg.header.stream_id)
        };

        if let Some(resp_tx) = resp_tx {
            let e = Error::Socket(format!("{:?}", e));
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

struct ClientReader {
    shutdown_waiter: shutdown::Waiter,
    req_map: Arc<Mutex<HashMap<u32, ResultSender>>>,
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
        let mut map = std::mem::take(&mut *self.req_map.lock().unwrap());
        // Terminate outstanding RPC requests with the error.
        for (_stream_id, resp_tx) in map.drain() {
            if let Err(_e) = resp_tx.send(Err(e.clone())).await {
                warn!("Failed to terminate pending RPC: the request has returned");
            }
        }
    }

    async fn exit(&self) {}

    async fn handle_msg(&self, msg: GenMessage) {
        let req_map = self.req_map.clone();
        tokio::spawn(async move {
            let resp_tx2;
            {
                let mut map = req_map.lock().unwrap();
                let resp_tx = match map.get(&msg.header.stream_id) {
                    Some(tx) => tx,
                    None => {
                        debug!("Receiver got unknown packet {:?}", msg);
                        return;
                    }
                };

                resp_tx2 = resp_tx.clone();
                map.remove(&msg.header.stream_id); // Forget the result, just remove.
            }

            if msg.header.type_ != MESSAGE_TYPE_RESPONSE {
                resp_tx2
                    .send(Err(Error::Others(format!(
                        "Recver got malformed packet {:?}",
                        msg
                    ))))
                    .await
                    .unwrap_or_else(|_e| error!("The request has returned"));
                return;
            }

            resp_tx2
                .send(Ok(msg))
                .await
                .unwrap_or_else(|_e| error!("The request has returned"));
        });
    }
}
