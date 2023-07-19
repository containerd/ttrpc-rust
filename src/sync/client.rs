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

#[cfg(unix)]
use std::os::unix::io::RawFd;

use protobuf::Message;
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::proto::{
    check_oversize, Code, Codec, MessageHeader, Request, Response, MESSAGE_TYPE_RESPONSE,
};
use crate::sync::channel::{read_message, write_message};
use crate::sync::sys::ClientConnection;

#[cfg(windows)]
use super::sys::PipeConnection;

type Sender = mpsc::Sender<(Vec<u8>, mpsc::SyncSender<Result<Vec<u8>>>)>;
type Receiver = mpsc::Receiver<(Vec<u8>, mpsc::SyncSender<Result<Vec<u8>>>)>;
type ReciverMap = Arc<Mutex<HashMap<u32, mpsc::SyncSender<Result<Vec<u8>>>>>>;

/// A ttrpc Client (sync).
#[derive(Clone)]
pub struct Client {
    _connection: Arc<ClientConnection>,
    sender_tx: Sender,
}

impl Client {
    pub fn connect(sockaddr: &str) -> Result<Client> {
        let conn = ClientConnection::client_connect(sockaddr)?;

        Self::new_client(conn)
    }

    #[cfg(unix)]
    /// Initialize a new [`Client`] from raw file descriptor.
    pub fn new(fd: RawFd) -> Result<Client> {
        let conn = ClientConnection::new(fd);

        Self::new_client(conn)
    }

    fn new_client(pipe_client: ClientConnection) -> Result<Client> {
        let client = Arc::new(pipe_client);

        let (sender_tx, rx): (Sender, Receiver) = mpsc::channel();
        let recver_map_orig = Arc::new(Mutex::new(HashMap::new()));

        let receiver_map = recver_map_orig.clone();
        let connection = Arc::new(client.get_pipe_connection()?);
        let sender_client = connection.clone();

        //Sender
        thread::spawn(move || {
            let mut stream_id: u32 = 1;
            for (buf, recver_tx) in rx.iter() {
                let current_stream_id = stream_id;
                stream_id += 2;
                //Put current_stream_id and recver_tx to recver_map
                {
                    let mut map = receiver_map.lock().unwrap();
                    map.insert(current_stream_id, recver_tx.clone());
                }
                let mut mh = MessageHeader::new_request(0, buf.len() as u32);
                mh.set_stream_id(current_stream_id);

                if let Err(e) = write_message(&sender_client, mh, buf) {
                    //Remove current_stream_id and recver_tx to recver_map
                    {
                        let mut map = receiver_map.lock().unwrap();
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
        let receiver_connection = connection;
        let receiver_client = client.clone();
        thread::spawn(move || {
            loop {
                match receiver_client.ready() {
                    Ok(None) => {
                        continue;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!("pipeConnection ready error {:?}", e);
                        break;
                    }
                }

                match read_message(&receiver_connection) {
                    Ok((mh, buf)) => {
                        trans_resp(recver_map_orig.clone(), mh, buf);
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
            }

            let _ = receiver_client
                .close_receiver()
                .map_err(|e| warn!("failed to close with error: {:?}", e));

            trace!("Receiver quit");
        });

        Ok(Client {
            _connection: client,
            sender_tx,
        })
    }
    pub fn request(&self, req: Request) -> Result<Response> {
        check_oversize(req.compute_size() as usize, false)?;

        let buf = req.encode().map_err(err_to_others_err!(e, ""))?;
        // Notice: pure client problem can't be rpc error

        let (tx, rx) = mpsc::sync_channel(0);

        self.sender_tx
            .send((buf, tx))
            .map_err(err_to_others_err!(e, "Send packet to sender error "))?;

        let result = if req.timeout_nano == 0 {
            rx.recv().map_err(err_to_others_err!(
                e,
                "Receive packet from Receiver error: "
            ))?
        } else {
            rx.recv_timeout(Duration::from_nanos(req.timeout_nano as u64))
                .map_err(err_to_others_err!(
                    e,
                    "Receive packet from Receiver timeout: "
                ))?
        };

        let buf = result?;
        let res = Response::decode(buf).map_err(err_to_others_err!(e, "Unpack response error "))?;

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
        trace!("Client is dropped");
    }
}

// close everything up from the pipe connection on Windows
#[cfg(windows)]
impl Drop for PipeConnection {
    fn drop(&mut self) {
        self.close()
            .unwrap_or_else(|e| trace!("connection may already be closed: {}", e));
        trace!("pipe connection is dropped");
    }
}

/// Transfer the response
fn trans_resp(recver_map_orig: ReciverMap, mh: MessageHeader, buf: Result<Vec<u8>>) {
    let mut map = recver_map_orig.lock().unwrap();
    let recver_tx = match map.get(&mh.stream_id) {
        Some(tx) => tx,
        None => {
            debug!("Recver got unknown packet {:?} {:?}", mh, buf);
            return;
        }
    };
    if mh.type_ != MESSAGE_TYPE_RESPONSE {
        recver_tx
            .send(Err(Error::Others(format!(
                "Recver got malformed packet {:?} {:?}",
                mh, buf
            ))))
            .unwrap_or_else(|_e| error!("The request has returned"));
        return;
    }

    recver_tx
        .send(buf)
        .unwrap_or_else(|_e| error!("The request has returned"));

    map.remove(&mh.stream_id);
}
