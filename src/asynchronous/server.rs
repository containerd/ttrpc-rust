// Copyright 2022 Alibaba Cloud. All rights reserved.
// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use std::collections::HashMap;
use std::convert::TryFrom;
#[cfg(unix)]
use std::os::unix::io::RawFd;
use std::result::Result as StdResult;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt as _;
use protobuf::Message as PbMessage;
use tokio::{
    self, select, spawn,
    sync::mpsc::{channel, Sender},
    task,
    time::timeout,
};

use crate::asynchronous::stream::SendingMessage;
use crate::asynchronous::transport::{Listener, Socket};
use crate::context;
use crate::error::{get_status, Error, Result};
use crate::ConnectionContext;
#[cfg(feature = "security_extension")]
use crate::security_extension::{AcceptHook, ServerExtensionConfig};
use crate::proto::{
    check_oversize, Code, Codec, GenMessage, Message, MessageHeader, Request, Response, Status,
    FLAG_NO_DATA, MESSAGE_TYPE_DATA, MESSAGE_TYPE_REQUEST,
};
use crate::r#async::connection::*;
use crate::r#async::shutdown;
use crate::r#async::stream::{
    Kind, MessageReceiver, MessageSender, ResultReceiver, ResultSender, StreamInner,
};
use crate::r#async::utils;
use crate::r#async::{MethodHandler, StreamHandler, TtrpcContext};

const DEFAULT_CONN_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_SERVER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Service {
    pub methods: HashMap<String, Box<dyn MethodHandler + Send + Sync>>,
    pub streams: HashMap<String, Arc<dyn StreamHandler + Send + Sync>>,
}

impl Service {
    pub(crate) fn get_method(&self, name: &str) -> Option<&(dyn MethodHandler + Send + Sync)> {
        self.methods.get(name).map(|b| b.as_ref())
    }

    pub(crate) fn get_stream(&self, name: &str) -> Option<Arc<dyn StreamHandler + Send + Sync>> {
        self.streams.get(name).cloned()
    }
}

/// A ttrpc Server (async).
pub struct Server {
    listeners: Vec<Listener>,
    services: Arc<HashMap<String, Service>>,

    shutdown: shutdown::Notifier,
    stop_listen_tx: Option<Sender<Sender<Listener>>>,

    // ── Connection Extension Framework ──
    // See crate::security_extension for full architecture documentation.
    #[cfg(feature = "security_extension")]
    accept_hook: Option<Arc<dyn AcceptHook>>,
}

impl Default for Server {
    fn default() -> Self {
        Server {
            listeners: Vec::with_capacity(1),
            services: Arc::new(HashMap::new()),
            shutdown: shutdown::with_timeout(DEFAULT_SERVER_SHUTDOWN_TIMEOUT).0,
            stop_listen_tx: None,
            #[cfg(feature = "security_extension")]
            accept_hook: None,
        }
    }
}

impl Server {
    pub fn new() -> Server {
        Server::default()
    }

    pub fn bind(self, sockaddr: &str) -> Result<Self> {
        let listener =
            Listener::bind(sockaddr).map_err(err_to_others_err!(e, "Listener::bind error "))?;
        Ok(self.add_listener(listener))
    }

    pub fn add_listener(mut self, listener: Listener) -> Server {
        self.listeners.push(listener);
        self
    }

    #[cfg(unix)]
    /// # Safety
    /// The file descriptor must represent a unix listener.
    pub unsafe fn add_unix_listener(self, fd: RawFd) -> Result<Server> {
        let listener = Listener::from_raw_unix_listener_fd(fd)
            .map_err(err_to_others_err!(e, "from_raw_unix_listener_fd error"))?;
        Ok(self.add_listener(listener))
    }

