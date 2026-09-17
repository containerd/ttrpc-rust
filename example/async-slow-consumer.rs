// Copyright The containerd Authors.
//
// SPDX-License-Identifier: Apache-2.0
//

// Regression test: one stream nobody reads must not stall the connection.
//
// The client opens a server-streaming RPC that produces FRAMES_PER_STREAM
// frames and initially leaves it unread, so those frames pile up in the
// client's per-stream mailbox. While the stream sits unread, the client issues
// an unrelated unary RPC on the same connection and asserts it still
// completes. It then drains the stream and verifies that no frames were lost.
//
// If the connection reader ever awaits a stream's consumer-facing channel, the
// unread stream fills that channel, the reader blocks on it, and every other
// stream on the connection is starved - the unary RPC below then times out.

mod protocols;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use protocols::asynchronous::{empty, streaming, streaming_ttrpc};
use tokio::sync::Notify;
use ttrpc::asynchronous::{Client, Server};

const SOCK: &str = "unix:///tmp/ttrpc-test-slow-consumer";
const FRAMES_PER_STREAM: u32 = 500;
const RPC_TIMEOUT: Duration = Duration::from_secs(10);
const WORKER_THREADS: usize = 4;

struct Svc {
    burst_sent: Arc<Notify>,
}

#[async_trait]
impl streaming_ttrpc::Streaming for Svc {
    async fn echo(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        req: streaming::EchoPayload,
    ) -> ::ttrpc::Result<streaming::EchoPayload> {
        Ok(req)
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
            .await?;
        }
        self.burst_sent.notify_one();
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

    let burst_sent = Arc::new(Notify::new());
    let service = streaming_ttrpc::create_streaming(Arc::new(Svc {
        burst_sent: burst_sent.clone(),
    }));
    let mut server = Server::new().bind(SOCK).unwrap().register_service(service);
    server.start().await.unwrap();

    let c = Client::connect(SOCK).await.unwrap();
    let sc = streaming_ttrpc::StreamingClient::new(c);

    // Open the stream and deliberately leave it unread. Keeping the receiver
    // alive keeps the stream registered on the connection.
    let ctx = ttrpc::context::with_timeout(RPC_TIMEOUT.as_nanos() as i64);
    let mut unread: ttrpc::asynchronous::ClientStreamReceiver<streaming::EchoPayload> = sc
        .server_send_stream(ctx, &empty::Empty::default())
        .await
        .expect("failed to open stream");

    // Wait until every stream frame is on the wire. The unary response will
    // therefore arrive behind the overflowing stream on this connection.
    tokio::time::timeout(RPC_TIMEOUT, burst_sent.notified())
        .await
        .expect("server did not finish sending the stream burst");

    // The connection must still be able to serve an unrelated RPC.
    let ctx = ttrpc::context::with_timeout(RPC_TIMEOUT.as_nanos() as i64);
    let req = streaming::EchoPayload {
        seq: 1,
        msg: "hello".into(),
        ..Default::default()
    };
    let resp = tokio::time::timeout(RPC_TIMEOUT, sc.echo(ctx, &req))
        .await
        .expect("unary RPC stalled behind an unread stream: the connection reader is blocked")
        .expect("unary RPC failed");
    assert_eq!(resp.msg, "hello");
    assert_eq!(resp.seq, 1);

    // The unread stream remains valid. Once its consumer catches up, every
    // frame and the final close must be delivered in wire order.
    let received = tokio::time::timeout(RPC_TIMEOUT, async {
        let mut expected_seq = 0;
        while let Some(payload) = unread.recv().await.expect("slow stream failed") {
            assert_eq!(payload.seq, expected_seq);
            expected_seq += 1;
        }
        expected_seq
    })
    .await
    .expect("timed out draining the slow stream");
    assert_eq!(received, FRAMES_PER_STREAM);

    server.shutdown().await.unwrap();

    eprintln!("unary RPC completed and all {received} slow-stream frames were delivered");
}
