// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
use tokio::io::{AsyncWriteExt, DuplexStream};
use tokio::sync::Semaphore;
use tokio::time::timeout;

const LIMIT: Duration = Duration::from_secs(5);

struct Harness {
    client: Client,
    attempts: Arc<AtomicUsize>,
    peers: mpsc::UnboundedReceiver<DuplexStream>,
    gate: Arc<Semaphore>,
}

impl Harness {
    async fn new(fail_second: bool) -> Self {
        let attempts = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(Semaphore::new(0));
        let (tx, peers) = mpsc::unbounded_channel();
        let client = Client::connect_with({
            let attempts = attempts.clone();
            let gate = gate.clone();
            move || {
                let attempts = attempts.clone();
                let gate = gate.clone();
                let tx = tx.clone();
                async move {
                    let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                    if attempt > 0 {
                        gate.acquire().await.unwrap().forget();
                    }
                    if fail_second && attempt == 1 {
                        return Err(Error::Socket("dial failed".into()));
                    }
                    let (socket, peer) = tokio::io::duplex(4096);
                    tx.send(peer).unwrap();
                    Ok(Socket::new(socket))
                }
            }
        })
        .await
        .unwrap();
        Self {
            client,
            attempts,
            peers,
            gate,
        }
    }

    async fn disconnect(&mut self) -> Arc<Session> {
        let session = self.client.session().await.unwrap();
        drop(self.peers.recv().await.unwrap());
        timeout(LIMIT, session.req_tx.closed()).await.unwrap();
        session
    }

    fn allow_connection(&self) {
        self.gate.add_permits(1);
    }
}

async fn read_request(peer: &mut DuplexStream) -> (u32, Request) {
    let message = timeout(LIMIT, GenMessage::read_from(peer))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(message.header.type_, crate::proto::MESSAGE_TYPE_REQUEST);
    (
        message.header.stream_id,
        Request::decode(message.payload).unwrap(),
    )
}

async fn respond(peer: &mut DuplexStream, id: u32, response: Response) {
    GenMessage::new_response(id, response.encode().unwrap())
        .write_to(peer)
        .await
        .unwrap();
}