    #[cfg(unix)]
    /// # Safety
    /// The file descriptor must represent a unix listener.
    pub unsafe fn add_tcp_listener(self, fd: RawFd) -> Result<Server> {
        let listener = Listener::from_raw_tcp_listener_fd(fd)
            .map_err(err_to_others_err!(e, "from_raw_tcp_listener_fd error"))?;
        Ok(self.add_listener(listener))
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    /// # Safety
    /// The file descriptor must represent a vsock listener.
    pub unsafe fn add_vsock_listener(self, fd: RawFd) -> Result<Self> {
        let listener = Listener::from_raw_vsock_listener_fd(fd)
            .map_err(err_to_others_err!(e, "from_raw_unix_listener_fd error"))?;
        Ok(self.add_listener(listener))
    }

    /// Register a hook called on every new accepted connection.
    /// Replaces any previously registered hook.
    ///
    /// Note (Unix): the hook requires the accepted transport
    /// [`Socket`](crate::asynchronous::transport::Socket) to carry a captured
    /// raw fd (`Socket::as_raw_fd() != None`). This is true for listeners
    /// created via `Server::bind()` / `Listener::from(<platform listener>)`,
    /// but not for sockets produced by `Listener::new()` / `Socket::new()`.
    #[cfg(feature = "security_extension")]
    pub fn set_accept_hook<H: AcceptHook + 'static>(mut self, hook: H) -> Self {
        let hook: Arc<dyn AcceptHook> = Arc::new(hook);
        self.accept_hook = Some(hook);
        self
    }

    pub fn register_service(mut self, new: HashMap<String, Service>) -> Server {
        let services = Arc::get_mut(&mut self.services).unwrap();
        services.extend(new);
        self
    }

    fn get_listener(&mut self) -> Result<Listener> {
        self.listeners.pop().ok_or_else(|| {
            Error::Others("ttrpc-rust server started with no bound listener".to_string())
        })
    }

    pub async fn start(&mut self) -> Result<()> {
        let incoming = self.get_listener()?;
        self.do_start(incoming).await
    }

    // ── Connection Extension: Injection Point 1/10 (accept hook) ──
    async fn do_start(&mut self, mut incoming: Listener) -> Result<()> {
        let services = self.services.clone();

        let shutdown_waiter = self.shutdown.subscribe();

        let (stop_listen_tx, mut stop_listen_rx) = channel(1);
        self.stop_listen_tx = Some(stop_listen_tx);

        #[cfg(feature = "security_extension")]
        let server_ext = Arc::new(ServerExtensionConfig {
            accept_hook: self.accept_hook.clone(),
        });

        spawn(async move {
            loop {
                select! {
                    conn = incoming.next() => {
                        if let Some(conn) = conn {
                            // Accept a new connection
                            match conn {
                                Ok(conn) => {
                                    // Spawn hook + handler setup per-connection
                                    // so the accept loop can immediately return
                                    // to accepting new connections.
                                    #[cfg(feature = "security_extension")]
                                    let server_ext = server_ext.clone();
                                    let services = services.clone();
                                    let shutdown_waiter = shutdown_waiter.clone();
                                    spawn(async move {
                                        // ── Injection Point 1/10: accept hook ──
                                        #[cfg(feature = "security_extension")]
                                        let conn_ctx = match server_ext.on_accept(&conn).await {
                                            Ok(output) => Arc::new(ConnectionContext::new(output)),
                                            Err(e) => {
                                                log::warn!("accept hook failed for connection: {:?}", e);
                                                return;
                                            }
                                        };
                                        #[cfg(not(feature = "security_extension"))]
                                        let conn_ctx = Arc::new(ConnectionContext::default());

                                        spawn_connection_handler(
                                            conn,
                                            services,
                                            shutdown_waiter,
                                            conn_ctx,
                                        ).await;
                                    });
                                }
                                Err(e) => {
                                    error!("incoming conn fail {:?}", e)
                                }
                            }

                        } else {
                            break;
                        }
                    }
                    fd_tx = stop_listen_rx.recv() => {
                        if let Some(fd_tx) = fd_tx {
                            fd_tx.send(incoming).await.unwrap();
                        }
                        break;
                    }
                }
            }
        });
        Ok(())
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        self.stop_listen().await;
        self.disconnect().await;
        drop(self.listeners.pop());
        Ok(())
    }

