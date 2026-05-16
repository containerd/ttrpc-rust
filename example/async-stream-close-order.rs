// Regression test for per-stream frame ordering on the async client.
//
// A server-streaming handler sends one payload and returns, producing a DATA
// frame immediately followed by a REMOTE_CLOSED frame on the wire. The client
// opens 200 concurrent streams in 10 batches (2000 total) on a multi-threaded
// runtime and asserts that every stream receives the payload before EOF.
// Concurrency creates scheduler contention that triggers the race far more
// reliably than sequential iterations.

mod protocols;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use protocols::asynchronous::{empty, streaming, streaming_ttrpc};
use ttrpc::asynchronous::{Client, Server};

const SOCK: &str = "unix:///tmp/ttrpc-test-close-order";
const ITERATIONS: usize = 2000;
const BATCH_SIZE: usize = 200;
const WORKER_THREADS: usize = 4;

struct Svc;

#[async_trait]
impl streaming_ttrpc::Streaming for Svc {
    async fn echo(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        _req: streaming::EchoPayload,
    ) -> ::ttrpc::Result<streaming::EchoPayload> {
        unimplemented!()
    }

    async fn echo_stream(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        _s: ::ttrpc::r#async::ServerStream<streaming::EchoPayload, streaming::EchoPayload>,
    ) -> ::ttrpc::Result<()> {
        unimplemented!()
    }

    async fn sum_stream(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        _s: ::ttrpc::r#async::ServerStreamReceiver<streaming::Part>,
    ) -> ::ttrpc::Result<streaming::Sum> {
        unimplemented!()
    }

    async fn divide_stream(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        _sum: streaming::Sum,
        _s: ::ttrpc::r#async::ServerStreamSender<streaming::Part>,
    ) -> ::ttrpc::Result<()> {
        unimplemented!()
    }

    async fn echo_null(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        _s: ::ttrpc::r#async::ServerStreamReceiver<streaming::EchoPayload>,
    ) -> ::ttrpc::Result<empty::Empty> {
        unimplemented!()
    }

    async fn echo_null_stream(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        _s: ::ttrpc::r#async::ServerStream<empty::Empty, streaming::EchoPayload>,
    ) -> ::ttrpc::Result<()> {
        unimplemented!()
    }

    async fn echo_default_value(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        _req: streaming::EchoPayload,
        s: ::ttrpc::r#async::ServerStreamSender<streaming::EchoPayload>,
    ) -> ::ttrpc::Result<()> {
        s.send(&streaming::EchoPayload {
            seq: 1,
            msg: "hello".into(),
            ..Default::default()
        })
        .await
        .unwrap();
        Ok(())
    }

    async fn server_send_stream(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        _req: empty::Empty,
        _s: ::ttrpc::r#async::ServerStreamSender<streaming::EchoPayload>,
    ) -> ::ttrpc::Result<()> {
        unimplemented!()
    }
}

fn main() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(WORKER_THREADS)
        .enable_all()
        .build()
        .unwrap()
        .block_on(run());
}

async fn run() {
    let path = SOCK.strip_prefix("unix://").unwrap();
    let _ = std::fs::remove_file(path);

    let service = streaming_ttrpc::create_streaming(Arc::new(Svc {}));
    let mut server = Server::new().bind(SOCK).unwrap().register_service(service);
    server.start().await.unwrap();

    let c = Client::connect(SOCK).await.unwrap();
    let sc = streaming_ttrpc::StreamingClient::new(c);

    let num_batches = ITERATIONS / BATCH_SIZE;
    let eof_without_payload = Arc::new(AtomicUsize::new(0));

    for _batch in 0..num_batches {
        let mut handles = Vec::with_capacity(BATCH_SIZE);
        for _ in 0..BATCH_SIZE {
            let sc = sc.clone();
            let eof_without_payload = eof_without_payload.clone();
            handles.push(tokio::spawn(async move {
                let ctx = ttrpc::context::with_timeout(10_000_000_000);
                let mut stream = sc
                    .echo_default_value(ctx, &streaming::EchoPayload::default())
                    .await
                    .expect("failed to open stream");

                match stream.recv().await {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        eof_without_payload.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => panic!("unexpected error from recv: {:?}", e),
                }
            }));
        }

        for h in handles {
            let _ = h.await;
        }
    }

    server.shutdown().await.unwrap();

    let lost = eof_without_payload.load(Ordering::Relaxed);
    eprintln!(
        "repeating the experiment {} times, we find data loss {}/{} times",
        ITERATIONS, lost, ITERATIONS
    );
    assert_eq!(
        lost, 0,
        "repeating the experiment {} times, we find data loss {}/{} times",
        ITERATIONS, lost, ITERATIONS
    );
}
