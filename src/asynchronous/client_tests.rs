use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::future::join_all;
use futures::task::AtomicWaker;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::Notify;
use tokio::time::timeout;

use super::*;

struct BlockedSocket {
    write_polls: Arc<AtomicUsize>,
    write_started: Arc<Notify>,
}

struct SinkSocket;

struct WriteGate {
    open: AtomicBool,
    bytes_written: AtomicUsize,
    waker: AtomicWaker,
}

impl WriteGate {
    fn new() -> Self {
        Self {
            open: AtomicBool::new(false),
            bytes_written: AtomicUsize::new(0),
            waker: AtomicWaker::new(),
        }
    }

    fn open(&self) {
        self.open.store(true, Ordering::Release);
        self.waker.wake();
    }

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<()> {
        if self.open.load(Ordering::Acquire) {
            return Poll::Ready(());
        }
        self.waker.register(cx.waker());
        if self.open.load(Ordering::Acquire) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

struct GatedSocket {
    gate: Arc<WriteGate>,
}

impl AsyncRead for BlockedSocket {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Pending
    }
}

impl AsyncWrite for BlockedSocket {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.write_polls.fetch_add(1, Ordering::Relaxed);
        self.write_started.notify_waiters();
        Poll::Pending
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Pending
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for SinkSocket {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Pending
    }
}

impl AsyncWrite for SinkSocket {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for GatedSocket {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Pending
    }
}

impl AsyncWrite for GatedSocket {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.gate.poll_ready(cx) {
            Poll::Ready(()) => {
                self.gate
                    .bytes_written
                    .fetch_add(buf.len(), Ordering::Relaxed);
                Poll::Ready(Ok(buf.len()))
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        self.gate.poll_ready(cx).map(|()| Ok(()))
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

fn blocked_client() -> (Client, Arc<AtomicUsize>, Arc<Notify>) {
    let write_polls = Arc::new(AtomicUsize::new(0));
    let write_started = Arc::new(Notify::new());
    let socket = BlockedSocket {
        write_polls: write_polls.clone(),
        write_started: write_started.clone(),
    };
    (
        Client::new(Socket::new(socket)),
        write_polls,
        write_started,
    )
}

fn request_with_timeout(timeout: Duration) -> Request {
    let mut req = Request::new();
    req.set_timeout_nano(timeout.as_nanos() as i64);
    req
}

#[tokio::test]
async fn request_deadline_covers_a_full_outbound_queue() {
    let (client, write_polls, _) = blocked_client();
    let mut tasks = Vec::new();

    for _ in 0..110 {
        let client = client.clone();
        tasks.push(tokio::spawn(async move {
            client
                .request(request_with_timeout(Duration::from_millis(100)))
                .await
        }));
    }

    let results = timeout(Duration::from_secs(2), join_all(tasks))
        .await
        .expect("requests must not remain blocked behind the full queue");
    assert!(results
        .into_iter()
        .all(|result| result.expect("request task panicked").is_err()));
    assert!(write_polls.load(Ordering::Relaxed) > 0);
    assert!(client.streams.lock().unwrap().is_empty());
}

#[tokio::test]
async fn response_timeout_removes_the_stream_without_closing_the_connection() {
    let client = Client::new(Socket::new(SinkSocket));

    let result = client
        .request(request_with_timeout(Duration::from_millis(50)))
        .await;

    assert!(result.is_err());
    assert!(client.streams.lock().unwrap().is_empty());
    assert!(!client.req_tx.is_closed());
}

#[tokio::test]
async fn expired_queued_request_preserves_the_timeout_error() {
    let gate = Arc::new(WriteGate::new());
    let client = Client::new(Socket::new(GatedSocket { gate: gate.clone() }));
    let blocker = GenMessage {
        header: MessageHeader::new_data(2, 0),
        payload: Vec::new(),
    };
    client
        .req_tx
        .send(SendingMessage::new(blocker))
        .await
        .unwrap();
    tokio::task::yield_now().await;

    let request = client.request(request_with_timeout(Duration::from_millis(50)));
    tokio::pin!(request);
    assert!(futures::poll!(request.as_mut()).is_pending());
    let marker = GenMessage {
        header: MessageHeader::new_data(4, 0),
        payload: Vec::new(),
    };
    client
        .req_tx
        .send(SendingMessage::new(marker))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(75)).await;
    gate.open();
    timeout(Duration::from_secs(1), async {
        while gate.bytes_written.load(Ordering::Relaxed)
            < 2 * crate::proto::MESSAGE_HEADER_LENGTH
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("writer did not process the message after the expired request");

    assert_eq!(
        request.await,
        Err(Error::Others("Request deadline elapsed".to_string()))
    );
    assert_eq!(
        gate.bytes_written.load(Ordering::Relaxed),
        2 * crate::proto::MESSAGE_HEADER_LENGTH
    );
    assert!(client.streams.lock().unwrap().is_empty());
    assert!(!client.req_tx.is_closed());
}

#[tokio::test]
async fn cancelling_an_in_progress_write_cleans_up_and_closes_the_connection() {
    let (client, _, write_started) = blocked_client();
    let request_client = client.clone();
    let request = tokio::spawn(async move { request_client.request(Request::new()).await });

    timeout(Duration::from_secs(1), write_started.notified())
        .await
        .expect("writer did not start");
    request.abort();
    request.await.expect_err("request task was not cancelled");

    timeout(Duration::from_secs(1), client.req_tx.closed())
        .await
        .expect("connection was not closed after cancelling an in-progress write");
    assert!(client.streams.lock().unwrap().is_empty());
}
