// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use std::collections::HashMap;
use std::convert::TryFrom;
use std::marker::Unpin;
use std::os::unix::io::RawFd;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixListener as SysUnixListener;
use std::result::Result as StdResult;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::stream::Stream;
use futures::StreamExt as _;
use nix::unistd;
use tokio::{
    self,
    io::{AsyncRead, AsyncWrite},
    net::UnixListener,
    select, spawn,
    sync::mpsc::{channel, Sender},
    task,
    time::timeout,
};
#[cfg(target_os = "linux")]
use tokio_vsock::VsockListener;

use crate::asynchronous::unix_incoming::UnixIncoming;
use crate::common::{self, Domain};
use crate::context;
use crate::error::{get_status, Error, Result};
use crate::proto::{
    Code, Codec, GenMessage, Message, MessageHeader, Request, Response, Status,
    MESSAGE_TYPE_REQUEST,
};
use crate::r#async::connection::*;
use crate::r#async::shutdown;
use crate::r#async::stream::{MessageReceiver, MessageSender};
use crate::r#async::utils;
use crate::r#async::{MethodHandler, TtrpcContext};

const DEFAULT_CONN_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(5000);
const DEFAULT_SERVER_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(10000);

/// A ttrpc Server (async).
pub struct Server {
    listeners: Vec<RawFd>,
    methods: Arc<HashMap<String, Box<dyn MethodHandler + Send + Sync>>>,
    domain: Option<Domain>,

    shutdown: shutdown::Notifier,
    stop_listen_tx: Option<Sender<Sender<RawFd>>>,
}

impl Default for Server {
    fn default() -> Self {
        Server {
            listeners: Vec::with_capacity(1),
            methods: Arc::new(HashMap::new()),
            domain: None,
            shutdown: shutdown::with_timeout(DEFAULT_SERVER_SHUTDOWN_TIMEOUT).0,
            stop_listen_tx: None,
        }
    }
}

impl Server {
    pub fn new() -> Server {
        Server::default()
    }

    pub fn bind(mut self, sockaddr: &str) -> Result<Self> {
        if !self.listeners.is_empty() {
            return Err(Error::Others(
                "ttrpc-rust just support 1 sockaddr now".to_string(),
            ));
        }

        let (fd, domain) = common::do_bind(sockaddr)?;
        self.domain = Some(domain);

        common::do_listen(fd)?;
        self.listeners.push(fd);
        Ok(self)
    }

    pub fn set_domain_unix(mut self) -> Self {
        self.domain = Some(Domain::Unix);
        self
    }

    #[cfg(target_os = "linux")]
    pub fn set_domain_vsock(mut self) -> Self {
        self.domain = Some(Domain::Vsock);
        self
    }

    pub fn add_listener(mut self, fd: RawFd) -> Result<Server> {
        self.listeners.push(fd);

        Ok(self)
    }

    pub fn register_service(
        mut self,
        methods: HashMap<String, Box<dyn MethodHandler + Send + Sync>>,
    ) -> Server {
        let mut_methods = Arc::get_mut(&mut self.methods).unwrap();
        mut_methods.extend(methods);
        self
    }

    fn get_listenfd(&self) -> Result<RawFd> {
        if self.listeners.is_empty() {
            return Err(Error::Others("ttrpc-rust not bind".to_string()));
        }

        let listenfd = self.listeners[self.listeners.len() - 1];
        Ok(listenfd)
    }

    pub async fn start(&mut self) -> Result<()> {
        let listenfd = self.get_listenfd()?;

        match self.domain.as_ref() {
            Some(Domain::Unix) => {
                let sys_unix_listener;
                unsafe {
                    sys_unix_listener = SysUnixListener::from_raw_fd(listenfd);
                }
                sys_unix_listener
                    .set_nonblocking(true)
                    .map_err(err_to_others_err!(e, "set_nonblocking error "))?;
                let unix_listener = UnixListener::from_std(sys_unix_listener)
                    .map_err(err_to_others_err!(e, "from_std error "))?;

                let incoming = UnixIncoming::new(unix_listener);

                self.do_start(incoming).await
            }
            #[cfg(target_os = "linux")]
            Some(Domain::Vsock) => {
                let incoming = unsafe { VsockListener::from_raw_fd(listenfd).incoming() };
                self.do_start(incoming).await
            }
            _ => Err(Error::Others(
                "Domain is not set or not supported".to_string(),
            )),
        }
    }

    async fn do_start<I, S>(&mut self, mut incoming: I) -> Result<()>
    where
        I: Stream<Item = std::io::Result<S>> + Unpin + Send + 'static + AsRawFd,
        S: AsyncRead + AsyncWrite + AsRawFd + Send + 'static,
    {
        let methods = self.methods.clone();

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
                                Ok(stream) => {
                                    let fd = stream.as_raw_fd();
                                    // spawn a connection handler, would not block
                                    spawn_connection_handler(
                                        fd,
                                        stream,
                                        methods.clone(),
                                        shutdown_waiter.clone(),
                                    ).await;
                                }
                                Err(e) => {
                                    error!("{:?}", e)
                                }
                            }

                        } else {
                            break;
                        }
                    }
                    fd_tx = stop_listen_rx.recv() => {
                        if let Some(fd_tx) = fd_tx {
                            // dup fd to keep the listener open
                            // or the listener will be closed when the incoming was dropped.
                            let dup_fd = unistd::dup(incoming.as_raw_fd()).unwrap();
                            common::set_fd_close_exec(dup_fd).unwrap();
                            drop(incoming);

                            fd_tx.send(dup_fd).await.unwrap();
                            break;
                        }
                    }
                }
            }
        });
        Ok(())
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        self.stop_listen().await;
        self.disconnect().await;

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

