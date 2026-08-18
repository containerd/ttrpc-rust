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
use crate::ConnectionContext;
#[cfg(feature = "security_extension")]
use crate::security_extension::ConnectHook;
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

/// A cloneable asynchronous ttrpc connection.
///
/// Generated service clients wrap this type. Clones share one connection and can issue concurrent
/// unary and streaming requests.
#[derive(Clone)]
pub struct Client {
    req_tx: MessageSender,
    next_stream_id: Arc<AtomicU32>,
    streams: Arc<Mutex<HashMap<u32, ResultSender>>>,
    conn_ctx: Arc<ConnectionContext>,
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
    pub async fn connect(sockaddr: &str) -> Result<Client> {
        let socket = Socket::connect(sockaddr)
            .await
            .map_err(err_to_others_err!(e, "Socket::connect error "))?;
        Self::new_inner(socket, None)
    }

    #[cfg(unix)]
    /// Creates a client from a connected Unix socket descriptor.
    ///
    /// # Safety
    ///
    /// `fd` must be a valid, open, connected Unix socket. The caller must transfer exclusive
    /// ownership to the returned client and must not close or use the descriptor afterward.
    ///
    /// # Panics
    ///
    /// Panics if the descriptor cannot be configured for asynchronous I/O or if called outside a
    /// Tokio runtime.
    pub unsafe fn from_raw_unix_socket_fd(fd: RawFd) -> Client {
        let stream = unsafe { Socket::from_raw_unix_socket_fd(fd) }.unwrap();
        Self::new(stream)
    }

    /// Creates a client over a custom asynchronous [`Socket`].
    ///
    /// # Panics
    ///
    /// Panics if called outside a Tokio runtime because the client starts a background connection
    /// task.
    pub fn new(stream: Socket) -> Client {
        Self::new_inner(stream, None)
            .expect("new_inner without hook cannot fail")
    }