    pub async fn disconnect(&mut self) {
        self.shutdown.shutdown();

        self.shutdown
            .wait_all_exit()
            .await
            .map_err(|e| {
                trace!("wait connection exit error: {}", e);
            })
            .ok();
        trace!("wait connection exit.");
    }

    pub async fn stop_listen(&mut self) {
        if let Some(tx) = self.stop_listen_tx.take() {
            let (fd_tx, mut fd_rx) = channel(1);
            tx.send(fd_tx).await.unwrap();

            let fd = fd_rx.recv().await.unwrap();
            self.listeners.clear();
            self.listeners.push(fd);
        }
    }
}

async fn spawn_connection_handler(
    conn: Socket,
    services: Arc<HashMap<String, Service>>,
    shutdown_waiter: shutdown::Waiter,
    conn_ctx: Arc<ConnectionContext>,
) {
    let delegate = ServerBuilder {
        services,
        streams: Arc::new(Mutex::new(HashMap::new())),
        shutdown_waiter,
        conn_ctx,
    };
    let conn = Connection::new(conn, delegate);
    spawn(async move {
        conn.run()
            .await
            .map_err(|e| {
                trace!("connection run error. {}", e);
            })
            .ok();
    });
}

struct ServerBuilder {
    services: Arc<HashMap<String, Service>>,
    streams: Arc<Mutex<HashMap<u32, ResultSender>>>,
    shutdown_waiter: shutdown::Waiter,
    conn_ctx: Arc<ConnectionContext>,
}

impl Builder for ServerBuilder {
    type Reader = ServerReader;
    type Writer = ServerWriter;

    fn build(&mut self) -> (Self::Reader, Self::Writer) {
        let (tx, rx): (MessageSender, MessageReceiver) = channel(100);
        let (disconnect_notifier, _disconnect_waiter) =
            shutdown::with_timeout(DEFAULT_CONN_SHUTDOWN_TIMEOUT);

        (
            ServerReader {
                tx,
                services: self.services.clone(),
                streams: self.streams.clone(),
                server_shutdown: self.shutdown_waiter.clone(),
                handler_shutdown: disconnect_notifier,
                conn_ctx: self.conn_ctx.clone(),
            },
            ServerWriter {
                rx,
                _server_shutdown: self.shutdown_waiter.clone(),
            },
        )
    }
}

struct ServerWriter {
    rx: MessageReceiver,
    _server_shutdown: shutdown::Waiter,
}

#[async_trait]
impl WriterDelegate for ServerWriter {
    async fn recv(&mut self) -> Option<SendingMessage> {
        self.rx.recv().await
    }
    async fn disconnect(&self, _msg: &GenMessage, _: Error) {}
    async fn exit(&self) {}
}

struct ServerReader {
    tx: MessageSender,
    services: Arc<HashMap<String, Service>>,
    streams: Arc<Mutex<HashMap<u32, ResultSender>>>,
    server_shutdown: shutdown::Waiter,
    handler_shutdown: shutdown::Notifier,
    conn_ctx: Arc<ConnectionContext>,
}

#[async_trait]
impl ReaderDelegate for ServerReader {
    async fn wait_shutdown(&self) {
        self.server_shutdown.wait_shutdown().await
    }

    async fn disconnect(&self, _: Error, _: &mut task::JoinHandle<()>) {
        self.handler_shutdown.shutdown();
        // TODO: Don't wait for all requests to complete? when the connection is disconnected.
    }

    async fn exit(&self) {
        // TODO: Don't self.conn_shutdown.shutdown();
        // Wait pedding request/stream to exit.
        self.handler_shutdown
            .wait_all_exit()
            .await
            .map_err(|e| {
                trace!("wait handler exit error: {}", e);
            })
            .ok();
    }

