// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use protobuf::{CodedInputStream, Message};
use std::collections::HashMap;
use std::os::unix::io::RawFd;
use std::sync::Arc;

use crate::asynchronous::stream::{receive, respond, respond_with_status};
use crate::common::{self, Domain, MESSAGE_TYPE_REQUEST};
use crate::error::{get_status, Error, Result};
use crate::r#async::{MethodHandler, TtrpcContext};
use crate::ttrpc::{Code, Request};
use crate::MessageHeader;
use futures::StreamExt as _;
use std::marker::Unpin;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixListener as SysUnixListener;
use tokio::{
    self,
    io::split,
    net::UnixListener,
    prelude::*,
    stream::Stream,
    sync::mpsc::{channel, Receiver, Sender},
};
use tokio_vsock::VsockListener;

/// A ttrpc Server (async).
pub struct Server {
    listeners: Vec<RawFd>,
    methods: Arc<HashMap<String, Box<dyn MethodHandler + Send + Sync>>>,
    domain: Option<Domain>,
}

impl Default for Server {
    fn default() -> Self {
        Server {
            listeners: Vec::with_capacity(1),
            methods: Arc::new(HashMap::new()),
            domain: None,
        }
    }
}

impl Server {
    pub fn new() -> Server {
        Server::default()
    }

    pub fn bind(mut self, host: &str) -> Result<Self> {
        if !self.listeners.is_empty() {
            return Err(Error::Others(
                "ttrpc-rust just support 1 host now".to_string(),
            ));
        }

        let (fd, domain) = common::do_bind(host)?;
        self.domain = Some(domain);

        self.listeners.push(fd);
        Ok(self)
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

    fn listen(&self) -> Result<RawFd> {
        if self.listeners.is_empty() {
            return Err(Error::Others("ttrpc-rust not bind".to_string()));
        }

        let listenfd = self.listeners[0];
        common::do_listen(listenfd)?;

        Ok(listenfd)
    }

    pub async fn start(&self) -> Result<()> {
        let listenfd = self.listen()?;

        match self.domain.as_ref().unwrap() {
            Domain::Unix => {
                let sys_unix_listener;
                unsafe {
                    sys_unix_listener = SysUnixListener::from_raw_fd(listenfd);
                }
                let mut unix_listener = UnixListener::from_std(sys_unix_listener).unwrap();
                let incoming = unix_listener.incoming();

                self.do_start(listenfd, incoming).await
            }
            Domain::Vsock => {
                let incoming;
                unsafe {
                    incoming = VsockListener::from_raw_fd(listenfd).incoming();
                }

                self.do_start(listenfd, incoming).await
            }
        }
    }

    pub async fn do_start<I, S>(&self, listenfd: RawFd, mut incoming: I) -> Result<()>
    where
        I: Stream<Item = std::io::Result<S>> + Unpin,
        S: AsyncRead + AsyncWrite + AsRawFd + Send + 'static,
    {
        while let Some(result) = incoming.next().await {
            match result {
                Ok(stream) => {
                    common::set_fd_close_exec(stream.as_raw_fd())?;
                    let methods = self.methods.clone();
                    tokio::spawn(async move {
                        let (mut reader, mut writer) = split(stream);
                        let (tx, mut rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = channel(100);

                        tokio::spawn(async move {
                            while let Some(buf) = rx.recv().await {
                                if let Err(e) = writer.write_all(&buf).await {
                                    error!("write_message got error: {:?}", e);
                                }
                            }
                        });

                        loop {
                            let tx = tx.clone();
                            let methods = methods.clone();

                            match receive(&mut reader).await {
                                Ok(message) => {
                                    tokio::spawn(async move {
                                        handle_request(tx, listenfd, methods, message).await;
                                    });
                                }
                                Err(e) => {
                                    trace!("error {:?}", e);
                                    break;
                                }
                            }
                        }
                    });
                }
                Err(e) => error!("{:?}", e),
            }
        }

        Ok(())
    }
}

async fn handle_request(
    tx: Sender<Vec<u8>>,
    fd: RawFd,
    methods: Arc<HashMap<String, Box<dyn MethodHandler + Send + Sync>>>,
    message: (MessageHeader, Vec<u8>),
) {
    let (header, body) = message;
    if header.type_ != MESSAGE_TYPE_REQUEST {
        return;
    }

    let mut req = Request::new();
    let merge_result;
    {
        let mut s = CodedInputStream::from_bytes(&body);
        merge_result = req.merge_from(&mut s);
    }

    if merge_result.is_err() {
        let status = get_status(Code::INVALID_ARGUMENT, "".to_string());

        if let Err(x) = respond_with_status(tx.clone(), header.stream_id, status).await {
            error!("respond get error {:?}", x);
        }
    }
    trace!("Got Message request {:?}", req);

    let path = format!("/{}/{}", req.service, req.method);
    if let Some(x) = methods.get(&path) {
        let method = x;
        let ctx = TtrpcContext { fd, mh: header };

        match method.handler(ctx, req).await {
            Ok((stream_id, body)) => {
                if let Err(x) = respond(tx.clone(), stream_id, body).await {
                    error!("respond get error {:?}", x);
                }
            }
            Err(e) => {
                error!("method handle {} get error {:?}", path, e);
            }
        }
    } else {
        let status = get_status(Code::INVALID_ARGUMENT, format!("{} does not exist", path));
        if let Err(e) = respond_with_status(tx, header.stream_id, status).await {
            error!("respond get error {:?}", e);
        }
    }
}