    /// Initialize a new [`Client`] with a connection hook.
    ///
    /// The hook is invoked synchronously during construction, receiving the
    /// socket's raw file descriptor so it can inspect peer identity (e.g.,
    /// via `getpeername`). Its [`HookOutput`](crate::security_extension::HookOutput)
    /// — connection metadata and optional payload transform — is stored in the
    /// client's [`ConnectionContext`](crate::security_extension::ConnectionContext)
    /// before this method returns.
    ///
    /// # Blocking behavior
    ///
    /// The hook may perform synchronous I/O (e.g., a cryptographic handshake).
    /// If called from an async context on a `current_thread` runtime, wrap the
    /// call in [`tokio::task::spawn_blocking`] to avoid stalling the executor:
    ///
    /// ```ignore
    /// let client = tokio::task::spawn_blocking(|| {
    ///     Client::with_hook(stream, hook)
    /// }).await??;
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the connect hook rejects or otherwise fails,
    /// allowing the caller to handle the failure. The socket is consumed
    /// regardless; on failure the caller should open a new connection.
    #[cfg(feature = "security_extension")]
    pub fn with_hook<H: ConnectHook + 'static>(stream: Socket, hook: H) -> Result<Client> {
        Self::new_inner(stream, Some(Box::new(hook)))
    }

    /// Returns the per-connection metadata from the [`ConnectHook`].
    ///
    /// This is the [`ConnectionData`](crate::security_extension::ConnectionData)
    /// returned by the hook during connection establishment. Empty (default)
    /// when no hook was configured.
    #[cfg(feature = "security_extension")]
    pub fn connection_data(&self) -> &crate::security_extension::ConnectionData {
        &self.conn_ctx.data
    }

    fn new_inner(
        stream: Socket,
        #[cfg(feature = "security_extension")] hook: Option<Box<dyn ConnectHook>>,
        #[cfg(not(feature = "security_extension"))] _hook: Option<()>,
    ) -> Result<Client> {
        // ── Injection Point 5/10: connect hook ──
        // Call connect hook if set, create ConnectionContext from output
        #[cfg(feature = "security_extension")]
        let conn_ctx = match hook {
            Some(h) => match stream.as_raw_fd() {
                Some(fd) => match h.on_connect(fd) {
                    Ok(output) => Arc::new(ConnectionContext::new(Some(output))),
                    Err(e) => {
                        return Err(Error::Others(format!(
                            "client connect hook failed (fd={}): {}",
                            fd, e
                        )));
                    }
                },
                None => {
                    return Err(Error::Others(
                        "client connect hook configured but socket has no raw fd; construct the Socket via Socket::connect or Socket::from(<platform stream>)".to_string(),
                    ));
                }
            },
            None => Arc::new(ConnectionContext::default()),
        };
        #[cfg(not(feature = "security_extension"))]
        let conn_ctx = Arc::new(ConnectionContext::default());

        let (req_tx, rx): (MessageSender, MessageReceiver) = mpsc::channel(100);
        let req_map = Arc::new(Mutex::new(HashMap::new()));
        let delegate = ClientBuilder {
            rx: Some(rx),
            streams: req_map.clone(),
            conn_ctx: conn_ctx.clone(),
        };

        let conn = Connection::new(stream, delegate);
        tokio::spawn(async move { conn.run().await });

        Ok(Client {
            req_tx,
            next_stream_id: Arc::new(AtomicU32::new(1)),
            streams: req_map,
            conn_ctx,
        })
    }

    /// Sends a unary request and waits for its response.
    ///
    /// A nonzero [`Request::timeout_nano`] limits how long this method waits. Generated clients
    /// construct the request and decode its payload, so most applications do not call this method
    /// directly.
    ///
    /// # Errors
    ///
    /// Returns an error when the request is oversized, serialization or transport fails, the
    /// timeout expires, the response is malformed, or the server returns a non-OK status.
    pub async fn request(&self, req: Request) -> Result<Response> {
        let timeout_nano = req.timeout_nano;
        let stream_id = self.next_stream_id.fetch_add(2, Ordering::Relaxed);

        let mut msg: GenMessage = Message::new_request(stream_id, req)?
            .try_into()
            .map_err(|e: protobuf::Error| Error::Others(e.to_string()))?;

        let (tx, mut rx): (ResultSender, ResultReceiver) = mpsc::channel(100);
        self.streams
            .lock()
            .map_err(|_| Error::Others("Failed to acquire lock on streams".to_string()))?
            .insert(stream_id, tx);

        // ── Injection Point 6/10: unary REQUEST transform_outbound ──
        if let Err(e) = self.conn_ctx.transform_send(&mut msg, &self.req_tx, false, false).await {
            self.streams.lock().unwrap().remove(&stream_id);
            return Err(e);
        }

        let result = if timeout_nano == 0 {
            rx.recv().await.ok_or(Error::RemoteClosed)?
        } else {
            tokio::time::timeout(
                std::time::Duration::from_nanos(timeout_nano as u64),
                rx.recv(),
            )
            .await
            .map_err(|e| Error::Others(format!("Receive packet timeout {e:?}")))?
            .ok_or(Error::RemoteClosed)?
        };

        let msg = result?;

        let res = Response::decode(msg.payload)
            .map_err(err_to_others_err!(e, "Unpack response error "))?;

        let status = res.status();
        if status.code() != Code::OK {
            return Err(Error::RpcStatus((*status).clone()));
        }

        Ok(res)
    }

    /// Opens a low-level streaming RPC.
    ///
    /// Generated streaming client methods select the appropriate `streaming_client` and
    /// `streaming_server` values and wrap the returned [`StreamInner`] in a typed stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the request is oversized, the stream registry is unavailable, the
    /// connection is closed, or a client-streaming request also contains an initial payload.
    pub async fn new_stream(
        &self,
        req: Request,
        streaming_client: bool,
        streaming_server: bool,
    ) -> Result<StreamInner> {
        let stream_id = self.next_stream_id.fetch_add(2, Ordering::Relaxed);
        let is_req_payload_empty = req.payload.is_empty();

        let mut msg: GenMessage = Message::new_request(stream_id, req)?
            .try_into()
            .map_err(|e: protobuf::Error| Error::Others(e.to_string()))?;

        if streaming_client {
            if !is_req_payload_empty {
                return Err(get_rpc_status(
                    Code::INVALID_ARGUMENT,
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

        // ── Injection Point 8/10: stream-init REQUEST transform_outbound ──
        if let Err(e) = self.conn_ctx.transform_send(&mut msg, &self.req_tx, false, false).await {
            self.streams.lock().unwrap().remove(&stream_id);
            return Err(e);
        }

        Ok(StreamInner::new(
            stream_id,
            self.req_tx.clone(),
            rx,
            streaming_client,
            streaming_server,
            Kind::Client,
            self.streams.clone(),
            self.conn_ctx.clone(),
        ))
    }
}

#[derive(Debug)]
struct ClientBuilder {
    rx: Option<MessageReceiver>,
    streams: Arc<Mutex<HashMap<u32, ResultSender>>>,
    conn_ctx: Arc<ConnectionContext>,
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
                conn_ctx: self.conn_ctx.clone(),
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
    conn_ctx: Arc<ConnectionContext>,
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
        let conn_ctx = self.conn_ctx.clone();

        // ── Inbound transform in wire order ──
        // Apply transform here (in the connection read loop) before spawning
        // a handler task. This ensures deterministic nonce sequencing for
        // stateful transforms (e.g., AEAD) regardless of task scheduling.
        let mut msg = msg;
        let result = conn_ctx.inbound(&mut msg, false);

        tokio::spawn(async move {
            if let Some(resp_tx) = get_resp_tx(req_map, &msg.header).await {
                resp_tx
                    .send(result.map(|_| msg))
                    .await
                    .unwrap_or_else(|_e| error!("The request has returned"));
            }
        });
    }
}

#[cfg(all(test, feature = "security_extension"))]
mod tests {
    use super::*;
    use crate::security_extension::{ConnectHook, ConnectionData, HookError, HookOutput};

    #[derive(Debug)]
    struct DummyConnectHook;

    impl ConnectHook for DummyConnectHook {
        fn on_connect(
            &self,
            _fd: std::os::unix::io::RawFd,
        ) -> std::result::Result<HookOutput, HookError> {
            Ok(HookOutput {
                data: ConnectionData::new(),
                payload_transform: None,
            })
        }
    }

    /// Constructing a Socket via Socket::new() leaves raw_fd == None.
    /// Client::with_hook must fail in that case instead of silently skipping
    /// the hook and using an untransformed connection.
    #[test]
    fn with_hook_requires_raw_fd() {
        let (client, _server) = tokio::io::duplex(64);
        let socket = Socket::new(client);
        let err = match Client::with_hook(socket, DummyConnectHook) {
            Ok(_) => panic!("hook configured but no fd -> should fail"),
            Err(e) => e,
        };
        let err_str = format!("{}", err);
        assert!(
            err_str.contains("socket has no raw fd"),
            "error should tell caller to use Socket::connect / Socket::from: {}",
            err_str
        );
    }
}
