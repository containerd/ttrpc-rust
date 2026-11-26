// Copyright The containerd Authors.
//
// SPDX-License-Identifier: Apache-2.0
//

//! Regressions for per-stream DATA frame handling on the async server: frames reach a handler in
//! wire order, and a stream whose handler stops receiving is failed instead of queued without
//! bound.

#![cfg(all(feature = "async", feature = "rustprotobuf", target_family = "unix"))]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use ttrpc::asynchronous::{Client, Server, Service, StreamHandler, TtrpcContext};
use ttrpc::proto::{Request, Response};
use ttrpc::r#async::StreamInner;
use ttrpc::Error;

const TEST_SERVICE: &str = "test.OrderService";
const TEST_STREAM_METHOD: &str = "SendStream";

fn service(handler: Arc<dyn StreamHandler + Send + Sync>) -> HashMap<String, Service> {
    let mut streams: HashMap<String, Arc<dyn StreamHandler + Send + Sync>> = HashMap::new();
    streams.insert(TEST_STREAM_METHOD.to_string(), handler);

    let mut services = HashMap::new();
    services.insert(
        TEST_SERVICE.to_string(),
        Service {
            methods: HashMap::new(),
            streams,
        },
    );
    services
}

fn stream_request() -> Request {
    Request {
        service: TEST_SERVICE.to_string(),
        method: TEST_STREAM_METHOD.to_string(),
        timeout_nano: 120_000_000_000,
        ..Default::default()
    }
}

fn socket_path(name: &str) -> String {
    std::env::temp_dir()
        .join(format!("ttrpc-{name}-{}", std::process::id()))
        .to_string_lossy()
        .into_owned()
}

// ── Ordering ────────────────────────────────────────────────────────────────

/// Linux only: the test pins itself to one CPU with `sched_setaffinity`.
#[cfg(target_os = "linux")]
mod ordering {
    use std::convert::TryInto;
    use std::sync::atomic::AtomicUsize;
    use std::thread;
    use std::time::Instant;

    use ttrpc::proto::{ResponseInit, Status};

    use super::*;

