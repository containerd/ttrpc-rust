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

//! Synchronous ttrpc client.

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use crate::error::{Error, Result};
use crate::proto::{
    check_oversize, Codec, MessageHeader, Request, Response, ResponseInit,
    MESSAGE_TYPE_RESPONSE,
};
use crate::security_extension::serialize_aad;
use crate::sync::channel::{read_message, write_message};
use crate::sync::sys::ClientConnection;

#[cfg(feature = "security_extension")]
use std::os::unix::io::RawFd;
use crate::ConnectionContext;
#[cfg(feature = "security_extension")]
use crate::security_extension::{ConnectHook, HookOutput};

#[cfg(windows)]
use super::sys::PipeConnection;

type Sender = mpsc::Sender<(Vec<u8>, mpsc::SyncSender<Result<Vec<u8>>>)>;
type Receiver = mpsc::Receiver<(Vec<u8>, mpsc::SyncSender<Result<Vec<u8>>>)>;
type ReciverMap = Arc<Mutex<HashMap<u32, mpsc::SyncSender<Result<Vec<u8>>>>>>;

/// A cloneable, synchronous ttrpc connection.
///
/// Generated service clients wrap this type and use [`Client::request`] internally. Clones share
/// the same connection and may issue concurrent requests.
#[derive(Clone)]
pub struct Client {
    _connection: Arc<ClientConnection>,
    sender_tx: Sender,
    _conn_ctx: Arc<ConnectionContext>,
}

impl Client {
    /// Connects to a ttrpc server at `sockaddr`.
    ///
    /// See the [crate-level transport table](crate#transport-addresses) for supported address
    /// formats.
    ///
    /// # Errors
    ///
    /// Returns an error if the address is unsupported or the transport cannot connect.
    pub fn connect(sockaddr: &str) -> Result<Client> {
        let conn = ClientConnection::client_connect(sockaddr)?;

        Self::new_client(conn, None)
    }

