// Regression test for per-stream DATA frame ordering on the async client.
//
// A server-streaming handler sends 10 DATA frames (seq 0..9) per stream.
// The client opens 200 concurrent streams on a multi-threaded runtime and
// asserts that every stream receives its DATA frames in sequence order.
// With the old spawn-per-frame design, 200 concurrent streams create enough
// task-list separation between same-stream DATA frames to trigger reordering.

mod protocols;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use protocols::asynchronous::{empty, streaming, streaming_ttrpc};
use ttrpc::asynchronous::{Client, Server};

const SOCK: &str = "unix:///tmp/ttrpc-test-data-order";
const ITERATIONS: usize = 100;
const FRAMES_PER_STREAM: u32 = 50;
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
        _s: ::ttrpc::r#async::ServerStreamSender<streaming::EchoPayload>,
    ) -> ::ttrpc::Result<()> {
        unimplemented!()
    }

    async fn server_send_stream(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        _: empty::Empty,
        s: ::ttrpc::r#async::ServerStreamSender<streaming::EchoPayload>,
    ) -> ::ttrpc::Result<()> {
        for seq in 0..FRAMES_PER_STREAM {
            s.send(&streaming::EchoPayload {
                seq,
                msg: format!("{}", seq),
                ..Default::default()
            })
            .await
            .unwrap();
        }
        Ok(())
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

    let out_of_order = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::with_capacity(ITERATIONS);

    for _ in 0..ITERATIONS {
        let sc = sc.clone();
        let out_of_order = out_of_order.clone();
        handles.push(tokio::spawn(async move {
            let ctx = ttrpc::context::with_timeout(10_000_000_000);
            let mut stream: ttrpc::asynchronous::ClientStreamReceiver<streaming::EchoPayload> = sc
                .server_send_stream(ctx, &empty::Empty::default())
                .await
                .expect("failed to open stream");

            let mut expected_seq: u32 = 0;
            let mut saw_reorder = false;
            while let Some(payload) = stream.recv().await.unwrap() {
                if payload.seq != expected_seq {
                    saw_reorder = true;
                }
                expected_seq += 1;
            }
            if saw_reorder {
                out_of_order.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    for h in handles {
        let _ = h.await;
    }

    server.shutdown().await.unwrap();

    let bad = out_of_order.load(Ordering::Relaxed);
    eprintln!(
        "repeating the experiment {} times, we find out of order frames {}/{} times",
        ITERATIONS, bad, ITERATIONS
    );
    assert_eq!(
        bad, 0,
        "repeating the experiment {} times, we find out of order frames {}/{} times",
        ITERATIONS, bad, ITERATIONS
    );
}
