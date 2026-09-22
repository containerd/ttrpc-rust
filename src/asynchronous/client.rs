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
use tokio::{
    self,
    sync::mpsc,
    time::{timeout_at, Instant},
};

use crate::error::{get_rpc_status, Error, Result};
use crate::proto::{
    check_oversize, Code, Codec, GenMessage, Message, MessageHeader, Request, Response,
    ResponseInit, FLAG_NO_DATA, FLAG_REMOTE_CLOSED, FLAG_REMOTE_OPEN, MESSAGE_TYPE_DATA,
    MESSAGE_TYPE_RESPONSE,
};
use crate::r#async::connection::*;
use crate::r#async::stream::{
    ClientResultSender, ClientStreams, MessageReceiver, MessageSender, StreamInner,
};
#[cfg(feature = "security_extension")]
use crate::security_extension::ConnectHook;
use crate::ConnectionContext;

use super::stream::{MessageControl, SendingMessage};
use super::transport::Socket;

struct StreamRegistrationGuard<'a> {
    stream_id: u32,
    streams: &'a Mutex<HashMap<u32, ClientResultSender>>,
    active: bool,
}

impl StreamRegistrationGuard<'_> {
    fn disarm(mut self) {
        self.active = false;
    }
}

impl Drop for StreamRegistrationGuard<'_> {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        match self.streams.lock() {
            Ok(mut streams) => {
                streams.remove(&self.stream_id);
            }
            Err(e) => {
                error!("Failed to clean up stream {}: {}", self.stream_id, e);
            }
        }
    }
}

/// A cloneable asynchronous ttrpc connection.
///
/// Generated service clients wrap this type. Clones share one connection and can issue concurrent
/// unary and streaming requests.
#[derive(Clone)]
pub struct Client {
    req_tx: MessageSender,
    next_stream_id: Arc<AtomicU32>,
    streams: ClientStreams,
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
        Self::new_inner(stream, None).expect("new_inner without hook cannot fail")
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
        let deadline = if timeout_nano == 0 {
            None
        } else {
            Some(Instant::now() + std::time::Duration::from_nanos(timeout_nano as u64))
        };
        let stream_id = self.next_stream_id.fetch_add(2, Ordering::Relaxed);

        let mut msg: GenMessage = Message::new_request(stream_id, req)?
            .try_into()
            .map_err(|e: <Request as Codec>::E| Error::Others(e.to_string()))?;
        // Validate the complete encoded request (envelope + protobuf length
        // prefixes) instead of only the payload length, consistent with the
        // sync client.
        check_oversize(msg.payload.len(), false)?;

        let (tx, mut rx) = mpsc::unbounded_channel();
        let control = MessageControl::new(deadline, tx.clone());
        self.streams
            .lock()
            .map_err(|_| Error::Others("Failed to acquire lock on streams".to_string()))?
            .insert(stream_id, tx);
        let registration = StreamRegistrationGuard {
            stream_id,
            streams: self.streams.as_ref(),
            active: true,
        };

        // ── Injection Point 6/10: unary REQUEST transform_outbound ──
        self.conn_ctx
            .transform_send_with_control(&mut msg, &self.req_tx, false, false, control)
            .await?;

        let result = if let Some(deadline) = deadline {
            timeout_at(deadline, rx.recv())
            .await
            .map_err(|_| request_timeout_error())?
            .ok_or(Error::RemoteClosed)?
        } else {
            rx.recv().await.ok_or(Error::RemoteClosed)?
        };
        registration.disarm();

        let msg = result?;

        let res = Response::decode(msg.payload)
            .map_err(err_to_others_err!(e, "Unpack response error "))?;

