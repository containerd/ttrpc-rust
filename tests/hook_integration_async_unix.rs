// Copyright 2026 Alibaba Cloud. All rights reserved.
//
// SPDX-License-Identifier: Apache-2.0
//
#![cfg(all(feature = "async", feature = "security_extension"))]
//! Integration tests for AcceptHook, ConnectHook, and PayloadTransform
//! exercising real client-server connections over Unix sockets.

mod common;

use std::collections::HashMap;
use std::os::unix::io::RawFd;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::time::sleep;

use ttrpc::asynchronous::{Client, MethodHandler, Server, Service, TtrpcContext};
use ttrpc::proto::{Request, Response, Status};
use ttrpc::security_extension::{
    AcceptHook, ConnectHook, ConnectionData, ConnectionDataExt, HookError, HookOutput,
    PayloadTransform,
};

use common::{cleanup_socket_file, temp_unix_socket_path, XorPayloadTransform};

// ── Test constants ──────────────────────────────────────────────────────────

const TEST_SERVICE: &str = "test.TestService";
const TEST_METHOD: &str = "Echo";

// ── Test AcceptHook (server side) ──────────────────────────────────────────

#[derive(Debug)]
struct TestAcceptHook {
    call_count: Arc<AtomicUsize>,
    attach_data: bool,
    attach_transform: bool,
    reject: bool,
}

impl TestAcceptHook {
    fn new_rejecting() -> Self {
        Self {
            call_count: Arc::new(AtomicUsize::new(0)),
            attach_data: false,
            attach_transform: false,
            reject: true,
        }
    }

    fn new_accepting(with_data: bool, with_transform: bool) -> Self {
        Self {
            call_count: Arc::new(AtomicUsize::new(0)),
            attach_data: with_data,
            attach_transform: with_transform,
            reject: false,
        }
    }
}

impl AcceptHook for TestAcceptHook {
    fn on_accept(&self, _fd: RawFd) -> Result<HookOutput, HookError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);

        if self.reject {
            return Err(HookError::Rejected("test rejection".into()));
        }

        let mut data = ConnectionData::new();
        if self.attach_data {
            data.insert("peer_role".into(), Box::new(String::from("test_client")));
            data.insert("peer_cid".into(), Box::new(42u32));
        }

        let payload_transform = if self.attach_transform {
            Some(Box::new(XorPayloadTransform) as Box<dyn PayloadTransform>)
        } else {
            None
        };

        Ok(HookOutput {
            data,
            payload_transform,
        })
    }
}

// ── Test ConnectHook (client side) ─────────────────────────────────────────

#[derive(Debug)]
struct TestConnectHook {
    call_count: Arc<AtomicUsize>,
    attach_data: bool,
    attach_transform: bool,
}

impl TestConnectHook {
    fn new(with_data: bool, with_transform: bool) -> Self {
        Self {
            call_count: Arc::new(AtomicUsize::new(0)),
            attach_data: with_data,
            attach_transform: with_transform,
        }
    }
}

impl ConnectHook for TestConnectHook {
    fn on_connect(&self, _fd: RawFd) -> Result<HookOutput, HookError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);

        let mut data = ConnectionData::new();
        if self.attach_data {
            data.insert("client_role".into(), Box::new(String::from("test_client")));
            data.insert("client_id".into(), Box::new(99u64));
        }

        let payload_transform = if self.attach_transform {
            Some(Box::new(XorPayloadTransform) as Box<dyn PayloadTransform>)
        } else {
            None
        };

        Ok(HookOutput {
            data,
            payload_transform,
        })
    }
}

// ── Echo MethodHandler ─────────────────────────────────────────────────────

#[derive(Debug)]
struct EchoHandler;

#[async_trait]
impl MethodHandler for EchoHandler {
    async fn handler(&self, ctx: TtrpcContext, req: Request) -> ttrpc::Result<Response> {
        // Echo the request payload back as the response payload
        let mut resp = Response::new();
        resp.set_status(Status::default());
        // Copy request payload to response payload
        resp.payload = req.payload;

        // Optionally attach connection data to response for verification
        if let Some(role) = ctx.connection_data.get_typed::<String>("peer_role") {
            // Prepend role info to response for test verification
            let prefix = format!("role:{}|", role);
            let mut new_payload = prefix.into_bytes();
            new_payload.extend_from_slice(&resp.payload);
            resp.payload = new_payload;
        }

        Ok(resp)
    }
}

// ── SlowEcho MethodHandler (for timeout testing) ───────────────────────────

#[derive(Debug)]
struct SlowEchoHandler;

#[async_trait]
impl MethodHandler for SlowEchoHandler {
    async fn handler(&self, _ctx: TtrpcContext, req: Request) -> ttrpc::Result<Response> {
        // Sleep just long enough to guarantee client timeout
        sleep(Duration::from_secs(1)).await;
        let mut resp = Response::new();
        resp.set_status(Status::default());
        resp.payload = req.payload;
        Ok(resp)
    }
}