    async fn handle_msg(&self, msg: GenMessage) {
        let handler_shutdown_waiter = self.handler_shutdown.subscribe();
        let context = self.context();
        //Check if it is already shutdown no need select wait
        if !handler_shutdown_waiter.is_shutdown() {
            let (wait_tx, wait_rx) = tokio::sync::oneshot::channel::<()>();

            // ── Inbound transform for ALL frames in wire order ──
            // Authenticate every inbound frame sequentially before any header
            // validation or routing. This prevents:
            // 1. Bypassing AAD verification by sending invalid stream_ids
            // 2. Desynchronizing stateful transforms (counter-based nonce)
            // The connection reader loop calls handle_msg sequentially,
            // ensuring transforms execute in wire order.
            let mut msg = msg;
            let is_request = msg.header.type_ == MESSAGE_TYPE_REQUEST;
            if let Err(e) = self.conn_ctx.inbound(&mut msg, is_request) {
                // Transform failure: drop the frame silently.
                // Do NOT use any unauthenticated header fields (type,
                // stream_id) for routing — an attacker could tamper with
                // them to redirect errors to unrelated active streams.
                // Affected stream handlers will time out via their own
                // request deadline or be cleaned up on disconnect.
                error!("transform inbound failed (frame dropped): {}", e);
                return;
            }

            spawn(async move {
                select! {
                    _ = context.handle_msg(msg, wait_tx) => {}
                    _ = handler_shutdown_waiter.wait_shutdown() => {}
                }
            });
            wait_rx.await.unwrap_or_default();
        }
    }

    async fn handle_err(&self, header: MessageHeader, e: Error) {
        self.context().handle_err(header, e).await
    }
}

impl ServerReader {
    fn context(&self) -> HandlerContext {
        HandlerContext {
            tx: self.tx.clone(),
            services: self.services.clone(),
            streams: self.streams.clone(),
            _handler_shutdown_waiter: self.handler_shutdown.subscribe(),
            conn_ctx: self.conn_ctx.clone(),
        }
    }
}

struct HandlerContext {
    tx: MessageSender,
    services: Arc<HashMap<String, Service>>,
    streams: Arc<Mutex<HashMap<u32, ResultSender>>>,
    // Used for waiting handler exit.
    _handler_shutdown_waiter: shutdown::Waiter,
    conn_ctx: Arc<ConnectionContext>,
}

impl HandlerContext {
    async fn handle_err(&self, header: MessageHeader, e: Error) {
        self.respond(header.stream_id, e.into())
            .await
            .map_err(|e| {
                error!("respond error got error {:?}", e);
            })
            .ok();
    }
    async fn handle_msg(&self, msg: GenMessage, wait_tx: tokio::sync::oneshot::Sender<()>) {
        let stream_id = msg.header.stream_id;

        if (stream_id % 2) != 1 {
            self.respond_with_status(
                stream_id,
                get_status(Code::INVALID_ARGUMENT, "stream id must be odd"),
            )
            .await;
            return;
        }

        match msg.header.type_ {
            MESSAGE_TYPE_REQUEST => match self.handle_request(msg, wait_tx).await {
                Ok(opt_msg) => match opt_msg {
                    Some(mut resp) => {
                        if let Err(e) = check_oversize(resp.compute_size() as usize, true) {
                            resp = e.into();
                        }
                        if let Err(e) = self.respond(stream_id, resp).await {
                            // respond() handles oversize internally via pre-check.
                            // Remaining failures (channel closed, transform error)
                            // are not recoverable per-request. The connection
                            // stays open until the next read error or client
                            // disconnect tears it down.
                            error!("respond failed for stream {}: {}", stream_id, e);
                        }
                    }
                    None => {
                        let mut msg = GenMessage::new_close(stream_id);
                        if let Err(e) = self.conn_ctx.transform_send(&mut msg, &self.tx, false, false).await {
                            error!("transform close message failed: {}", e);
                        }
                    }
                },
                Err(status) => self.respond_with_status(stream_id, status).await,
            },
            MESSAGE_TYPE_DATA => {
                // no need to wait data message handling
                drop(wait_tx);

                // DATA transform already applied in ServerReader::handle_msg()
                // (wire order, before spawn).
                let stream_tx = self.streams.lock().unwrap().get(&stream_id).cloned();
                if let Some(stream_tx) = stream_tx {
                    if let Err(e) = stream_tx.send(Ok(msg)).await {
                        self.respond_with_status(
                            stream_id,
                            get_status(
                                Code::INVALID_ARGUMENT,
                                format!("Stream id {stream_id}: handling data error: {e}"),
                            ),
                        )
                        .await;
                    }
                } else {
                    self.respond_with_status(
                        stream_id,
                        get_status(Code::INVALID_ARGUMENT, "Stream is no longer active"),
                    )
                    .await;
                }
            }
            _ => {
                // TODO: else we must ignore this for future compat. log this?
                // TODO(wllenyj): Compatible with golang behavior.
                error!("Unknown message type. {:?}", msg.header);
            }
        }
    }

