// Copyright (c) 2019 Ant Financial
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Sync client of ttrpc.


use std::collections::HashMap;
use std::os::unix::io::RawFd;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::{thread};

use crate::error::{Error, Result};
use crate::sync::sys::{ClientConnection};
use crate::proto::{Code, Codec, MessageHeader, Request, Response, MESSAGE_TYPE_RESPONSE};
use crate::sync::channel::{read_message, write_message};
use std::time::Duration;

type Sender = mpsc::Sender<(Vec<u8>, mpsc::SyncSender<Result<Vec<u8>>>)>;
type Receiver = mpsc::Receiver<(Vec<u8>, mpsc::SyncSender<Result<Vec<u8>>>)>;

/// A ttrpc Client (sync).
#[derive(Clone)]
pub struct Client {
    _fd: Arc<ClientConnection>,
    sender_tx: Sender,
}

impl Client {
    pub fn connect(sockaddr: &str) -> Result<Client> {
        let conn = ClientConnection::client_connect(sockaddr)?;
        
        Ok(Self::new_client(conn))
    }

    /// Initialize a new [`Client`] from raw file descriptor.
    pub fn new(fd: RawFd) -> Client {
        let conn = ClientConnection::new(fd);

        Self::new_client(conn)
    }

    fn new_client(pipe_client: ClientConnection) -> Client {
        let client = Arc::new(pipe_client);
        

        let (sender_tx, rx): (Sender, Receiver) = mpsc::channel();

        
        let recver_map_orig = Arc::new(Mutex::new(HashMap::new()));

        //Sender
        let recver_map = recver_map_orig.clone();
        let sender_client = client.clone();
        thread::spawn(move || {
            let mut stream_id: u32 = 1;
            for (buf, recver_tx) in rx.iter() {
                let current_stream_id = stream_id;
                stream_id += 2;
                //Put current_stream_id and recver_tx to recver_map
                {
                    let mut map = recver_map.lock().unwrap();
                    map.insert(current_stream_id, recver_tx.clone());
                }
                let mut mh = MessageHeader::new_request(0, buf.len() as u32);
                mh.set_stream_id(current_stream_id);
                let c = sender_client.get_pipe_connection();
                if let Err(e) = write_message(&c, mh, buf) {
                    //Remove current_stream_id and recver_tx to recver_map
                    {
                        let mut map = recver_map.lock().unwrap();
                        map.remove(&current_stream_id);
                    }
                    recver_tx
                        .send(Err(e))
                        .unwrap_or_else(|_e| error!("The request has returned"));
                }
            }
            trace!("Sender quit");
        });

        //Recver
        let reciever_client = client.clone();
        thread::spawn(move || {
          

            loop {
                
                match reciever_client.ready() {
                    Ok(None) => {
                        continue;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!("pipeConnection ready error {:?}", e);
                        break;
                    }
                }
                let mh;
                let buf;

                let pipe_connection = reciever_client.get_pipe_connection();

                match read_message(&pipe_connection) {
                    Ok((x, y)) => {
                        mh = x;
                        buf = y;
                    }
                    Err(x) => match x {
                        Error::Socket(y) => {
                            trace!("Socket error {}", y);
                            let mut map = recver_map_orig.lock().unwrap();
                            for (_, recver_tx) in map.iter_mut() {
                                recver_tx
                                    .send(Err(Error::Socket(format!("socket error {y}"))))
                                    .unwrap_or_else(|e| {
                                        error!("The request has returned error {:?}", e)
                                    });
                            }
                            map.clear();
                            break;
                        }
                        _ => {
                            trace!("Others error {:?}", x);
                            continue;
                        }
                    },
                };
                let mut map = recver_map_orig.lock().unwrap();
                let recver_tx = match map.get(&mh.stream_id) {
                    Some(tx) => tx,
                    None => {
                        debug!("Recver got unknown packet {:?} {:?}", mh, buf);
                        continue;
                    }
                };
                if mh.type_ != MESSAGE_TYPE_RESPONSE {
                    recver_tx
                        .send(Err(Error::Others(format!(
                            "Recver got malformed packet {mh:?} {buf:?}"
                        ))))
                        .unwrap_or_else(|_e| error!("The request has returned"));
                    continue;
                }

                recver_tx
                    .send(Ok(buf))
                    .unwrap_or_else(|_e| error!("The request has returned"));

                map.remove(&mh.stream_id);
            }

            let _ = reciever_client.close_receiver().map_err(|e| {
                warn!(
                    "failed to close with error: {:?}", e
                )
            });

            trace!("Recver quit");
        });

        Client {
            _fd: client,
            sender_tx,
        }
    }
    pub fn request(&self, req: Request) -> Result<Response> {
        let buf = req.encode().map_err(err_to_others_err!(e, ""))?;

        let (tx, rx) = mpsc::sync_channel(0);

        self.sender_tx
            .send((buf, tx))
            .map_err(err_to_others_err!(e, "Send packet to sender error "))?;

        let result = if req.timeout_nano == 0 {
            rx.recv()
                .map_err(err_to_others_err!(e, "Receive packet from recver error: "))?
        } else {
            rx.recv_timeout(Duration::from_nanos(req.timeout_nano as u64))
                .map_err(err_to_others_err!(
                    e,
                    "Receive packet from recver timeout: "
                ))?
        };

        let buf = result?;
        let res =
            Response::decode(buf).map_err(err_to_others_err!(e, "Unpack response error "))?;

        let status = res.status();
        if status.code() != Code::OK {
            return Err(Error::RpcStatus((*status).clone()));
        }

        Ok(res)
    }
}

impl Drop for ClientConnection {
    fn drop(&mut self) {
        self.close().unwrap();
        trace!("All client is dropped");
    }
}