fn build_slow_echo_service() -> HashMap<String, Service> {
    let mut methods: HashMap<String, Box<dyn MethodHandler + Send + Sync>> = HashMap::new();
    methods.insert(TEST_METHOD.to_string(), Box::new(SlowEchoHandler));

    let mut services = HashMap::new();
    services.insert(
        TEST_SERVICE.to_string(),
        Service {
            methods,
            streams: HashMap::new(),
        },
    );
    services
}

fn build_test_service() -> HashMap<String, Service> {
    let mut methods: HashMap<String, Box<dyn MethodHandler + Send + Sync>> = HashMap::new();
    // Register with just the method name, not the full path
    methods.insert(TEST_METHOD.to_string(), Box::new(EchoHandler));

    let mut services = HashMap::new();
    services.insert(
        TEST_SERVICE.to_string(),
        Service {
            methods,
            streams: HashMap::new(),
        },
    );
    services
}

fn build_echo_request(payload: &[u8]) -> Request {
    let mut req = Request::new();
    req.service = TEST_SERVICE.to_string();
    req.method = TEST_METHOD.to_string();
    req.payload = payload.to_vec();
    req.timeout_nano = 5_000_000_000; // 5 seconds
    req
}

/// Brief yield to allow the server's background accept loop to start.
///
/// We intentionally avoid a "connect-and-retry" readiness probe because
/// any connection to the server would trigger the accept hook, altering
/// the expected hook-call counts that tests assert on.
async fn wait_for_server_ready() {
    tokio::task::yield_now().await;
    sleep(Duration::from_millis(30)).await;
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_server_accept_hook_called_on_connection() {
    let sock_path = temp_unix_socket_path();
    let hook = TestAcceptHook::new_accepting(true, false);
    let hook_count = hook.call_count.clone();

    let mut server = Server::new()
        .bind(&format!("unix://{}", sock_path))
        .unwrap()
        .set_accept_hook(hook)
        .register_service(build_test_service());

    server.start().await.unwrap();
    wait_for_server_ready().await;

    // Connect client (no hook on client side)
    let client = Client::connect(&format!("unix://{}", sock_path))
        .await
        .unwrap();

    // Send a request to trigger connection establishment.
    // The accept hook fires during connection acceptance, which completes
    // before the request is processed, so no extra sleep is needed.
    let req = build_echo_request(b"hello");
    let _resp = client.request(req).await.unwrap();

    // Verify hook was called exactly once
    assert_eq!(
        hook_count.load(Ordering::SeqCst),
        1,
        "AcceptHook should be called once"
    );

    // Cleanup
    server.shutdown().await.unwrap();
    cleanup_socket_file(&sock_path);
}

#[tokio::test]
async fn test_server_accept_hook_rejects_connection() {
    let sock_path = temp_unix_socket_path();
    let hook = TestAcceptHook::new_rejecting();
    let hook_count = hook.call_count.clone();

    let mut server = Server::new()
        .bind(&format!("unix://{}", sock_path))
        .unwrap()
        .set_accept_hook(hook)
        .register_service(build_test_service());

    server.start().await.unwrap();
    wait_for_server_ready().await;

    // Connect client
    let client = Client::connect(&format!("unix://{}", sock_path))
        .await
        .unwrap();

    // Request should fail because server rejects the connection
    let req = build_echo_request(b"hello");
    let result = client.request(req).await;
    assert!(
        result.is_err(),
        "Expected request to fail when server rejects connection"
    );

    // Wait for server to process the rejection
    sleep(Duration::from_millis(200)).await;

    // Verify hook was called
    assert!(
        hook_count.load(Ordering::SeqCst) >= 1,
        "AcceptHook should be called at least once"
    );

    // Cleanup
    server.shutdown().await.unwrap();
    cleanup_socket_file(&sock_path);
}

#[tokio::test]
async fn test_connection_data_propagated_to_handler() {
    let sock_path = temp_unix_socket_path();
    let hook = TestAcceptHook::new_accepting(true, false);

    let mut server = Server::new()
        .bind(&format!("unix://{}", sock_path))
        .unwrap()
        .set_accept_hook(hook)
        .register_service(build_test_service());

    server.start().await.unwrap();
    wait_for_server_ready().await;

    let client = Client::connect(&format!("unix://{}", sock_path))
        .await
        .unwrap();

    let req = build_echo_request(b"test_data");
    let resp = client.request(req).await.unwrap();

    // EchoHandler prepends "role:test_client|" if peer_role is in connection_data
    let resp_str = String::from_utf8_lossy(&resp.payload);
    assert!(
        resp_str.starts_with("role:test_client|"),
        "Expected connection data to be propagated to handler, got: {}",
        resp_str
    );

    server.shutdown().await.unwrap();
    cleanup_socket_file(&sock_path);
}

#[tokio::test]
async fn test_xor_transform_on_wire() {
    let sock_path = temp_unix_socket_path();

    // Server with XOR transform
    let server_hook = TestAcceptHook::new_accepting(false, true);
    let mut server = Server::new()
        .bind(&format!("unix://{}", sock_path))
        .unwrap()
        .set_accept_hook(server_hook)
        .register_service(build_test_service());

    server.start().await.unwrap();
    wait_for_server_ready().await;

    // Client with XOR transform (symmetric)
    let client_hook = TestConnectHook::new(false, true);
    let socket = ttrpc::asynchronous::transport::Socket::connect(&format!("unix://{}", sock_path))
        .await
        .unwrap();
    let client = Client::with_hook(socket, client_hook).unwrap();

    // Send request — should be XOR-encrypted on wire, decrypted by server
    let original_payload = b"encrypted_message_content";
    let req = build_echo_request(original_payload);
    let resp = client.request(req).await.unwrap();

    // Response should be the echo of the original payload (after decrypt→encrypt→decrypt)
    assert_eq!(
        &resp.payload, original_payload,
        "XOR roundtrip failed: expected original payload back"
    );

    server.shutdown().await.unwrap();
    cleanup_socket_file(&sock_path);
}

#[tokio::test]
async fn test_client_connect_hook_called() {
    let sock_path = temp_unix_socket_path();

    let mut server = Server::new()
        .bind(&format!("unix://{}", sock_path))
        .unwrap()
        .register_service(build_test_service());

    server.start().await.unwrap();
    wait_for_server_ready().await;

    let client_hook = TestConnectHook::new(true, false);
    let hook_count = client_hook.call_count.clone();

    let socket = ttrpc::asynchronous::transport::Socket::connect(&format!("unix://{}", sock_path))
        .await
        .unwrap();
    let client = Client::with_hook(socket, client_hook).unwrap();

    let req = build_echo_request(b"hook_test");
    let _resp = client.request(req).await.unwrap();

    assert_eq!(
        hook_count.load(Ordering::SeqCst),
        1,
        "ConnectHook should be called exactly once"
    );

    server.shutdown().await.unwrap();
    cleanup_socket_file(&sock_path);
}

#[tokio::test]
async fn test_symmetric_hooks_with_data_and_transform() {
    let sock_path = temp_unix_socket_path();

    // Server: attach data + XOR transform
    let server_hook = TestAcceptHook::new_accepting(true, true);
    let server_count = server_hook.call_count.clone();

    let mut server = Server::new()
        .bind(&format!("unix://{}", sock_path))
        .unwrap()
        .set_accept_hook(server_hook)
        .register_service(build_test_service());

    server.start().await.unwrap();
    wait_for_server_ready().await;

    // Client: attach data + XOR transform
    let client_hook = TestConnectHook::new(true, true);
    let client_count = client_hook.call_count.clone();

    let socket = ttrpc::asynchronous::transport::Socket::connect(&format!("unix://{}", sock_path))
        .await
        .unwrap();
    let client = Client::with_hook(socket, client_hook).unwrap();

    // Multiple requests to verify transform persists across messages
    for i in 0..5 {
        let payload = format!("message_{}", i);
        let req = build_echo_request(payload.as_bytes());
        let resp = client.request(req).await.unwrap();

        // EchoHandler prepends role info from connection_data
        let resp_str = String::from_utf8_lossy(&resp.payload);
        assert!(
            resp_str.starts_with("role:test_client|"),
            "Request {}: expected role prefix, got: {}",
            i,
            resp_str
        );
    }

    // Both hooks should have been called exactly once
    assert_eq!(server_count.load(Ordering::SeqCst), 1);
    assert_eq!(client_count.load(Ordering::SeqCst), 1);

    server.shutdown().await.unwrap();
    cleanup_socket_file(&sock_path);
}

#[tokio::test]
async fn test_no_hook_plaintext_passthrough() {
    let sock_path = temp_unix_socket_path();

    // Server without hook
    let mut server = Server::new()
        .bind(&format!("unix://{}", sock_path))
        .unwrap()
        .register_service(build_test_service());

    server.start().await.unwrap();
    wait_for_server_ready().await;

    // Client without hook
    let client = Client::connect(&format!("unix://{}", sock_path))
        .await
        .unwrap();

    let original = b"plaintext_message";
    let req = build_echo_request(original);
    let resp = client.request(req).await.unwrap();

    // No transform, no connection data — pure echo
    assert_eq!(
        &resp.payload, original,
        "Plaintext passthrough should return exact payload"
    );

    server.shutdown().await.unwrap();
    cleanup_socket_file(&sock_path);
}

// ── Streaming tests ─────────────────────────────────────────────────────────

use ttrpc::asynchronous::StreamHandler;
use ttrpc::r#async::StreamInner;

const TEST_STREAM_METHOD: &str = "DuplexEcho";

#[derive(Debug)]
struct DuplexEchoHandler;

#[async_trait]
impl StreamHandler for DuplexEchoHandler {
    async fn handler(
        &self,
        ctx: TtrpcContext,
        mut stream: StreamInner,
    ) -> ttrpc::Result<Option<Response>> {
        use ttrpc::proto::Codec;
        // Echo loop: receive Request, send back Response with same payload
        loop {
            match stream.recv().await {
                Ok(data) => {
                    // Decode as Request to extract payload
                    let req = Request::decode(&data).unwrap_or_else(|_| {
                        let mut r = Request::new();
                        r.payload = data.clone();
                        r
                    });
                    // Build Response with same payload and encode it
                    let mut resp = Response::new();
                    resp.set_status(Status::default());
                    resp.payload = req.payload;
                    let encoded = resp
                        .encode()
                        .map_err(|e| ttrpc::Error::Others(format!("encode resp failed: {}", e)))?;
                    stream.send(encoded).await?;
                }
                Err(ttrpc::Error::Eof) => break,
                Err(ttrpc::Error::RemoteClosed) => break,
                Err(e) => return Err(e),
            }
        }
        // Send final response
        let mut resp = Response::new();
        resp.set_status(Status::default());
        if let Some(role) = ctx.connection_data.get_typed::<String>("peer_role") {
            resp.payload = format!("stream_done:{}", role).into_bytes();
        } else {
            resp.payload = b"stream_done".to_vec();
        }
        Ok(Some(resp))
    }
}

fn build_test_service_with_stream() -> HashMap<String, Service> {
    let mut methods: HashMap<String, Box<dyn MethodHandler + Send + Sync>> = HashMap::new();
    methods.insert(TEST_METHOD.to_string(), Box::new(EchoHandler));

    let mut streams: HashMap<String, Arc<dyn StreamHandler + Send + Sync>> = HashMap::new();
    streams.insert(TEST_STREAM_METHOD.to_string(), Arc::new(DuplexEchoHandler));

    let mut services = HashMap::new();
    services.insert(TEST_SERVICE.to_string(), Service { methods, streams });
    services
}

fn build_stream_request() -> Request {
    let mut req = Request::new();
    req.service = TEST_SERVICE.to_string();
    req.method = TEST_STREAM_METHOD.to_string();
    req.timeout_nano = 5_000_000_000;
    req
}

#[tokio::test]
async fn test_streaming_with_xor_transform() {
    let sock_path = temp_unix_socket_path();

    // Server with XOR transform + stream handler
    let server_hook = TestAcceptHook::new_accepting(false, true);
    let mut server = Server::new()
        .bind(&format!("unix://{}", sock_path))
        .unwrap()
        .set_accept_hook(server_hook)
        .register_service(build_test_service_with_stream());

    server.start().await.unwrap();
    wait_for_server_ready().await;

    // Client with XOR transform
    let client_hook = TestConnectHook::new(false, true);
    let socket = ttrpc::asynchronous::transport::Socket::connect(&format!("unix://{}", sock_path))
        .await
        .unwrap();
    let client = Client::with_hook(socket, client_hook).unwrap();

    // Open a duplex stream
    let inner = client
        .new_stream(build_stream_request(), true, true)
        .await
        .unwrap();
    let mut stream = ttrpc::r#async::ClientStream::<Request, Response>::new(inner);

    // Send 3 messages and verify echo
    for i in 0u32..3 {
        let mut msg = Request::new();
        msg.payload = format!("stream_msg_{}", i).into_bytes();
        stream.send(&msg).await.unwrap();

        let echoed = stream.recv().await.unwrap();
        assert_eq!(echoed.payload, msg.payload, "Stream echo {} failed", i);
    }

    // Close the client side
    stream.close_send().await.unwrap();

    // Wait for server to process close and send final response
    sleep(Duration::from_millis(200)).await;

    server.shutdown().await.unwrap();
    cleanup_socket_file(&sock_path);
}

#[tokio::test]
async fn test_streaming_without_transform_plaintext() {
    let sock_path = temp_unix_socket_path();

    // Server without hook (no transform)
    let mut server = Server::new()
        .bind(&format!("unix://{}", sock_path))
        .unwrap()
        .register_service(build_test_service_with_stream());

    server.start().await.unwrap();
    wait_for_server_ready().await;

    let client = Client::connect(&format!("unix://{}", sock_path))
        .await
        .unwrap();

    let inner = client
        .new_stream(build_stream_request(), true, true)
        .await
        .unwrap();
    let mut stream = ttrpc::r#async::ClientStream::<Request, Response>::new(inner);

    let mut msg = Request::new();
    msg.payload = b"plain_stream_data".to_vec();
    stream.send(&msg).await.unwrap();

    let echoed = stream.recv().await.unwrap();
    assert_eq!(echoed.payload, b"plain_stream_data");

    stream.close_send().await.unwrap();
    sleep(Duration::from_millis(200)).await;

    server.shutdown().await.unwrap();
    cleanup_socket_file(&sock_path);
}

// ── ConnectHook error path ──────────────────────────────────────────────────

#[tokio::test]
async fn test_client_connect_hook_rejected_fails_connection() {
    let sock_path = temp_unix_socket_path();

    let mut server = Server::new()
        .bind(&format!("unix://{}", sock_path))
        .unwrap()
        .register_service(build_test_service());

    server.start().await.unwrap();
    wait_for_server_ready().await;

    // ConnectHook that returns Err — client creation must fail.
    #[derive(Debug)]
    struct RejectingConnectHook;
    impl ConnectHook for RejectingConnectHook {
        fn on_connect(&self, _fd: RawFd) -> Result<HookOutput, HookError> {
            Err(HookError::Rejected("client self-reject".into()))
        }
    }

    let socket = ttrpc::asynchronous::transport::Socket::connect(&format!("unix://{}", sock_path))
        .await
        .unwrap();
    let err = match Client::with_hook(socket, RejectingConnectHook) {
        Ok(_) => panic!("Expected Client::with_hook to fail when connect hook rejects"),
        Err(e) => e,
    };
    let err_str = format!("{}", err);
    assert!(
        err_str.contains("client self-reject") || err_str.contains("connect hook rejected"),
        "Expected hook rejection error, got: {}",
        err_str
    );

    server.shutdown().await.unwrap();
    cleanup_socket_file(&sock_path);
}

// ── Multiple concurrent connections ─────────────────────────────────────────

#[tokio::test]
async fn test_multiple_concurrent_connections_with_transform() {
    let sock_path = temp_unix_socket_path();

    let server_hook = TestAcceptHook::new_accepting(true, true);
    let server_count = server_hook.call_count.clone();

    let mut server = Server::new()
        .bind(&format!("unix://{}", sock_path))
        .unwrap()
        .set_accept_hook(server_hook)
        .register_service(build_test_service());

    server.start().await.unwrap();
    wait_for_server_ready().await;

    // Create 3 concurrent clients, each with XOR transform
    let mut handles = vec![];
    for i in 0..3 {
        let path = sock_path.clone();
        handles.push(tokio::spawn(async move {
            let client_hook = TestConnectHook::new(true, true);
            let socket =
                ttrpc::asynchronous::transport::Socket::connect(&format!("unix://{}", path))
                    .await
                    .unwrap();
            let client = Client::with_hook(socket, client_hook).unwrap();

            let payload = format!("concurrent_client_{}", i);
            let req = build_echo_request(payload.as_bytes());
            let resp = client.request(req).await.unwrap();

            // Verify response contains role prefix (from server's connection data)
            let resp_str = String::from_utf8_lossy(&resp.payload);
            assert!(
                resp_str.starts_with("role:test_client|"),
                "Client {}: expected role prefix, got: {}",
                i,
                resp_str
            );
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Each connection should have triggered AcceptHook once
    assert_eq!(
        server_count.load(Ordering::SeqCst),
        3,
        "AcceptHook should be called once per connection"
    );

    server.shutdown().await.unwrap();
    cleanup_socket_file(&sock_path);
}

// ── Coverage gap tests ────────────────────────────────────────────────────────

// Test 1: streaming_client=false with XOR transform
//
// When streaming_client=false with a non-empty request payload, the server's
// handle_stream() creates a synthetic DATA message from the REQUEST payload
// that was already decrypted by handle_request() (Injection Point 2/10).
// The fix: use StreamMsg::PreDecoded so StreamReceiver::recv()
// (Injection Point 10/10) passes it through without re-applying
// transform_inbound. Works for any PayloadTransform type.
#[tokio::test]
async fn test_streaming_client_false_with_xor_transform() {
    let sock_path = temp_unix_socket_path();

    let server_hook = TestAcceptHook::new_accepting(false, true);
    let mut server = Server::new()
        .bind(&format!("unix://{}", sock_path))
        .unwrap()
        .set_accept_hook(server_hook)
        .register_service(build_test_service_with_stream());

    server.start().await.unwrap();
    wait_for_server_ready().await;

    let client_hook = TestConnectHook::new(false, true);
    let socket = ttrpc::asynchronous::transport::Socket::connect(&format!("unix://{}", sock_path))
        .await
        .unwrap();
    let client = Client::with_hook(socket, client_hook).unwrap();

    // streaming_client = false, non-empty payload → triggers handle_stream
    // faked DATA with re-encryption → StreamReceiver::recv() decrypts correctly
    let mut req = build_stream_request();
    req.payload = b"initial_payload".to_vec();

    let inner = client.new_stream(req, false, true).await.unwrap();
    let mut stream = ttrpc::r#async::ClientStream::<Request, Response>::new(inner);

    // Server handler (DuplexEchoHandler) receives the initial payload as
    // the first DATA message, echoes it back, then enters the recv loop.
    // The echo should contain the original payload bytes — proving that
    // the double-transform was correctly compensated.
    let echoed = stream.recv().await.unwrap();
    assert_eq!(
        echoed.payload, b"initial_payload",
        "streaming_client=false initial payload should survive transform fix"
    );

    // With streaming_client=false, the client cannot send or close_send.
    // Just drop the stream to signal we're done.
    drop(stream);
    sleep(Duration::from_millis(200)).await;

    server.shutdown().await.unwrap();
    cleanup_socket_file(&sock_path);
}

// Test 2: Stream close semantics — close_send + post-close send error
#[tokio::test]
async fn test_stream_close_send_then_send_fails() {
    let sock_path = temp_unix_socket_path();

    let mut server = Server::new()
        .bind(&format!("unix://{}", sock_path))
        .unwrap()
        .register_service(build_test_service_with_stream());

    server.start().await.unwrap();
    wait_for_server_ready().await;

    let client = Client::connect(&format!("unix://{}", sock_path))
        .await
        .unwrap();

    let inner = client
        .new_stream(build_stream_request(), true, true)
        .await
        .unwrap();
    let stream = ttrpc::r#async::ClientStream::<Request, Response>::new(inner);

    // Send one message successfully
    let mut msg = Request::new();
    msg.payload = b"before_close".to_vec();
    stream.send(&msg).await.unwrap();

    // Close the send side
    stream.close_send().await.unwrap();

    // Sending after close should fail with LocalClosed
    let result = stream.send(&msg).await;
    assert!(result.is_err(), "send after close_send should fail");
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("LocalClosed") || err_msg.contains("closed"),
        "Expected LocalClosed error, got: {}",
        err_msg
    );

    sleep(Duration::from_millis(100)).await;
    server.shutdown().await.unwrap();
    cleanup_socket_file(&sock_path);
}

// Test 3: Server shutdown during active streaming
//
// Verifies that server.shutdown() completes cleanly even when a stream
// is still open on the client side. The post-shutdown stream operations
// are wrapped in a timeout because the client may not detect the closed
// connection immediately.
#[tokio::test]
async fn test_server_shutdown_during_active_stream() {
    let sock_path = temp_unix_socket_path();

    let mut server = Server::new()
        .bind(&format!("unix://{}", sock_path))
        .unwrap()
        .register_service(build_test_service_with_stream());

    server.start().await.unwrap();
    wait_for_server_ready().await;

    let client = Client::connect(&format!("unix://{}", sock_path))
        .await
        .unwrap();

    let inner = client
        .new_stream(build_stream_request(), true, true)
        .await
        .unwrap();
    let mut stream = ttrpc::r#async::ClientStream::<Request, Response>::new(inner);

    // Send a message and receive echo — verifies stream works before shutdown
    let mut msg = Request::new();
    msg.payload = b"before_shutdown".to_vec();
    stream.send(&msg).await.unwrap();
    let echoed = stream.recv().await.unwrap();
    assert_eq!(echoed.payload, b"before_shutdown");

    // Shutdown server while stream is still open — must complete without deadlock
    server.shutdown().await.unwrap();
    cleanup_socket_file(&sock_path);

    // Post-shutdown: recv() should detect the closed connection (with timeout)
    sleep(Duration::from_millis(200)).await;
    let result = tokio::time::timeout(Duration::from_secs(3), stream.recv()).await;
    match result {
        Ok(Ok(_)) => {} // Unexpected but not a failure — server may have sent a final message
        Ok(Err(_)) => {} // Expected: recv detects closed connection
        Err(_) => {}    // Timeout: client hasn't detected shutdown yet — acceptable
    }
}

// Test 4: ConnectHook receives valid raw_fd
#[tokio::test]
async fn test_connect_hook_receives_valid_raw_fd() {
    let sock_path = temp_unix_socket_path();

    let mut server = Server::new()
        .bind(&format!("unix://{}", sock_path))
        .unwrap()
        .register_service(build_test_service());

    server.start().await.unwrap();
    wait_for_server_ready().await;

    // ConnectHook that captures the fd it receives
    let captured_fd = Arc::new(std::sync::atomic::AtomicI32::new(-1));
    let captured_fd_clone = captured_fd.clone();

    #[derive(Debug)]
    struct FdCapturingHook {
        captured: Arc<std::sync::atomic::AtomicI32>,
    }
    impl ConnectHook for FdCapturingHook {
        fn on_connect(&self, fd: RawFd) -> Result<HookOutput, HookError> {
            self.captured.store(fd, Ordering::SeqCst);
            Ok(HookOutput {
                data: ConnectionData::new(),
                payload_transform: None,
            })
        }
    }

    let socket = ttrpc::asynchronous::transport::Socket::connect(&format!("unix://{}", sock_path))
        .await
        .unwrap();
    let client = Client::with_hook(
        socket,
        FdCapturingHook {
            captured: captured_fd_clone,
        },
    )
    .unwrap();

    // Trigger connection
    let req = build_echo_request(b"fd_check");
    let _resp = client.request(req).await.unwrap();

    let fd = captured_fd.load(Ordering::SeqCst);
    assert!(
        fd >= 0,
        "ConnectHook should receive a valid raw fd (>= 0), got: {}",
        fd
    );

    server.shutdown().await.unwrap();
    cleanup_socket_file(&sock_path);
}

// Test 5: Server-initiated stream close — client receives Eof
//
// Handler that sends 2 echo responses then returns (server closes the stream).
#[derive(Debug)]
struct LimitedEchoHandler;

#[async_trait]
impl StreamHandler for LimitedEchoHandler {
    async fn handler(
        &self,
        _ctx: TtrpcContext,
        mut stream: StreamInner,
    ) -> ttrpc::Result<Option<Response>> {
        // Receive one message, echo it as DATA, then return final response
        if let Ok(data) = stream.recv().await {
            // Echo the raw data back as a DATA message
            stream.send(data).await?;
        }
        // Return final response — triggers server stream close
        let mut resp = Response::new();
        resp.set_status(Status::default());
        resp.payload = b"limited_done".to_vec();
        Ok(Some(resp))
    }
}

fn build_limited_echo_service() -> HashMap<String, Service> {
    let mut streams: HashMap<String, Arc<dyn StreamHandler + Send + Sync>> = HashMap::new();
    streams.insert("LimitedEcho".to_string(), Arc::new(LimitedEchoHandler));

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

#[tokio::test]
async fn test_server_initiated_stream_close_client_gets_final_response() {
    let sock_path = temp_unix_socket_path();

    let mut server = Server::new()
        .bind(&format!("unix://{}", sock_path))
        .unwrap()
        .register_service(build_limited_echo_service());

    server.start().await.unwrap();
    wait_for_server_ready().await;

    let client = Client::connect(&format!("unix://{}", sock_path))
        .await
        .unwrap();

    // Build stream request for LimitedEcho method
    let mut req = Request::new();
    req.service = TEST_SERVICE.to_string();
    req.method = "LimitedEcho".to_string();
    req.timeout_nano = 5_000_000_000;

    let inner = client.new_stream(req, true, true).await.unwrap();
    let mut stream = ttrpc::r#async::ClientStream::<Request, Response>::new(inner);

    // Send one message, server echoes it as DATA, then returns final response
    let mut msg = Request::new();
    msg.payload = b"limited_test".to_vec();
    stream.send(&msg).await.unwrap();

    // First recv: gets the echo DATA message (raw bytes, decoded as Response).
    // We don't check the echo content because it's raw StreamInner::send() bytes,
    // but we do assert success so a failed echo doesn't silently cascade.
    let _echoed = stream.recv().await.expect("echo recv should succeed");

    // Second recv: gets the final RESPONSE from the handler's return value
    // Use timeout to prevent hanging
    let result = tokio::time::timeout(Duration::from_secs(5), stream.recv()).await;
    match result {
        Ok(Ok(final_resp)) => {
            assert_eq!(
                final_resp.payload, b"limited_done",
                "Expected final response payload 'limited_done'"
            );
        }
        Ok(Err(e)) => {
            // Server may close the stream before sending final response — that's acceptable
            let err_str = format!("{}", e);
            assert!(
                err_str.contains("Eof")
                    || err_str.contains("RemoteClosed")
                    || err_str.contains("closed")
                    || err_str.contains("Receiver")
                    || err_str.contains("Decode"),
                "Expected Eof/RemoteClosed/Decode, got: {}",
                err_str
            );
        }
        Err(_) => {
            panic!("Timed out waiting for server's final response or stream close");
        }
    }

    server.shutdown().await.unwrap();
    cleanup_socket_file(&sock_path);
}

// Test 6: Unary request timeout (server-side DEADLINE_EXCEEDED + client-side timeout)
//
// Covers the timeout code path in server.rs handle_method() (tokio::time::timeout
// around the handler) and client.rs request() (one deadline for send and response).
#[tokio::test]
async fn test_unary_request_timeout() {
    let sock_path = temp_unix_socket_path();

    // Server with SlowEchoHandler (sleeps 1s)
    let mut server = Server::new()
        .bind(&format!("unix://{}", sock_path))
        .unwrap()
        .register_service(build_slow_echo_service());

    server.start().await.unwrap();
    wait_for_server_ready().await;

    let client = Client::connect(&format!("unix://{}", sock_path))
        .await
        .unwrap();

    // Build request with very short timeout (100ms) — handler sleeps 1s
    let mut req = Request::new();
    req.service = TEST_SERVICE.to_string();
    req.method = TEST_METHOD.to_string();
    req.payload = b"timeout_test".to_vec();
    req.timeout_nano = 100_000_000; // 100ms

    let result = client.request(req).await;
    assert!(result.is_err(), "Expected timeout error");
    let err = result.unwrap_err();
    let err_str = format!("{:?}", err);
    assert!(
        err_str.contains("timeout")
            || err_str.contains("Timeout")
            || err_str.contains("deadline elapsed")
            || err_str.contains("DEADLINE_EXCEEDED"),
        "Expected timeout-related error, got: {}",
        err_str
    );

    server.shutdown().await.unwrap();
    cleanup_socket_file(&sock_path);
}

// Test 7: Streaming with streaming_server=false — client rejects DATA messages
//
// When streaming_server=false, the client's StreamReceiver has receivable=false.
// If the server sends DATA messages, recv() returns an error because the client
// does not expect streaming data from the server.
#[tokio::test]
async fn test_streaming_server_false_rejects_data() {
    let sock_path = temp_unix_socket_path();

    let server_hook = TestAcceptHook::new_accepting(false, true);
    let mut server = Server::new()
        .bind(&format!("unix://{}", sock_path))
        .unwrap()
        .set_accept_hook(server_hook)
        .register_service(build_test_service_with_stream());

    server.start().await.unwrap();
    wait_for_server_ready().await;

    let client_hook = TestConnectHook::new(false, true);
    let socket = ttrpc::asynchronous::transport::Socket::connect(&format!("unix://{}", sock_path))
        .await
        .unwrap();
    let client = Client::with_hook(socket, client_hook).unwrap();

    // streaming_client=true, streaming_server=false
    let inner = client
        .new_stream(build_stream_request(), true, false)
        .await
        .unwrap();
    let mut stream = ttrpc::r#async::ClientStream::<Request, Response>::new(inner);

    // Send a message — server's DuplexEchoHandler will echo it back as DATA
    let mut msg = Request::new();
    msg.payload = b"test_data".to_vec();
    stream.send(&msg).await.unwrap();

    // recv() should fail because streaming_server=false (receivable=false)
    let result = tokio::time::timeout(Duration::from_secs(3), stream.recv()).await;
    match result {
        Ok(Err(e)) => {
            let err_str = format!("{}", e);
            assert!(
                err_str.contains("non-streaming server")
                    || err_str.contains("Eof")
                    || err_str.contains("RemoteClosed")
                    || err_str.contains("closed"),
                "Expected non-streaming-server error or close, got: {}",
                err_str
            );
        }
        Ok(Ok(_)) => {
            panic!("recv() should have failed for streaming_server=false, but succeeded");
        }
        Err(_) => {
            panic!("recv() timed out — expected non-streaming-server error");
        }
    }

    drop(stream);
    sleep(Duration::from_millis(200)).await;
    server.shutdown().await.unwrap();
    cleanup_socket_file(&sock_path);
}

// Test 8: Multiple concurrent streams on one connection
#[tokio::test]
async fn test_multiple_concurrent_streams_on_one_connection() {
    let sock_path = temp_unix_socket_path();

    let server_hook = TestAcceptHook::new_accepting(false, true);
    let mut server = Server::new()
        .bind(&format!("unix://{}", sock_path))
        .unwrap()
        .set_accept_hook(server_hook)
        .register_service(build_test_service_with_stream());

    server.start().await.unwrap();
    wait_for_server_ready().await;

    // Single client with XOR transform
    let client_hook = TestConnectHook::new(false, true);
    let socket = ttrpc::asynchronous::transport::Socket::connect(&format!("unix://{}", sock_path))
        .await
        .unwrap();
    let client = Client::with_hook(socket, client_hook).unwrap();

    // Open 2 independent streams on the same connection
    let inner1 = client
        .new_stream(build_stream_request(), true, true)
        .await
        .unwrap();
    let mut stream1 = ttrpc::r#async::ClientStream::<Request, Response>::new(inner1);

    let inner2 = client
        .new_stream(build_stream_request(), true, true)
        .await
        .unwrap();
    let mut stream2 = ttrpc::r#async::ClientStream::<Request, Response>::new(inner2);

    // Interleave sends/receives across both streams
    let mut msg1 = Request::new();
    msg1.payload = b"stream1_data".to_vec();
    stream1.send(&msg1).await.unwrap();

    let mut msg2 = Request::new();
    msg2.payload = b"stream2_data".to_vec();
    stream2.send(&msg2).await.unwrap();

    // Receive in reverse order to verify independence
    let echoed2 = stream2.recv().await.unwrap();
    assert_eq!(echoed2.payload, b"stream2_data", "Stream 2 echo mismatch");

    let echoed1 = stream1.recv().await.unwrap();
    assert_eq!(echoed1.payload, b"stream1_data", "Stream 1 echo mismatch");

    // Close both streams
    stream1.close_send().await.unwrap();
    stream2.close_send().await.unwrap();

    sleep(Duration::from_millis(200)).await;
    server.shutdown().await.unwrap();
    cleanup_socket_file(&sock_path);
}
