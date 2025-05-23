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
use protobuf::Message as _;
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
use crate::proto::{
    check_oversize, Code, Codec, GenMessage, Message, MessageHeader, Request, Response, Status,
    FLAG_NO_DATA, FLAG_REMOTE_CLOSED, MESSAGE_TYPE_DATA, MESSAGE_TYPE_REQUEST,
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
}

impl Default for Server {
    fn default() -> Self {
        Server {
            listeners: Vec::with_capacity(1),
            services: Arc::new(HashMap::new()),
            shutdown: shutdown::with_timeout(DEFAULT_SERVER_SHUTDOWN_TIMEOUT).0,
            stop_listen_tx: None,
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

    #[cfg(any(target_os = "linux", target_os = "android"))]
    /// # Safety
    /// The file descriptor must represent a vsock listener.
    pub unsafe fn add_vsock_listener(self, fd: RawFd) -> Result<Self> {
        let listener = Listener::from_raw_vsock_listener_fd(fd)
            .map_err(err_to_others_err!(e, "from_raw_unix_listener_fd error"))?;
        Ok(self.add_listener(listener))
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

    async fn do_start(&mut self, mut incoming: Listener) -> Result<()> {
        let services = self.services.clone();

        let shutdown_waiter = self.shutdown.subscribe();

        let (stop_listen_tx, mut stop_listen_rx) = channel(1);
        self.stop_listen_tx = Some(stop_listen_tx);

        spawn(async move {
            loop {
                select! {
                    conn = incoming.next() => {
                        if let Some(conn) = conn {
                            // Accept a new connection
                            match conn {
                                Ok(conn) => {
                                    // spawn a connection handler, would not block
                                    spawn_connection_handler(
                                        conn,
                                        services.clone(),
                                        shutdown_waiter.clone(),
                                    ).await;
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

    pub async fn accept(&mut self, conn: Socket) -> std::io::Result<()> {
        let delegate = ServerBuilder {
            services: self.services.clone(),
            streams: Arc::default(),
            shutdown_waiter: self.shutdown.subscribe(),
        };
        Connection::new(conn, delegate).run().await
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
) {
    let delegate = ServerBuilder {
        services,
        streams: Arc::new(Mutex::new(HashMap::new())),
        shutdown_waiter,
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
        }
    }
}

struct HandlerContext {
    tx: MessageSender,
    services: Arc<HashMap<String, Service>>,
    streams: Arc<Mutex<HashMap<u32, ResultSender>>>,
    // Used for waiting handler exit.
    _handler_shutdown_waiter: shutdown::Waiter,
}

impl HandlerContext {
    async fn handle_err(&self, header: MessageHeader, e: Error) {
        Self::respond(self.tx.clone(), header.stream_id, e.into())
            .await
            .map_err(|e| {
                error!("respond error got error {:?}", e);
            })
            .ok();
    }
    async fn handle_msg(&self, msg: GenMessage, wait_tx: tokio::sync::oneshot::Sender<()>) {
        let stream_id = msg.header.stream_id;

        if (stream_id % 2) != 1 {
            Self::respond_with_status(
                self.tx.clone(),
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
                        // Server: check size before sending to client
                        if let Err(e) = check_oversize(resp.compute_size() as usize, true) {
                            resp = e.into();
                        }

                        Self::respond(self.tx.clone(), stream_id, resp)
                            .await
                            .map_err(|e| {
                                error!("respond got error {:?}", e);
                            })
                            .ok();
                    }
                    None => {
                        let mut header = MessageHeader::new_data(stream_id, 0);
                        header.set_flags(FLAG_REMOTE_CLOSED | FLAG_NO_DATA);
                        let msg = GenMessage {
                            header,
                            payload: Vec::new(),
                        };

                        self.tx
                            .send(SendingMessage::new(msg))
                            .await
                            .map_err(err_to_others_err!(e, "Send packet to sender error "))
                            .ok();
                    }
                },
                Err(status) => Self::respond_with_status(self.tx.clone(), stream_id, status).await,
            },
            MESSAGE_TYPE_DATA => {
                // no need to wait data message handling
                drop(wait_tx);
                // TODO(wllenyj): Compatible with golang behavior.
                if (msg.header.flags & FLAG_REMOTE_CLOSED) == FLAG_REMOTE_CLOSED
                    && !msg.payload.is_empty()
                {
                    Self::respond_with_status(
                        self.tx.clone(),
                        stream_id,
                        get_status(
                            Code::INVALID_ARGUMENT,
                            format!(
                                "Stream id {stream_id}: data close message connot include data"
                            ),
                        ),
                    )
                    .await;
                    return;
                }
                let stream_tx = self.streams.lock().unwrap().get(&stream_id).cloned();
                if let Some(stream_tx) = stream_tx {
                    if let Err(e) = stream_tx.send(Ok(msg)).await {
                        Self::respond_with_status(
                            self.tx.clone(),
                            stream_id,
                            get_status(
                                Code::INVALID_ARGUMENT,
                                format!("Stream id {stream_id}: handling data error: {e}"),
                            ),
                        )
                        .await;
                    }
                } else {
                    Self::respond_with_status(
                        self.tx.clone(),
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
        );

        let ctx = TtrpcContext {
            mh: req_msg.header,
            metadata: context::from_pb(&req.metadata),
            timeout_nano: req.timeout_nano,
        };

        let task = spawn(async move { stream.handler(ctx, si).await });

        if !no_data {
            // Fake the first data message.
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

    async fn respond(tx: MessageSender, stream_id: u32, resp: Response) -> Result<()> {
        let payload = resp
            .encode()
            .map_err(err_to_others_err!(e, "Encode Response failed."))?;
        let msg = GenMessage {
            header: MessageHeader::new_response(stream_id, payload.len() as u32),
            payload,
        };
        tx.send(SendingMessage::new(msg))
            .await
            .map_err(err_to_others_err!(e, "Send packet to sender error "))
    }

    async fn respond_with_status(tx: MessageSender, stream_id: u32, status: Status) {
        let mut resp = Response::new();
        resp.set_status(status);
        Self::respond(tx, stream_id, resp)
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
