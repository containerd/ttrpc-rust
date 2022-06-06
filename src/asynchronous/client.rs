// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use std::collections::HashMap;
use std::convert::TryInto;
use std::os::unix::io::RawFd;
use std::sync::{Arc, Mutex};

use nix::unistd::close;
use tokio::{
    self,
    io::split,
    sync::mpsc::{channel, Receiver, Sender},
    sync::Notify,
};

use crate::common::client_connect;
use crate::error::{Error, Result};
use crate::proto::{Code, Codec, GenMessage, Message, Request, Response, MESSAGE_TYPE_RESPONSE};
use crate::r#async::utils;

type RequestSender = Sender<(GenMessage, Sender<Result<Vec<u8>>>)>;
type RequestReceiver = Receiver<(GenMessage, Sender<Result<Vec<u8>>>)>;

type ResponseSender = Sender<Result<Vec<u8>>>;
type ResponseReceiver = Receiver<Result<Vec<u8>>>;

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

        let (mut reader, mut writer) = split(stream);
        let (req_tx, mut rx): (RequestSender, RequestReceiver) = channel(100);

        let req_map = Arc::new(Mutex::new(HashMap::new()));
        let req_map2 = req_map.clone();

        let notify = Arc::new(Notify::new());
        let notify2 = notify.clone();

        // Request sender
        let request_sender = tokio::spawn(async move {
            let mut stream_id: u32 = 1;

            while let Some((mut msg, resp_tx)) = rx.recv().await {
                let current_stream_id = stream_id;
                msg.header.set_stream_id(current_stream_id);
                stream_id += 2;

                {
                    let mut map = req_map2.lock().unwrap();
                    map.insert(current_stream_id, resp_tx.clone());
                }

                if let Err(e) = msg.write_to(&mut writer).await {
                    error!("write_message got error: {:?}", e);

                    {
                        let mut map = req_map2.lock().unwrap();
                        map.remove(&current_stream_id);
                    }

                    let e = Error::Socket(format!("{:?}", e));
                    resp_tx
                        .send(Err(e))
                        .await
                        .unwrap_or_else(|_e| error!("The request has returned"));

                    break; // The stream is dead, exit the loop.
                }
            }

            // rx.recv will abort when client.req_tx and client is dropped.
            // notify the response-receiver to quit at this time.
            notify.notify_one();
        });

        // Response receiver
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = notify2.notified() => {
                        break;
                    }
                    res = GenMessage::read_from(&mut reader) => {
                        match res {
                            Ok(msg) => {
                                trace!("Got Message body {:?}", msg.payload);
                                let req_map = req_map.clone();
                                tokio::spawn(async move {
                                    let resp_tx2;
                                    {
                                        let mut map = req_map.lock().unwrap();
                                        let resp_tx = match map.get(&msg.header.stream_id) {
                                            Some(tx) => tx,
                                            None => {
                                                debug!(
                                                    "Receiver got unknown packet {:?}",
                                                    msg
                                                );
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

                                    resp_tx2.send(Ok(msg.payload)).await.unwrap_or_else(|_e| error!("The request has returned"));
                                });
                            }
                            Err(e) => {
                                debug!("Connection closed by the ttRPC server: {}", e);

                                // Abort the request sender task to prevent incoming RPC requests
                                // from being processed.
                                request_sender.abort();
                                let _ = request_sender.await;

                                // Take all items out of `req_map`.
                                let mut map = std::mem::take(&mut *req_map.lock().unwrap());
                                // Terminate outstanding RPC requests with the error.
                                for (_stream_id, resp_tx) in map.drain() {
                                    if let Err(_e) = resp_tx.send(Err(e.clone())).await {
                                        warn!("Failed to terminate pending RPC: \
                                               the request has returned");
                                    }
                                }

                                break;
                            }
                        }
                    }
                };
            }
        });

        Client { req_tx }
    }

    /// Requsts a unary request and returns with response.
    pub async fn request(&self, req: Request) -> Result<Response> {
        let timeout_nano = req.timeout_nano;
        let msg: GenMessage = Message::new_request(0, req)
            .try_into()
            .map_err(|e: protobuf::error::ProtobufError| Error::Others(e.to_string()))?;

        let (tx, mut rx): (ResponseSender, ResponseReceiver) = channel(100);
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

        let buf = result?;
        let res =
            Response::decode(&buf).map_err(err_to_others_err!(e, "Unpack response error "))?;

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