async fn spawn_connection_handler<C>(
    fd: RawFd,
    conn: C,
    methods: Arc<HashMap<String, Box<dyn MethodHandler + Send + Sync>>>,
    shutdown_waiter: shutdown::Waiter,
) where
    C: AsyncRead + AsyncWrite + AsRawFd + Send + 'static,
{
    let delegate = ServerBuilder {
        fd,
        methods,
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

impl FromRawFd for Server {
    unsafe fn from_raw_fd(fd: RawFd) -> Self {
        Self::default().add_listener(fd).unwrap()
    }
}

impl AsRawFd for Server {
    fn as_raw_fd(&self) -> RawFd {
        self.listeners[0]
    }
}

struct ServerBuilder {
    fd: RawFd,
    methods: Arc<HashMap<String, Box<dyn MethodHandler + Send + Sync>>>,
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
                fd: self.fd,
                tx,
                methods: self.methods.clone(),
                server_shutdown: self.shutdown_waiter.clone(),
                handler_shutdown: disconnect_notifier,
            },
            ServerWriter { rx },
        )
    }
}

struct ServerWriter {
    rx: MessageReceiver,
}

#[async_trait]
impl WriterDelegate for ServerWriter {
    async fn recv(&mut self) -> Option<GenMessage> {
        self.rx.recv().await
    }
    async fn disconnect(&self, _msg: &GenMessage, _: Error) {}
    async fn exit(&self) {}
}

struct ServerReader {
    fd: RawFd,
    tx: MessageSender,
    methods: Arc<HashMap<String, Box<dyn MethodHandler + Send + Sync>>>,
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
        spawn(async move {
            select! {
                _ = context.handle_msg(msg) => {}
                _ = handler_shutdown_waiter.wait_shutdown() => {}
            }
        });
    }
}

impl ServerReader {
    fn context(&self) -> HandlerContext {
        HandlerContext {
            fd: self.fd,
            tx: self.tx.clone(),
            methods: self.methods.clone(),
            _handler_shutdown_waiter: self.handler_shutdown.subscribe(),
        }
    }
}

struct HandlerContext {
    fd: RawFd,
    tx: MessageSender,
    methods: Arc<HashMap<String, Box<dyn MethodHandler + Send + Sync>>>,
    // Used for waiting handler exit.
    _handler_shutdown_waiter: shutdown::Waiter,
}

impl HandlerContext {
    async fn handle_msg(&self, msg: GenMessage) {
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
            MESSAGE_TYPE_REQUEST => match self.handle_request(msg).await {
                Ok(opt_msg) => match opt_msg {
                    Some(msg) => {
                        Self::respond(self.tx.clone(), stream_id, msg)
                            .await
                            .map_err(|e| {
                                error!("respond got error {:?}", e);
                            })
                            .ok();
                    }
                    None => {
                        unimplemented!();
                    }
                },
                Err(status) => Self::respond_with_status(self.tx.clone(), stream_id, status).await,
            },
            _ => {
                // TODO: else we must ignore this for future compat. log this?
                // TODO(wllenyj): Compatible with golang behavior.
                error!("Unknown message type. {:?}", msg.header);
            }
        }
    }

    async fn handle_request(&self, msg: GenMessage) -> StdResult<Option<Response>, Status> {
        //TODO:
        //if header.stream_id <= self.last_stream_id {
        //    return Err;
        //}
        // self.last_stream_id = header.stream_id;

        let req_msg = Message::<Request>::try_from(msg)
            .map_err(|e| get_status(Code::INVALID_ARGUMENT, e.to_string()))?;

        let req = &req_msg.payload;
        trace!("Got Message request {} {}", req.service, req.method);

        let path = utils::get_path(&req.service, &req.method);
        let method = self.methods.get(&path).ok_or_else(|| {
            get_status(Code::INVALID_ARGUMENT, format!("{} does not exist", &path))
        })?;

        return self.handle_method(method.as_ref(), req_msg).await;
    }

    async fn handle_method(
        &self,
        method: &(dyn MethodHandler + Send + Sync),
        req_msg: Message<Request>,
    ) -> StdResult<Option<Response>, Status> {
        let req = req_msg.payload;
        let path = utils::get_path(&req.service, &req.method);

        let ctx = TtrpcContext {
            fd: self.fd,
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

    async fn respond(tx: MessageSender, stream_id: u32, resp: Response) -> Result<()> {
        let payload = resp
            .encode()
            .map_err(err_to_others_err!(e, "Encode Response failed."))?;
        let msg = GenMessage {
            header: MessageHeader::new_response(stream_id, payload.len() as u32),
            payload,
        };
        tx.send(msg)
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
