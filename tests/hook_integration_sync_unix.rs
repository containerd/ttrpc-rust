// Copyright 2026 Alibaba Cloud. All rights reserved.
//
// SPDX-License-Identifier: Apache-2.0
//
#![cfg(all(feature = "sync", feature = "security_extension"))]
//! Integration tests for sync AcceptHook, ConnectHook, and PayloadTransform.

mod common;

use std::collections::HashMap;
use std::os::unix::io::RawFd;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ttrpc::proto::{Request, Response};
use ttrpc::security_extension::{
    AcceptHook, ConnectHook, ConnectionData, ConnectionDataExt, HookError, HookOutput,
};
use ttrpc::sync::{Client, MethodHandler, Server, TtrpcContext};
use ttrpc::{get_status, Code};

use common::{cleanup_socket_file, temp_unix_socket_path, XorPayloadTransform};

// ── Test constants ──────────────────────────────────────────────────────────

const TEST_SERVICE: &str = "test.SyncTestService";
const TEST_METHOD: &str = "Echo";

// ── Test hooks ──────────────────────────────────────────────────────────────

#[derive(Debug)]
struct CountingAcceptHook {
    call_count: Arc<AtomicUsize>,
    reject: bool,
}

impl AcceptHook for CountingAcceptHook {
    fn on_accept(&self, _fd: RawFd) -> Result<HookOutput, HookError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        if self.reject {
            return Err(HookError::Rejected("rejected by hook".into()));
        }
        let mut data = ConnectionData::new();
        data.insert("peer_role".into(), Box::new(String::from("sync-client")));
        Ok(HookOutput {
            data,
            payload_transform: Some(Box::new(XorPayloadTransform)),
        })
    }
}

#[derive(Debug)]
struct CountingConnectHook {
    call_count: Arc<AtomicUsize>,
}

impl ConnectHook for CountingConnectHook {
    fn on_connect(&self, _fd: RawFd) -> Result<HookOutput, HookError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(HookOutput {
            data: ConnectionData::new(),
            payload_transform: Some(Box::new(XorPayloadTransform)),
        })
    }
}

#[derive(Debug)]
struct RejectingConnectHook;

impl ConnectHook for RejectingConnectHook {
    fn on_connect(&self, _fd: RawFd) -> Result<HookOutput, HookError> {
        Err(HookError::Rejected("client rejected".into()))
    }
}

// ── Sync MethodHandler ──────────────────────────────────────────────────────

#[derive(Debug)]
struct SyncEchoHandler;

impl MethodHandler for SyncEchoHandler {
    fn handler(&self, ctx: TtrpcContext, req: Request) -> ttrpc::Result<()> {
        let mut resp = Response::new();
        resp.set_status(get_status(Code::OK, "".to_string()));
        // Echo the request payload back, optionally prefixed with connection data
        let mut payload = Vec::new();
        if let Some(role) = ctx.conn_ctx.data.get_typed::<String>("peer_role") {
            payload.extend_from_slice(format!("role:{}|", role).as_bytes());
        }
        payload.extend_from_slice(&req.payload);
        resp.payload = payload;
        ctx.respond(ctx.mh.stream_id, resp)?;
        Ok(())
    }
}

fn build_sync_echo_service() -> HashMap<String, Box<dyn MethodHandler + Send + Sync>> {
    let mut methods: HashMap<String, Box<dyn MethodHandler + Send + Sync>> = HashMap::new();
    let path = format!("/{}/{}", TEST_SERVICE, TEST_METHOD);
    methods.insert(path, Box::new(SyncEchoHandler));
    methods
}

// ── Helper: wait for server to be ready ─────────────────────────────────────