    async fn handle_request(
        &self,
        msg: GenMessage,
        wait_tx: tokio::sync::oneshot::Sender<()>,
    ) -> StdResult<Option<Response>, Status> {
        //TODO:
        //if header.stream_id <= self.last_stream_id {
        //    return Err;
        //}
        // self.last_stream_id = header.stream_id;

        // ── REQUEST transform_inbound already applied in ServerReader::handle_msg() ──
        // (wire order, before any header validation or routing)

        let req_msg = Message::<Request>::try_from(msg)
            .map_err(|e| get_status(Code::INVALID_ARGUMENT, e.to_string()))?;

        let req = &req_msg.payload;
        trace!("Got Message request {} {}", req.service, req.method);

        let srv = self.services.get(&req.service).ok_or_else(|| {
            get_status(
                Code::INVALID_ARGUMENT,
                format!("{} service does not exist", &req.service),
            )
        })?;

        if let Some(method) = srv.get_method(&req.method) {
            drop(wait_tx);
            return self.handle_method(method, req_msg).await;
        }
        if let Some(stream) = srv.get_stream(&req.method) {
            return self.handle_stream(stream, req_msg, wait_tx).await;
        }
        Err(get_status(
            Code::UNIMPLEMENTED,
            format!("{} method", &req.method),
        ))
    }

    async fn handle_method(
        &self,
        method: &(dyn MethodHandler + Send + Sync),
        req_msg: Message<Request>,
    ) -> StdResult<Option<Response>, Status> {
        let req = req_msg.payload;
        let path = utils::get_path(&req.service, &req.method);

        let ctx = TtrpcContext {
            mh: req_msg.header,
            metadata: context::from_pb(&req.metadata),
            timeout_nano: req.timeout_nano,
            connection_data: self.conn_ctx.data.clone(),
        };

        let get_unknown_status_and_log_err = |e| {
            error!("method handle {} got error {:?}", path, &e);
            get_status(Code::UNKNOWN, e)
        };
        if req.timeout_nano == 0 {
            method
                .handler(ctx, req)
                .await
                .map_err(get_unknown_status_and_log_err)
                .map(Some)
        } else {
            timeout(
                Duration::from_nanos(req.timeout_nano as u64),
                method.handler(ctx, req),
            )
            .await
            .map_err(|_| {
                // Timed out
                error!("method handle {} got error timed out", path);
                get_status(Code::DEADLINE_EXCEEDED, "timeout")
            })
            .and_then(|r| {
                // Handler finished
                r.map_err(get_unknown_status_and_log_err)
            })
            .map(Some)
        }
    }