    /// Create a sync client with a [`ConnectHook`] for security negotiation.
    ///
    /// The hook is invoked with the connection's raw file descriptor before
    /// any ttrpc messages are exchanged. It can perform handshakes and return
    /// a [`PayloadTransform`](crate::security_extension::PayloadTransform) for
    /// connection-level encryption.
    ///
    /// The fd is captured by a `ClientConnection`
    /// **before** the hook runs, so if the hook rejects, the connection's
    /// `Drop` impl closes the fd and prevents leaks.
    #[cfg(feature = "security_extension")]
    pub fn with_hook<H: ConnectHook + 'static>(fd: RawFd, hook: H) -> Result<Client> {
        // Take ownership of the fd BEFORE invoking the hook. ClientConnection
        // has a Drop impl that closes `fd` (and its internal socket_pair), so
        // if the hook rejects we just propagate the error and the Drop cleanup
        // releases the fd — no leak, no double-close.
        let conn = ClientConnection::new(fd)
            .map_err(err_to_others_err!(e, "new ClientConnection"))?;
        let output = hook.on_connect(fd).map_err(|e| {
            Error::Others(format!(
                "sync client connect hook failed (fd={}): {}",
                fd, e
            ))
        })?;
        Self::new_client(conn, Some(output))
    }

    /// Returns the per-connection metadata from the [`ConnectHook`].
    ///
    /// This is the [`ConnectionData`](crate::security_extension::ConnectionData)
    /// returned by the hook during connection establishment. Empty (default)
    /// when no hook was configured.
    #[cfg(feature = "security_extension")]
    pub fn connection_data(&self) -> &crate::security_extension::ConnectionData {
        &self._conn_ctx.data
    }

    #[cfg(unix)]
    /// Creates a client from a connected Unix file descriptor.
    ///
    /// The client takes ownership of `fd` and closes it when the last clone is dropped. The caller
    /// must not close or otherwise use the descriptor after calling this function.
    ///
    /// # Errors
    ///
    /// Returns an error if the internal wake-up socket pair cannot be created.
    pub fn new(fd: std::os::unix::io::RawFd) -> Result<Client> {
        let conn =
            ClientConnection::new(fd).map_err(err_to_others_err!(e, "new ClientConnection"))?;

        Self::new_client(conn, None)
    }

    fn new_client(
        pipe_client: ClientConnection,
        #[cfg(feature = "security_extension")]
        hook_output: Option<HookOutput>,
        #[cfg(not(feature = "security_extension"))] _hook_output: Option<()>,
    ) -> Result<Client> {
        #[cfg(feature = "security_extension")]
        let conn_ctx = Arc::new(ConnectionContext::new(hook_output));
        #[cfg(not(feature = "security_extension"))]
        let conn_ctx = Arc::new(ConnectionContext::default());
        let client = Arc::new(pipe_client);
        let weak_client = Arc::downgrade(&client);
        let (sender_tx, rx): (Sender, Receiver) = mpsc::channel();
        let recver_map_orig = Arc::new(Mutex::new(HashMap::new()));

        let receiver_map = recver_map_orig.clone();
        let connection = Arc::new(client.get_pipe_connection()?);
        let sender_client = connection.clone();

        //Sender
        let sender_ctx = conn_ctx.clone();
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

                // ── outbound transform ──
                let (mh, buf) = match sender_ctx.outbound_buf(
                    buf,
                    &serialize_aad(&mh),
                    false,
                ) {
                    Ok(transformed) => {
                        mh.length = transformed.len() as u32;
                        (mh, transformed)
                    }
                    Err(e) => {
                        {
                            let mut map = receiver_map.lock().unwrap();
                            map.remove(&current_stream_id);
                        }
                        recver_tx
                            .send(Err(e))
                            .unwrap_or_else(|_e| error!("The request has returned"));
                        continue;
                    }
                };

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

        //Reciver
        let receiver_connection = connection;
        //this thread should use weak arc for ClientConnection, otherwise the thread will occupy a reference count of ClientConnection's arc,
        //ClientConnection's drop will be not call until the thread finished. It means if all the external references are finished,
        //this thread should be release.
        let receiver_client = weak_client.clone();
        let receiver_ctx = conn_ctx.clone();
        thread::spawn(move || {
            loop {
                //The count of ClientConnection's Arc will be add one , and back to original value when this code ends. 
                if let Some(receiver_client) = receiver_client.upgrade(){
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
                } else {
                    break;
                }

                match read_message(&receiver_connection) {
                    Ok((mh, y)) => {
                        let buf = match y {
                            Ok(data) => receiver_ctx.inbound_buf(
                                data,
                                &serialize_aad(&mh),
                                false,
                            ),
                            Err(e) => Err(e),
                        };
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

            trace!("Receiver quit");
        });

        Ok(Client {
            _connection: client,
            sender_tx,
            _conn_ctx: conn_ctx,
        })
    }
    /// Sends a unary request and blocks until its response arrives.
    ///
    /// A nonzero [`Request::timeout_nano`] limits how long this method waits. Generated clients
    /// construct the request and decode its payload, so most applications do not call this method
    /// directly.
    ///
    /// # Errors
    ///
    /// Returns an error when the request is oversized, serialization or transport fails, the
    /// timeout expires, the response is malformed, or the server returns a non-OK status.
    pub fn request(&self, req: Request) -> Result<Response> {
        let buf = req
            .encode()
            .map_err(err_to_others_err!(e, "Encode request error "))?;
        // Validate the complete encoded request (envelope + protobuf length
        // prefixes) instead of only the payload length.
        check_oversize(buf.len(), false)?;
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
        if let Some(status) = <Response as ResponseInit>::non_ok(&res) {
            return Err(Error::RpcStatus(status));
        }

        Ok(res)
    }
}

impl Drop for ClientConnection {
    fn drop(&mut self) {
        //close all fd , make sure all fd have been release
        self.close().unwrap();
        self.close_receiver().unwrap();
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
