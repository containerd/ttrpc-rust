// Copyright The containerd Authors.
//
// SPDX-License-Identifier: Apache-2.0
//

//! Regressions for per-stream DATA frame handling on the async server: frames reach a handler in
//! wire order, and a stream whose handler stops receiving is failed instead of queued without
//! bound.

#![cfg(all(feature = "async", feature = "rustprotobuf", target_family = "unix"))]

use std::collections::HashMap;
use std::convert::TryInto;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use async_trait::async_trait;

use ttrpc::asynchronous::{Client, Server, Service, StreamHandler, TtrpcContext};
use ttrpc::proto::{Request, Response, ResponseInit, Status};
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

/// Waits for `done` or gives up, so a regression fails an assertion rather than hanging.
async fn wait_for(limit: Duration, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + limit;
    while !done() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// ── Ordering ────────────────────────────────────────────────────────────────

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
#[cfg(target_os = "linux")]
fn pin_to_one_cpu() {
    use nix::sched::{sched_getaffinity, sched_setaffinity, CpuSet};
    use nix::unistd::Pid;

    let allowed = sched_getaffinity(Pid::from_raw(0)).unwrap();
    let first = (0..CpuSet::count())
        .find(|&c| allowed.is_set(c).unwrap_or(false))
        .expect("no CPU available");
    let mut pinned = CpuSet::new();
    pinned.set(first).unwrap();
    sched_setaffinity(Pid::from_raw(0), &pinned).unwrap();
}

/// Each DATA frame used to be handed to its own task, which released the connection reader before
/// it queued the frame. The tasks then raced: frames reached the handler out of order, and the
/// close frame could overtake the last one and drop it.
///
/// The race needs a runtime worker to be preempted inside that window, so the test squeezes the
/// client, the server and two busy threads onto one CPU.
#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
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

// ── Queue limit ─────────────────────────────────────────────────────────────

const BIG_FRAME: usize = 1 << 20;
/// Enough 1 MiB frames to pass the server's 64 MiB per-stream queue limit.
const BIG_FRAMES: usize = 70;

struct StalledHandler {
    error: Arc<Mutex<Option<String>>>,
    received: Arc<AtomicUsize>,
}

#[async_trait]
impl StreamHandler for StalledHandler {
    async fn handler(
        &self,
        _ctx: TtrpcContext,
        mut stream: StreamInner,
    ) -> ttrpc::Result<Option<Response>> {
        // Receive nothing while the client fills the queue.
        tokio::time::sleep(Duration::from_secs(1)).await;
        loop {
            match stream.recv().await {
                Ok(_) => {
                    self.received.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    *self.error.lock().unwrap() = Some(e.to_string());
                    return Err(e);
                }
            }
        }
    }
}

/// An unbounded queue keeps a handler that stops receiving from stalling the whole connection, so
/// the stream is failed once it has queued more than the server allows.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn server_fails_a_stream_that_queues_too_much() {
    let path = socket_path("server-data-limit");
    let _ = std::fs::remove_file(&path);
    let addr = format!("unix://{path}");

    let error = Arc::new(Mutex::new(None));
    let received = Arc::new(AtomicUsize::new(0));
    let mut server = Server::new()
        .bind(&addr)
        .unwrap()
        .register_service(service(Arc::new(StalledHandler {
            error: error.clone(),
            received: received.clone(),
        })));
    server.start().await.unwrap();

    let client = Client::connect(&addr).await.unwrap();
    let stream = client
        .new_stream(stream_request(), true, false)
        .await
        .unwrap();
    for _ in 0..BIG_FRAMES {
        stream.send(vec![0u8; BIG_FRAME]).await.unwrap();
    }

    wait_for(Duration::from_secs(15), || error.lock().unwrap().is_some()).await;

    let error = error.lock().unwrap().clone();
    let received = received.load(Ordering::Relaxed);

    server.shutdown().await.unwrap();
    let _ = std::fs::remove_file(&path);

    let error = error.expect("stream over the queue limit was not failed");
    assert!(
        error.contains("not receiving"),
        "stream failed with an unrelated error: {}",
        error
    );
    // Everything queued before the limit is still delivered in order ahead of the failure.
    assert!(
        received > BIG_FRAMES / 2,
        "only {} of {} frames reached the handler before it was failed",
        received,
        BIG_FRAMES
    );
}
