// SPDX-License-Identifier: Apache-2.0

#![cfg(all(unix, feature = "async"))]

use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{UnixListener, UnixStream};
use ttrpc::r#async::{
    transport::{Listener, Socket},
    Client, MethodHandler, Server, Service, TtrpcContext,
};
use ttrpc::{Request, Response, Result};

struct Echo(Arc<AtomicUsize>);

#[async_trait::async_trait]
impl MethodHandler for Echo {
    async fn handler(&self, _: TtrpcContext, req: Request) -> Result<Response> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Response {
            payload: req.payload,
            ..Default::default()
        })
    }
}

struct FailResponse {
    socket: UnixStream,
    remaining: usize,
}

impl AsyncRead for FailResponse {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_read(cx, buf)
    }
}

impl AsyncWrite for FailResponse {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.remaining == 0 {
            return Poll::Ready(Err(io::Error::other("injected server write failure")));
        }
        let len = self.remaining.min(buf.len());
        let result = Pin::new(&mut self.socket).poll_write(cx, &buf[..len]);
        if let Poll::Ready(Ok(written)) = result {
            self.remaining -= written;
        }
        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_shutdown(cx)
    }
}

#[tokio::test]
async fn server_write_failure_allows_the_next_rpc_to_reconnect_without_replay() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rpc.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let accepted = Arc::new(AtomicUsize::new(0));
    let accepts = accepted.clone();
    let incoming = async_stream::stream! {
        loop {
            match listener.accept().await {
                Ok((socket, _)) => {
                    let first = accepts.fetch_add(1, Ordering::SeqCst) == 0;
                    yield Ok(FailResponse {
                        socket,
                        remaining: if first { 5 } else { usize::MAX },
                    });
                }
                Err(error) => {
                    yield Err(error);
                    break;
                }
            }
        }
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let mut methods: HashMap<String, Box<dyn MethodHandler + Send + Sync>> = HashMap::new();
    methods.insert("Echo".into(), Box::new(Echo(calls.clone())));
    let mut services = HashMap::new();
    services.insert(
        "test.Reconnect".into(),
        Service {
            methods,
            streams: HashMap::new(),
        },
    );
    let mut server = Server::new()
        .add_listener(Listener::new(incoming))
        .register_service(services);
    server.start().await.unwrap();
    let client = Client::connect_with(move || {
        let path = path.clone();
        async move {
            UnixStream::connect(path)
                .await
                .map(Socket::from)
                .map_err(|error| ttrpc::Error::Socket(error.to_string()))
        }
    })
    .await
    .unwrap();
    let request = |payload: &[u8]| Request {
        service: "test.Reconnect".into(),
        method: "Echo".into(),
        timeout_nano: 2_000_000_000,
        payload: payload.to_vec(),
        ..Default::default()
    };
    assert!(client.request(request(b"first")).await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
    assert_eq!(
        client.request(request(b"second")).await.unwrap().payload,
        b"second"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(accepted.load(Ordering::SeqCst), 2);
    drop(client);
    tokio::time::timeout(Duration::from_secs(5), server.shutdown())
        .await
        .unwrap()
        .unwrap();
}