    /// Waits for `done` or gives up, so a regression fails an assertion rather than hanging.
    async fn wait_for(limit: Duration, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + limit;
        while !done() && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    const STREAMS: usize = 20;
    const FRAMES_PER_STREAM: u32 = 200;
    const ROUNDS: usize = 20;

    #[derive(Debug, Default)]
    struct Counters {
        received: AtomicUsize,
        out_of_order: AtomicUsize,
    }

    struct SeqHandler {
        counters: Arc<Counters>,
    }

    #[async_trait]
    impl StreamHandler for SeqHandler {
        async fn handler(
            &self,
            _ctx: TtrpcContext,
            mut stream: StreamInner,
        ) -> ttrpc::Result<Option<Response>> {
            let mut expected = 0u32;
            loop {
                match stream.recv().await {
                    Ok(buf) => {
                        let seq = u32::from_be_bytes(buf[..4].try_into().unwrap());
                        if seq != expected {
                            self.counters.out_of_order.fetch_add(1, Ordering::Relaxed);
                        }
                        expected = seq.wrapping_add(1);
                        self.counters.received.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(Error::Eof) | Err(Error::RemoteClosed) => break,
                    Err(e) => return Err(e),
                }
            }
            Ok(Some(ResponseInit::init_status(Status::default())))
        }
    }

    /// Busy threads competing with the runtime's workers for the CPU they share. Stops and joins them
    /// on drop.
    struct CpuContention {
        stop: Arc<AtomicBool>,
        threads: Vec<thread::JoinHandle<()>>,
    }

    impl CpuContention {
        fn start(threads: usize) -> Self {
            let stop = Arc::new(AtomicBool::new(false));
            let threads = (0..threads)
                .map(|_| {
                    let stop = stop.clone();
                    thread::spawn(move || {
                        while !stop.load(Ordering::Relaxed) {
                            std::hint::spin_loop();
                        }
                    })
                })
                .collect();
            Self { stop, threads }
        }
    }

    impl Drop for CpuContention {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            for t in self.threads.drain(..) {
                let _ = t.join();
            }
        }
    }

    /// Pins this thread, and every thread it spawns later, to the first CPU it may run on.
    fn pin_to_one_cpu() {
        use std::mem::{size_of, zeroed};

        // SAFETY: cpu_set_t is plain data, and both calls get a valid pointer and its size.
        unsafe {
            let mut set: libc::cpu_set_t = zeroed();
            assert_eq!(
                libc::sched_getaffinity(0, size_of::<libc::cpu_set_t>(), &mut set),
                0
            );
            let first = (0..libc::CPU_SETSIZE as usize)
                .find(|&c| libc::CPU_ISSET(c, &set))
                .expect("no CPU available");
            libc::CPU_ZERO(&mut set);
            libc::CPU_SET(first, &mut set);
            assert_eq!(
                libc::sched_setaffinity(0, size_of::<libc::cpu_set_t>(), &set),
                0
            );
        }
    }

    /// Each DATA frame used to be handed to its own task, which released the connection reader before
    /// it queued the frame. The tasks then raced: frames reached the handler out of order, and the
    /// close frame could overtake the last one and drop it.
    ///
    /// The race needs a runtime worker to be preempted inside that window, so the test squeezes the
    /// client, the server and two busy threads onto one CPU.
    #[test]
    fn server_preserves_client_stream_data_order() {
        // The pinning must not outlive the test, so it runs on its own thread.
        thread::spawn(|| {
            pin_to_one_cpu();
            let _contention = CpuContention::start(2);
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .enable_all()
                .build()
                .unwrap()
                .block_on(data_order());
        })
        .join()
        .unwrap();
    }

    async fn data_order() {
        let path = socket_path("server-data-order");
        let _ = std::fs::remove_file(&path);
        let addr = format!("unix://{path}");

        let counters = Arc::new(Counters::default());
        let mut server = Server::new()
            .bind(&addr)
            .unwrap()
            .register_service(service(Arc::new(SeqHandler {
                counters: counters.clone(),
            })));
        server.start().await.unwrap();

        let client = Client::connect(&addr).await.unwrap();

        for _ in 0..ROUNDS {
            let mut senders = Vec::with_capacity(STREAMS);
            for _ in 0..STREAMS {
                // Client-streaming only: the server never sends DATA back, which is the exec-stdin
                // shape from the bug report.
                let stream = client
                    .new_stream(stream_request(), true, false)
                    .await
                    .unwrap();
                senders.push(tokio::spawn(async move {
                    for seq in 0..FRAMES_PER_STREAM {
                        stream.send(seq.to_be_bytes().to_vec()).await.unwrap();
                    }
                    stream.close_send().await.unwrap();
                }));
            }
            for sender in senders {
                sender.await.unwrap();
            }
        }

        let expected = ROUNDS * STREAMS * FRAMES_PER_STREAM as usize;
        wait_for(Duration::from_secs(10), || {
            counters.received.load(Ordering::Relaxed) >= expected
        })
        .await;

        let received = counters.received.load(Ordering::Relaxed);
        let out_of_order = counters.out_of_order.load(Ordering::Relaxed);

        server.shutdown().await.unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(
            (out_of_order, received),
            (0, expected),
            "(frames out of order, frames received) of {} sent",
            expected
        );
    }
}

// ── Queue limit ─────────────────────────────────────────────────────────────

const BIG_FRAME: usize = 1 << 20;
/// Enough 1 MiB frames to pass the server's 64 MiB per-stream queue limit.
const BIG_FRAMES: usize = 70;

/// Never receives, and records being cancelled.
struct StalledHandler {
    cancelled: Arc<AtomicBool>,
}

struct SetOnDrop(Arc<AtomicBool>);

impl Drop for SetOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

#[async_trait]
impl StreamHandler for StalledHandler {
    async fn handler(
        &self,
        _ctx: TtrpcContext,
        _stream: StreamInner,
    ) -> ttrpc::Result<Option<Response>> {
        let _cancelled = SetOnDrop(self.cancelled.clone());
        std::future::pending().await
    }
}

/// An unbounded queue keeps a handler that stops receiving from stalling the whole connection, so
/// past the limit the server cancels the handler and fails the stream.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn server_fails_a_stream_that_queues_too_much() {
    let path = socket_path("server-data-limit");
    let _ = std::fs::remove_file(&path);
    let addr = format!("unix://{path}");

    let cancelled = Arc::new(AtomicBool::new(false));
    let mut server = Server::new()
        .bind(&addr)
        .unwrap()
        .register_service(service(Arc::new(StalledHandler {
            cancelled: cancelled.clone(),
        })));
    server.start().await.unwrap();

    let client = Client::connect(&addr).await.unwrap();
    let mut stream = client
        .new_stream(stream_request(), true, false)
        .await
        .unwrap();
    for _ in 0..BIG_FRAMES {
        stream.send(vec![0u8; BIG_FRAME]).await.unwrap();
    }

    let result = tokio::time::timeout(Duration::from_secs(15), stream.recv()).await;

    server.shutdown().await.unwrap();
    let _ = std::fs::remove_file(&path);

    match result {
        Ok(Err(Error::RpcStatus(status))) => assert_eq!(
            status.code(),
            ttrpc::Code::RESOURCE_EXHAUSTED,
            "{:?}",
            status
        ),
        other => panic!("expected RESOURCE_EXHAUSTED, got {:?}", other),
    }
    assert!(
        cancelled.load(Ordering::Relaxed),
        "handler was not cancelled"
    );
}