fn wait_for_server_ready() {
    thread::sleep(Duration::from_millis(50));
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn test_sync_no_hook_plaintext_passthrough() {
    let sock_path = temp_unix_socket_path();
    let path = format!("unix://{}", sock_path);

    let mut server = Server::new()
        .bind(&path)
        .unwrap()
        .register_service(build_sync_echo_service());
    server.start().unwrap();
    wait_for_server_ready();

    let client = Client::connect(&path).unwrap();

    let mut req = Request::new();
    req.set_service(TEST_SERVICE.to_string());
    req.set_method(TEST_METHOD.to_string());
    req.payload = b"hello".to_vec();

    let resp = client.request(req).unwrap();
    assert_eq!(resp.payload, b"hello");

    server.shutdown();
    cleanup_socket_file(&sock_path);
}

#[test]
fn test_sync_accept_hook_called_on_connection() {
    let sock_path = temp_unix_socket_path();
    let path = format!("unix://{}", sock_path);

    let hook_count = Arc::new(AtomicUsize::new(0));
    let hook = CountingAcceptHook {
        call_count: hook_count.clone(),
        reject: false,
    };

    let mut server = Server::new()
        .bind(&path)
        .unwrap()
        .register_service(build_sync_echo_service())
        .set_accept_hook(hook);
    server.start().unwrap();
    wait_for_server_ready();

    // Connect (triggers accept hook)
    let hook_count_client = Arc::new(AtomicUsize::new(0));
    let connect_hook = CountingConnectHook {
        call_count: hook_count_client.clone(),
    };
    let fd = create_connected_fd(&sock_path);
    let client = Client::with_hook(fd, connect_hook).unwrap();

    // Send a request to ensure the server processes the accept
    let mut req = Request::new();
    req.set_service(TEST_SERVICE.to_string());
    req.set_method(TEST_METHOD.to_string());
    req.payload = b"test".to_vec();

    let resp = client.request(req).unwrap();

    // Hook should have been called once on accept
    assert_eq!(hook_count.load(Ordering::SeqCst), 1);
    assert_eq!(hook_count_client.load(Ordering::SeqCst), 1);

    // Response should have connection data prefix
    let resp_str = String::from_utf8_lossy(&resp.payload);
    assert!(resp_str.starts_with("role:sync-client|"));

    server.shutdown();
    cleanup_socket_file(&sock_path);
}

#[test]
fn test_sync_accept_hook_rejects_connection() {
    let sock_path = temp_unix_socket_path();
    let path = format!("unix://{}", sock_path);

    let hook_count = Arc::new(AtomicUsize::new(0));
    let hook = CountingAcceptHook {
        call_count: hook_count.clone(),
        reject: true,
    };

    let mut server = Server::new()
        .bind(&path)
        .unwrap()
        .register_service(build_sync_echo_service())
        .set_accept_hook(hook);
    server.start().unwrap();
    wait_for_server_ready();

    // Try to connect — server will reject via hook
    let fd = create_connected_fd(&sock_path);
    let client = Client::new(fd).unwrap();

    // Send a request to trigger the accept loop processing the connection
    let mut req = Request::new();
    req.set_service(TEST_SERVICE.to_string());
    req.set_method(TEST_METHOD.to_string());
    req.payload = b"fail".to_vec();
    req.timeout_nano = 2_000_000_000; // 2 second timeout

    let result = client.request(req);

    // Hook was called
    assert_eq!(hook_count.load(Ordering::SeqCst), 1);

    // Request should fail (server closed the connection after hook rejection)
    assert!(result.is_err());

    server.shutdown();
    cleanup_socket_file(&sock_path);
}

#[test]
fn test_sync_connect_hook_rejected_fails() {
    let sock_path = temp_unix_socket_path();
    let path = format!("unix://{}", sock_path);

    let mut server = Server::new()
        .bind(&path)
        .unwrap()
        .register_service(build_sync_echo_service());
    server.start().unwrap();
    wait_for_server_ready();

    let fd = create_connected_fd(&sock_path);
    let result = Client::with_hook(fd, RejectingConnectHook);
    assert!(result.is_err());
    if let Err(e) = result {
        let err_msg = format!("{}", e);
        assert!(err_msg.contains("rejected"));
    }

    server.shutdown();
    cleanup_socket_file(&sock_path);
}

#[test]
fn test_sync_xor_transform_roundtrip() {
    let sock_path = temp_unix_socket_path();
    let path = format!("unix://{}", sock_path);

    let hook_count = Arc::new(AtomicUsize::new(0));
    let hook = CountingAcceptHook {
        call_count: hook_count.clone(),
        reject: false,
    };

    let mut server = Server::new()
        .bind(&path)
        .unwrap()
        .register_service(build_sync_echo_service())
        .set_accept_hook(hook);
    server.start().unwrap();
    wait_for_server_ready();

    let client_hook_count = Arc::new(AtomicUsize::new(0));
    let connect_hook = CountingConnectHook {
        call_count: client_hook_count.clone(),
    };
    let fd = create_connected_fd(&sock_path);
    let client = Client::with_hook(fd, connect_hook).unwrap();

    // Send various payload sizes
    for payload in &[
        b"a".to_vec(),
        b"ab".to_vec(),
        b"abc".to_vec(),
        vec![0u8; 1024],
    ] {
        let mut req = Request::new();
        req.set_service(TEST_SERVICE.to_string());
        req.set_method(TEST_METHOD.to_string());
        req.payload = payload.clone();

        let resp = client.request(req).unwrap();
        // Response has "role:sync-client|" prefix + original payload
        let prefix = b"role:sync-client|";
        assert!(resp.payload.starts_with(prefix));
        let original = &resp.payload[prefix.len()..];
        assert_eq!(original, payload.as_slice());
    }

    server.shutdown();
    cleanup_socket_file(&sock_path);
}

#[test]
fn test_sync_connection_data_propagated() {
    let sock_path = temp_unix_socket_path();
    let path = format!("unix://{}", sock_path);

    let hook = CountingAcceptHook {
        call_count: Arc::new(AtomicUsize::new(0)),
        reject: false,
    };

    let mut server = Server::new()
        .bind(&path)
        .unwrap()
        .register_service(build_sync_echo_service())
        .set_accept_hook(hook);
    server.start().unwrap();
    wait_for_server_ready();

    let connect_hook = CountingConnectHook {
        call_count: Arc::new(AtomicUsize::new(0)),
    };
    let fd = create_connected_fd(&sock_path);
    let client = Client::with_hook(fd, connect_hook).unwrap();

    let mut req = Request::new();
    req.set_service(TEST_SERVICE.to_string());
    req.set_method(TEST_METHOD.to_string());
    req.payload = b"data".to_vec();

    let resp = client.request(req).unwrap();
    // Verify the connection data was accessible in the handler
    let resp_str = String::from_utf8_lossy(&resp.payload);
    assert!(resp_str.starts_with("role:sync-client|"));

    server.shutdown();
    cleanup_socket_file(&sock_path);
}

// ── Utility: create a connected fd ──────────────────────────────────────────

fn create_connected_fd(path: &str) -> RawFd {
    use std::os::unix::io::IntoRawFd;
    use std::os::unix::net::UnixStream;

    let stream = UnixStream::connect(path).expect("connect");
    stream.into_raw_fd()
}