        if let Some(status) = <Response as ResponseInit>::non_ok(&res) {
            return Err(Error::RpcStatus(status));
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
            .map_err(|e: <Request as Codec>::E| Error::Others(e.to_string()))?;
        // Validate the complete encoded request, consistent with the unary
        // path and the sync client.
        check_oversize(msg.payload.len(), false)?;

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

        let (tx, rx) = mpsc::unbounded_channel();
        self.streams
            .lock()
            .map_err(|_| Error::Others("Failed to acquire lock on streams".to_string()))?
            .insert(stream_id, tx);
        let registration = StreamRegistrationGuard {
            stream_id,
            streams: self.streams.as_ref(),
            active: true,
        };

        // ── Injection Point 8/10: stream-init REQUEST transform_outbound ──
        self.conn_ctx
            .transform_send(&mut msg, &self.req_tx, false, false)
            .await?;

        let inner = StreamInner::new_client(
            stream_id,
            self.req_tx.clone(),
            rx,
            streaming_client,
            streaming_server,
            self.streams.clone(),
            self.conn_ctx.clone(),
        );
        registration.disarm();
        Ok(inner)
    }
}

#[derive(Debug)]
struct ClientBuilder {
    rx: Option<MessageReceiver>,
    streams: ClientStreams,
    conn_ctx: Arc<ConnectionContext>,
}

impl Builder for ClientBuilder {
    type Reader = ClientReader;
    type Writer = ClientWriter;

    fn build(&mut self) -> (Self::Reader, Self::Writer) {
        (
            ClientReader {
                streams: self.streams.clone(),
                conn_ctx: self.conn_ctx.clone(),
            },
            ClientWriter {
                rx: self.rx.take().unwrap(),
            },
        )
    }
}

struct ClientWriter {
    rx: MessageReceiver,
}

#[async_trait]
impl WriterDelegate for ClientWriter {
    async fn recv(&mut self) -> Option<SendingMessage> {
        self.rx.recv().await
    }

    async fn exit(&self) {}
}

struct ClientReader {
    streams: ClientStreams,
    conn_ctx: Arc<ConnectionContext>,
}

#[async_trait]
impl ReaderDelegate for ClientReader {
    async fn wait_shutdown(&self) {
        std::future::pending().await
    }

    async fn disconnect(&self, e: Error) {
        // Take all items out of `req_map`.
        let mut map = std::mem::take(&mut *self.streams.lock().unwrap());
        // Terminate every pending RPC with the error. Enqueuing into each
        // per-RPC mailbox never waits for its consumer, so a slow or stalled
        // stream cannot block teardown of the others.
        for (stream_id, resp_tx) in map.drain() {
            if resp_tx.send(Err(e.clone())).is_err() {
                debug!(
                    "Could not deliver connection error to stream {stream_id}: response receiver was dropped"
                );
            }
        }
    }

    async fn exit(&self) {}

    async fn handle_err(&self, header: MessageHeader, e: Error) {
        self.fail_stream(header.stream_id, e);
    }

    async fn handle_msg(&self, msg: GenMessage) {
        let stream_id = msg.header.stream_id;

        // ── Inbound transform in wire order ──
        // Applied here, in the connection read loop, so that stateful
        // transforms (e.g., AEAD) observe frames in deterministic nonce order.
        let mut msg = msg;
        if let Err(e) = self.conn_ctx.inbound(&mut msg, false) {
            self.fail_stream(stream_id, e);
            return;
        }

        if let Some(resp_tx) = self.get_resp_tx(&msg.header) {
            self.send_result(stream_id, resp_tx, Ok(msg));
        }
    }
}

impl ClientReader {
    fn get_resp_tx(&self, header: &MessageHeader) -> Option<ClientResultSender> {
        let resp_tx = match header.type_ {
            MESSAGE_TYPE_RESPONSE => self.streams.lock().unwrap().remove(&header.stream_id),
            MESSAGE_TYPE_DATA => {
                let mut streams = self.streams.lock().unwrap();
                if (header.flags & FLAG_REMOTE_CLOSED) == FLAG_REMOTE_CLOSED {
                    streams.remove(&header.stream_id)
                } else {
                    streams.get(&header.stream_id).cloned()
                }
            }
            _ => {
                self.fail_stream(
                    header.stream_id,
                    Error::Others(format!("Receiver got malformed packet {header:?}")),
                );
                return None;
            }
        };

        if resp_tx.is_none() {
            debug!("Receiver got unknown packet {header:?}");
        }
        resp_tx
    }