#[tokio::test]
async fn initial_connection_failure_is_returned_without_retry() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let result = Client::connect_with({
        let attempts = attempts.clone();
        move || {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Err(Error::Socket("initial dial failed".into())) }
        }
    })
    .await;
    assert!(matches!(result, Err(Error::Socket(_))));
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn truncated_response_is_not_replayed_and_next_request_reconnects() {
    let mut harness = Harness::new(false).await;
    let old = harness.client.session().await.unwrap();
    let mut peer = harness.peers.recv().await.unwrap();
    let request = harness.client.request(Request::default());
    tokio::pin!(request);
    assert!(futures::poll!(request.as_mut()).is_pending());
    let (_, _) = read_request(&mut peer).await;
    // The operation has reached the server. Only part of its response arrives
    // before the transport fails, so replaying it could duplicate side effects.
    peer.write_all(&[0; 5]).await.unwrap();
    drop(peer);
    assert!(timeout(LIMIT, request).await.unwrap().is_err());
    assert_eq!(harness.attempts.load(Ordering::SeqCst), 1);
    timeout(LIMIT, old.req_tx.closed()).await.unwrap();

    harness.allow_connection();
    let request = harness.client.request(Request::default());
    tokio::pin!(request);
    assert!(futures::poll!(request.as_mut()).is_pending());
    let mut peer = harness.peers.recv().await.unwrap();
    let (id, _) = read_request(&mut peer).await;
    respond(&mut peer, id, Response::default()).await;
    assert!(timeout(LIMIT, request).await.unwrap().is_ok());
    assert_eq!(harness.attempts.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn concurrent_clones_share_a_connection_and_keep_requests_separate() {
    let mut harness = Harness::new(false).await;
    harness.disconnect().await;
    let clients = vec![harness.client.clone(); 16];
    let requests = futures::future::join_all(clients.iter().enumerate().map(|(i, client)| {
        client.request(Request {
            payload: vec![i as u8],
            ..Default::default()
        })
    }));
    tokio::pin!(requests);
    assert!(futures::poll!(requests.as_mut()).is_pending());
    assert_eq!(harness.attempts.load(Ordering::SeqCst), 2);
    harness.allow_connection();
    assert!(futures::poll!(requests.as_mut()).is_pending());
    let mut peer = harness.peers.recv().await.unwrap();
    let mut ids = std::collections::HashSet::new();
    for _ in 0..16 {
        let (id, req) = read_request(&mut peer).await;
        assert!(ids.insert(id));
        respond(
            &mut peer,
            id,
            Response {
                payload: req.payload,
                ..Default::default()
            },
        )
        .await;
    }
    let results = timeout(LIMIT, requests).await.unwrap();
    for (i, result) in results.into_iter().enumerate() {
        assert_eq!(result.unwrap().payload, vec![i as u8]);
    }
    assert_eq!(harness.attempts.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn failed_attempt_is_shared_and_only_a_later_call_retries() {
    let mut harness = Harness::new(true).await;
    harness.disconnect().await;
    let client = harness.client.clone();
    let requests = futures::future::join_all((0..16).map(|_| client.request(Request::default())));
    tokio::pin!(requests);
    assert!(futures::poll!(requests.as_mut()).is_pending());
    harness.allow_connection();
    for result in timeout(LIMIT, requests).await.unwrap() {
        assert_eq!(result, Err(Error::Socket("dial failed".into())));
    }
    assert_eq!(harness.attempts.load(Ordering::SeqCst), 2);

    harness.allow_connection();
    let request = harness.client.request(Request::default());
    tokio::pin!(request);
    assert!(futures::poll!(request.as_mut()).is_pending());
    let mut peer = harness.peers.recv().await.unwrap();
    let (id, _) = read_request(&mut peer).await;
    respond(&mut peer, id, Response::default()).await;
    assert!(timeout(LIMIT, request).await.unwrap().is_ok());
    assert_eq!(harness.attempts.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn one_waiter_timing_out_does_not_cancel_another_or_reset_its_deadline() {
    let mut harness = Harness::new(false).await;
    harness.disconnect().await;
    let long = harness.client.request(Request {
        timeout_nano: 2_000_000_000,
        ..Default::default()
    });
    tokio::pin!(long);
    assert!(futures::poll!(long.as_mut()).is_pending());
    let short = harness.client.request(Request {
        timeout_nano: 50_000_000,
        ..Default::default()
    });
    assert_eq!(
        timeout(LIMIT, short).await.unwrap(),
        Err(request_timeout_error())
    );
    harness.allow_connection();
    assert!(futures::poll!(long.as_mut()).is_pending());
    let mut peer = harness.peers.recv().await.unwrap();
    let (id, req) = read_request(&mut peer).await;
    assert!(req.timeout_nano > 0 && req.timeout_nano < 1_975_000_000);
    respond(&mut peer, id, Response::default()).await;
    assert!(timeout(LIMIT, long).await.unwrap().is_ok());
    assert_eq!(harness.attempts.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn cancelled_waiter_leaves_one_attempt_for_a_later_request() {
    let mut harness = Harness::new(false).await;
    harness.disconnect().await;
    {
        let request = harness.client.request(Request::default());
        tokio::pin!(request);
        assert!(futures::poll!(request.as_mut()).is_pending());
        // Dropping the only waiting RPC does not discard the stored attempt.
    }
    harness.allow_connection();
    let request = harness.client.request(Request::default());
    tokio::pin!(request);
    assert!(futures::poll!(request.as_mut()).is_pending());
    let mut peer = harness.peers.recv().await.unwrap();
    let (id, _) = read_request(&mut peer).await;
    respond(&mut peer, id, Response::default()).await;
    assert!(timeout(LIMIT, request).await.unwrap().is_ok());
    assert_eq!(harness.attempts.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn application_error_does_not_replace_a_healthy_connection() {
    let mut harness = Harness::new(false).await;
    let mut peer = harness.peers.recv().await.unwrap();
    let error = get_rpc_status(Code::INVALID_ARGUMENT, "bad request");
    for _ in 0..2 {
        let request = harness.client.request(Request::default());
        tokio::pin!(request);
        assert!(futures::poll!(request.as_mut()).is_pending());
        let (id, _) = read_request(&mut peer).await;
        respond(&mut peer, id, error.clone().into()).await;
        assert_eq!(timeout(LIMIT, request).await.unwrap(), Err(error.clone()));
    }
    assert_eq!(harness.attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn streams_stay_on_their_session_after_reconnection() {
    let mut harness = Harness::new(false).await;
    let mut old_stream = harness
        .client
        .new_stream(Request::default(), false, true)
        .await
        .unwrap();
    let old = harness.disconnect().await;
    harness.allow_connection();
    let mut new_stream = harness
        .client
        .new_stream(Request::default(), false, true)
        .await
        .unwrap();
    let new = harness.client.session().await.unwrap();
    assert!(!Arc::ptr_eq(&old, &new));
    assert_eq!(new.streams.lock().unwrap().len(), 1);
    // Both sessions may use stream ID 1. Reading/dropping the old stream must
    // not remove that ID from the new session's registry.
    assert!(timeout(LIMIT, old_stream.recv()).await.unwrap().is_err());
    drop(old_stream);
    assert_eq!(new.streams.lock().unwrap().len(), 1);
    let mut peer = harness.peers.recv().await.unwrap();
    let (id, _) = read_request(&mut peer).await;
    GenMessage {
        header: MessageHeader::new_data(id, 3),
        payload: vec![1, 2, 3],
    }
    .write_to(&mut peer)
    .await
    .unwrap();
    assert_eq!(
        timeout(LIMIT, new_stream.recv()).await.unwrap().unwrap(),
        vec![1, 2, 3]
    );
}

#[tokio::test]
async fn fixed_client_still_fails_after_disconnect() {
    let (socket, peer) = tokio::io::duplex(64);
    let client = Client::new(Socket::new(socket));
    let session = client.session().await.unwrap();
    drop(peer);
    timeout(LIMIT, session.req_tx.closed()).await.unwrap();
    assert!(timeout(LIMIT, client.request(Request::default()))
        .await
        .unwrap()
        .is_err());
    assert!(Arc::ptr_eq(&session, &client.session().await.unwrap()));
}

#[tokio::test]
async fn opening_a_stream_obeys_the_connection_wait_deadline() {
    let mut harness = Harness::new(false).await;
    harness.disconnect().await;
    let result = timeout(
        LIMIT,
        harness.client.new_stream(
            Request {
                timeout_nano: 50_000_000,
                ..Default::default()
            },
            false,
            true,
        ),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(error) if error == request_timeout_error()));
    assert_eq!(harness.attempts.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn dropping_last_client_drops_the_pending_connector() {
    struct DropFlag(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let attempts = Arc::new(AtomicUsize::new(0));
    let (tx, mut peers) = mpsc::unbounded_channel();
    let client = Client::connect_with({
        let dropped = dropped.clone();
        move || {
            let attempts = attempts.clone();
            let dropped = dropped.clone();
            let tx = tx.clone();
            async move {
                if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    let (socket, peer) = tokio::io::duplex(64);
                    tx.send(peer).unwrap();
                    return Ok(Socket::new(socket));
                }
                let _guard = DropFlag(dropped);
                std::future::pending().await
            }
        }
    })
    .await
    .unwrap();
    let old = client.session().await.unwrap();
    drop(peers.recv().await.unwrap());
    timeout(LIMIT, old.req_tx.closed()).await.unwrap();
    {
        let request = client.request(Request::default());
        tokio::pin!(request);
        assert!(futures::poll!(request.as_mut()).is_pending());
    }
    assert!(!dropped.load(Ordering::SeqCst));
    drop(client);
    assert!(dropped.load(Ordering::SeqCst));
}