    async fn handle_stream(
        &self,
        stream: Arc<dyn StreamHandler + Send + Sync>,
        req_msg: Message<Request>,
        wait_tx: tokio::sync::oneshot::Sender<()>,
    ) -> StdResult<Option<Response>, Status> {
        let stream_id = req_msg.header.stream_id;
        let req = req_msg.payload;
        let path = utils::get_path(&req.service, &req.method);

        let (tx, rx): (ResultSender, ResultReceiver) = channel(100);
        let stream_tx = tx.clone();
        self.streams.lock().unwrap().insert(stream_id, tx);

        let no_data = (req_msg.header.flags & FLAG_NO_DATA) == FLAG_NO_DATA;

        drop(wait_tx);

        let si = StreamInner::new(
            stream_id,
            self.tx.clone(),
            rx,
            true, // TODO
            true,
            Kind::Server,
            self.streams.clone(),
            self.conn_ctx.clone(),
        );

        let ctx = TtrpcContext {
            mh: req_msg.header,
            metadata: context::from_pb(&req.metadata),
            timeout_nano: req.timeout_nano,
            connection_data: self.conn_ctx.data.clone(),
        };

        let task = spawn(async move { stream.handler(ctx, si).await });

        if !no_data {
            // "Fake" the first DATA message from the stream-init REQUEST payload.
            //
            // `req.payload` has already been decrypted by `handle_request()`.
            // The payload is decrypted exactly once regardless of transform type.
            //
            // For the common duplex streaming case (`streaming_client = true`,
            // FLAG_NO_DATA set), this block is skipped and all DATA messages
            // arrive via the normal `handle_msg` route where they are
            // transformed in the connection reader (wire order).
            let msg = GenMessage {
                header: MessageHeader::new_data(stream_id, req.payload.len() as u32),
                payload: req.payload,
            };
            stream_tx.send(Ok(msg)).await.map_err(|e| {
                error!("send stream data {} got error {:?}", path, &e);
                get_status(Code::UNKNOWN, e)
            })?;
        }
        task.await
            .unwrap_or_else(|e| Err(Error::Others(format!("stream {path} task got error {e:?}"))))
            .map_err(|e| get_status(Code::UNKNOWN, e))
    }

    async fn respond(&self, stream_id: u32, resp: Response) -> Result<()> {
        let payload = resp
            .encode()
            .map_err(err_to_others_err!(e, "Encode Response failed."))?;

        // Pre-check: ensure raw payload fits within the transform-safe limit.
        // This avoids calling transform_outbound on data that would be rejected
        // post-transform, which would advance stateful transforms (e.g., AEAD
        // nonce counters) without producing a sendable message.
        let max_len = self.conn_ctx.max_raw_payload_len();
        let payload = if payload.len() > max_len {
            // Original too large — build a small error response that is
            // guaranteed to fit after transform (single transform call).
            let err_msg = format!(
                "response payload {} bytes exceeds safe limit {} bytes (after transform overhead)",
                payload.len(),
                max_len
            );
            let err_resp: Response =
                Error::RpcStatus(get_status(Code::INVALID_ARGUMENT, err_msg)).into();
            err_resp
                .encode()
                .map_err(err_to_others_err!(e, "Encode error Response failed."))?
        } else {
            payload
        };

        let mut msg = GenMessage::new_response(stream_id, payload);
        self.conn_ctx.transform_send(&mut msg, &self.tx, true, false).await
    }

    async fn respond_with_status(&self, stream_id: u32, status: Status) {
        let mut resp = Response::new();
        resp.set_status(status);
        self.respond(stream_id, resp)
            .await
            .map_err(|e| {
                error!("respond with status got error {:?}", e);
            })
            .ok();
    }
}

#[cfg(target_os = "linux")]
#[cfg(test)]
mod tests {
    use super::*;

    pub const SOCK_ADDR: &str = r"unix://@/tmp/ttrpc-server-unit-test";

    pub fn is_socket_in_use(sock_path: &str) -> bool {
        let output = std::process::Command::new("bash")
            .args(["-c", &format!("lsof -U|grep {}", sock_path)])
            .output()
            .expect("Failed to execute lsof command");

        output.status.success()
    }

    #[tokio::test]
    async fn test_server_lifetime() {
        let addr = SOCK_ADDR
            .strip_prefix("unix://@")
            .expect("socket address is not expected");
        {
            let mut server = Server::new().bind(SOCK_ADDR).unwrap();
            server.start().await.unwrap();
            assert!(is_socket_in_use(addr));
        }

        // Sleep to wait for shutdown of server caused by server's lifetime over
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        assert!(!is_socket_in_use(addr));
    }
}