    fn send_result(&self, stream_id: u32, resp_tx: ClientResultSender, result: Result<GenMessage>) {
        if resp_tx.send(result).is_err() {
            self.streams.lock().unwrap().remove(&stream_id);
            debug!("Dropped stream {stream_id}: response receiver was dropped");
        }
    }

    /// Terminates and unregisters one stream without affecting other streams
    /// sharing the connection.
    fn fail_stream(&self, stream_id: u32, e: Error) {
        let resp_tx = self.streams.lock().unwrap().remove(&stream_id);
        if let Some(resp_tx) = resp_tx {
            self.send_result(stream_id, resp_tx, Err(e.clone()));
            debug!("Failed stream {stream_id}: {e}");
        } else {
            debug!("Receiver got error for unknown stream {stream_id}: {e}");
        }
    }
}

#[cfg(all(test, feature = "security_extension"))]
mod security_tests {
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

#[cfg(test)]
mod teardown_tests {
    use super::*;
    use std::io;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::Duration;
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

    // A transport whose writes always fail, whose shutdown never completes, and
    // whose reads never return. This is the worst case for teardown: the read
    // side can never drive cleanup and writer.shutdown() would block forever, so
    // a pending request can only complete if the writer reports the failure to
    // Connection::run and run() tears the whole connection down.
    struct FailWriteHangShutdown;

    impl AsyncRead for FailWriteHangShutdown {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Poll::Pending
        }
    }

    impl AsyncWrite for FailWriteHangShutdown {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Err(io::Error::other("simulated write failure")))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            // Never completes: teardown must not wait on this.
            Poll::Pending
        }
    }

    // A write failure must tear the connection down and fail a pending
    // `timeout_nano == 0` request promptly, even though shutdown() never
    // completes and the read half never observes EOF.
    #[tokio::test]
    async fn write_failure_tears_down_despite_hanging_shutdown() {
        let client = Client::new(Socket::new(FailWriteHangShutdown));

        // `timeout_nano == 0` exercises the indefinite-wait path.
        let req = Request {
            timeout_nano: 0,
            ..Default::default()
        };

        let res = tokio::time::timeout(Duration::from_secs(5), client.request(req)).await;
        let rpc_result = res.expect("request must not hang: teardown should fail it promptly");
        assert!(
            rpc_result.is_err(),
            "request should fail because the connection write failed"
        );
    }

    // ClientReader::disconnect must fail every registered stream without
    // blocking on a slow or stalled receiver.
    #[tokio::test]
    async fn disconnect_does_not_block_on_stalled_stream() {
        let streams: ClientStreams = Arc::new(Mutex::new(HashMap::new()));

        // Stream 1: results are already queued, and its receiver is deliberately never read.
        let (full_tx, _full_rx) = mpsc::unbounded_channel();
        for _ in 0..200 {
            full_tx
                .send(Err(Error::Others("prefill".to_string())))
                .expect("unbounded send must not wait for its consumer");
        }
        streams.lock().unwrap().insert(1, full_tx);

        // Stream 2: active receiver waiting for a connection error.
        let (tx2, mut rx2) = mpsc::unbounded_channel();
        streams.lock().unwrap().insert(2, tx2);

        let reader = ClientReader {
            streams: streams.clone(),
            conn_ctx: Arc::new(ConnectionContext::default()),
        };

        tokio::time::timeout(
            Duration::from_secs(5),
            reader.disconnect(Error::Socket("boom".to_string())),
        )
        .await
        .expect("disconnect must not block on a stalled stream");

        // The active stream received the terminal error.
        let got = rx2.recv().await.expect("stream 2 should receive a message");
        assert!(got.is_err(), "stream 2 should be terminated with an error");

        // Every stream was drained from the map.
        assert!(
            streams.lock().unwrap().is_empty(),
            "all streams should be drained from the map"
        );
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
