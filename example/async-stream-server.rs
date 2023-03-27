// Copyright 2022 Alibaba Cloud. All rights reserved.
//
// SPDX-License-Identifier: Apache-2.0
//

mod protocols;
mod utils;

use std::sync::Arc;

use log::{info, LevelFilter};

#[cfg(unix)]
use protocols::r#async::{empty, streaming, streaming_ttrpc};
#[cfg(unix)]
use ttrpc::asynchronous::Server;

#[cfg(unix)]
use async_trait::async_trait;
#[cfg(unix)]
use tokio::signal::unix::{signal, SignalKind};
use tokio::time::sleep;

struct StreamingService;

#[cfg(unix)]
#[async_trait]
impl streaming_ttrpc::Streaming for StreamingService {
    async fn echo(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        mut e: streaming::EchoPayload,
    ) -> ::ttrpc::Result<streaming::EchoPayload> {
        e.seq += 1;
        Ok(e)
    }

    async fn echo_stream(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        mut s: ::ttrpc::r#async::ServerStream<streaming::EchoPayload, streaming::EchoPayload>,
    ) -> ::ttrpc::Result<()> {
        while let Some(mut e) = s.recv().await? {
            e.seq += 1;
            s.send(&e).await?;
        }

        Ok(())
    }

    async fn sum_stream(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        mut s: ::ttrpc::r#async::ServerStreamReceiver<streaming::Part>,
    ) -> ::ttrpc::Result<streaming::Sum> {
        let mut sum = streaming::Sum::new();
        while let Some(part) = s.recv().await? {
            sum.sum += part.add;
            sum.num += 1;
        }

        Ok(sum)
    }

    async fn divide_stream(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        sum: streaming::Sum,
        s: ::ttrpc::r#async::ServerStreamSender<streaming::Part>,
    ) -> ::ttrpc::Result<()> {
        let mut parts = vec![streaming::Part::new(); sum.num as usize];

        let mut total = 0i32;
        for i in 1..(sum.num - 2) {
            let add = (rand::random::<u32>() % 1000) as i32 - 500;
            parts[i as usize].add = add;
            total += add;
        }

        parts[sum.num as usize - 2].add = sum.sum - total;

        for part in parts {
            s.send(&part).await.unwrap();
        }

        Ok(())
    }

    async fn echo_null(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        mut s: ::ttrpc::r#async::ServerStreamReceiver<streaming::EchoPayload>,
    ) -> ::ttrpc::Result<empty::Empty> {
        let mut seq = 0;
        while let Some(e) = s.recv().await? {
            assert_eq!(e.seq, seq);
            assert_eq!(e.msg.as_str(), "non-empty empty");
            seq += 1;
        }
        Ok(empty::Empty::new())
    }

    async fn echo_null_stream(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        s: ::ttrpc::r#async::ServerStream<empty::Empty, streaming::EchoPayload>,
    ) -> ::ttrpc::Result<()> {
        let msg = "non-empty empty".to_string();

        let mut tasks = Vec::new();

        let (tx, mut rx) = s.split();
        let mut seq = 0u32;
        while let Some(e) = rx.recv().await? {
            assert_eq!(e.seq, seq);
            assert_eq!(e.msg, msg);
            seq += 1;

            for _i in 0..10 {
                let tx = tx.clone();
                tasks.push(tokio::spawn(
                    async move { tx.send(&empty::Empty::new()).await },
                ));
            }
        }

        for t in tasks {
            t.await.unwrap().map_err(|e| {
                ::ttrpc::Error::RpcStatus(::ttrpc::get_status(
                    ::ttrpc::Code::UNKNOWN,
                    e.to_string(),
                ))
            })?;
        }
        Ok(())
    }
}

#[cfg(windows)]
fn main() {
    println!("This example only works on Unix-like OSes");
}

#[cfg(unix)]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    simple_logging::log_to_stderr(LevelFilter::Info);

    let s = Box::new(StreamingService {}) as Box<dyn streaming_ttrpc::Streaming + Send + Sync>;
    let s = Arc::new(s);
    let service = streaming_ttrpc::create_streaming(s);

    utils::remove_if_sock_exist(utils::SOCK_ADDR).unwrap();

    let mut server = Server::new()
        .bind(utils::SOCK_ADDR)
        .unwrap()
        .register_service(service);

    let mut hangup = signal(SignalKind::hangup()).unwrap();
    let mut interrupt = signal(SignalKind::interrupt()).unwrap();
    server.start().await.unwrap();

    tokio::select! {
        _ = hangup.recv() => {
            // test stop_listen -> start
            info!("stop listen");
            server.stop_listen().await;
            info!("start listen");
            server.start().await.unwrap();

            // hold some time for the new test connection.
            sleep(std::time::Duration::from_secs(100)).await;
        }
        _ = interrupt.recv() => {
            // test graceful shutdown
            info!("graceful shutdown");
            server.shutdown().await.unwrap();
        }
    };
}
