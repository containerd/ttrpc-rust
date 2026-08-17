// Copyright 2026 Alibaba Cloud. All rights reserved.
//
// SPDX-License-Identifier: Apache-2.0
//

//! Connection Extension Framework.
//!
//! # Design Principle
//!
//! **ttrpc provides mechanism, not policy.**
//!
//! ttrpc is a lightweight RPC transport framework. It provides generic extension
//! points -- hooks and opaque data attachment -- for applications to implement
//! connection-level security, authorization, or any other cross-cutting concern.
//! ttrpc itself contains **zero** domain logic: no roles, no authorization
//! policies, no encryption algorithms, no identity types.
//!
//! All policy decisions and concrete implementations reside exclusively in
//! the application layer (e.g., kata-agent).
//!
//! # Extension Points
//!
//! ```text
//!            +------------------------------+   +------------------------------+
//!            | ttrpc provides               |   | application decides          |
//!            |                              |   |                              |
//! accept()   | AcceptHook                   |   | CID check, mTLS, token auth, |
//!            |  -> accept / reject          |   | or nothing -- app's choice   |
//!            |  -> attach metadata          |   |                              |
//!            |                              |   |                              |
//! request    | PayloadTransform             |   | AES-GCM, ChaCha20, zstd      |
//! (in)       | (optional, per-conn)         |   | compression, or no-op        |
//!            |                              |   |                              |
//! dispatch   | TtrpcContext                 |   |                              |
//!            | .connection_data             |   | Handler reads metadata,      |
//!            |                              |   | app enforces its own authz   |
//!            |                              |   |                              |
//! response   | PayloadTransform             |   | AES-GCM, ChaCha20, zstd      |
//! (out)      | (optional, per-conn)         |   | compression, or no-op        |
//!            +------------------------------+   +------------------------------+
//! ```
//!
//! # What ttrpc Knows vs. Doesn't Know
//!
//! | ttrpc knows                        | ttrpc does NOT know             |
//! |------------------------------------|---------------------------------|
//! | A hook exists and can reject conns | CID, roles, identities          |
//! | Connections can carry opaque data  | What the data means             |
//! | Payloads can be transformed in/out | Encryption, compression, codecs |
//! | Handlers can read connection data  | Authorization policies          |
//! | Stream IDs, message framing        | Business logic of any kind      |
//!
//! # Backward Compatibility
//!
//! | Scenario                   | Behavior                                          |
//! |----------------------------|---------------------------------------------------|
//! | No hook set                | All connections accepted, empty data, no xform    |
//! | Hook returns `Ok`          | Connection accepted with app-defined metadata     |
//! | Hook returns `Err`         | Connection rejected and closed immediately        |
//! | `payload_transform = None` | Plaintext pass-through (no overhead)              |
//! | Existing handlers          | `connection_data` is empty HashMap -- no breakage |
//!
//! # 10 Injection Points
//!
//! ttrpc has 10 internal injection points where the extension framework hooks
//! into the message processing pipeline. They ensure **no payload bypasses
//! the transform regardless of message type or direction**.
//!
//! | #  | Side   | Location                 | Direction | Message Type        |
//! |----|--------|--------------------------|-----------|---------------------|
//! | 1  | server | `do_start()` accept      | -         | New connection      |
//! | 2  | server | `handle_request()`       | inbound   | Unary REQUEST       |
//! | 3  | server | `handle_msg()` DATA      | route     | Streaming DATA (1)  |
//! | 4  | server | `handle_msg()` response  | outbound  | Unary response      |
//! | 5  | client | `new_inner()` connect    | -         | New connection      |
//! | 6  | client | `request()` send         | outbound  | Unary REQUEST       |
//! | 7  | client | `handle_msg()` recv      | inbound   | Unary response      |
//! | 8  | client | `new_stream()` send      | outbound  | Stream-init REQUEST |
//! | 9  | both   | `StreamSender::send()`   | outbound  | Streaming DATA      |
//! | 10 | both   | connection reader        | inbound   | Streaming DATA      |
//!
//! (1) Streaming DATA inbound transform is applied in the **connection reader**
//! (`handle_msg()`) before spawning a handler task. This ensures deterministic
//! nonce sequencing for stateful transforms (e.g., AEAD) when multiple streams
//! are active concurrently. `StreamReceiver::recv()` passes through the
//! already-decrypted payload without re-applying transform.
//!
//! **Note**: The `streaming_client = false` with non-empty initial payload
//! path is fully supported — `handle_stream()` sends the already-decrypted
//! payload directly, so `StreamReceiver::recv()` passes it through without
//! re-applying `transform_inbound`. This works for **any** `PayloadTransform`
//! implementation, including asymmetric transforms where
//! `transform_outbound(transform_inbound(x))` is not guaranteed to equal `x`.
//!
//! # Socket raw_fd
//!
//! With the `async` and `security_extension` features, `Socket` stores the underlying
//! file descriptor so `AcceptHook` can access it for:
//!   - `getpeername()` to inspect peer identity (e.g., vsock CID)
//!   - Bidirectional handshake I/O (e.g., ECDH key exchange)
//!
//! The fd is captured in the platform-specific `From` impls (vsock, unix, tcp)
//! before the socket is type-erased into the inner `Box<dyn AsyncReadWrite>`.

// ── Always-compiled imports ──
use crate::error::Error;
use crate::proto::{check_oversize, MessageHeader};
use std::sync::Arc;

// ── Always-compiled core types ──────────────────────────────────────────────

/// Helper to extract typed values from [`ConnectionData`].
///
/// The trait is always available; the concrete `ConnectionData` type and its
/// `ConnectionDataExt` impl live in the feature-gated sub-modules below.
pub trait ConnectionDataExt {
    /// Returns the value stored at `key` when it exists and has type `T`.
    ///
    /// Returns `None` when the key is absent or the stored value has a different type.
    fn get_typed<T: 'static>(&self, key: &str) -> Option<&T>;
}

/// Optional bidirectional payload transform, attached per-connection.
///
/// Applied by ttrpc at the wire boundary for **all** message types:
///   - `transform_inbound`: after reading from transport, before dispatching
///   - `transform_outbound`: after handler returns, before writing to transport
///
/// Application-defined. Typical uses:
///   - Encryption/decryption (AES-256-GCM, ChaCha20-Poly1305)
///   - No-op (plaintext, the default)
///
/// **Compression** should be performed at the application layer (before
/// serialization into `Request.payload`), not as a `PayloadTransform`.
/// This avoids payload-expansion edge cases and keeps the transform layer
/// focused on security with predictable, fixed-size overhead.
pub trait PayloadTransform: Send + Sync + std::fmt::Debug {
    /// Decrypt / decode a payload. `aad` is the authenticated-but-unencrypted
    /// header data (`stream_id || type_ || flags`); implementations using AEAD
    /// ciphers should pass it as Additional Authenticated Data so that header
    /// tampering is detected.
    fn transform_inbound(&self, data: Vec<u8>, aad: &[u8]) -> std::result::Result<Vec<u8>, String>;
    /// Encrypt / encode a payload. `aad` has the same semantics as
    /// [`transform_inbound`](Self::transform_inbound).
    fn transform_outbound(&self, data: Vec<u8>, aad: &[u8])
        -> std::result::Result<Vec<u8>, String>;

    /// Maximum number of bytes that `transform_outbound` may add to a payload.
    ///
    /// Used by the framework to pre-check whether a raw payload will fit
    /// within [`MESSAGE_LENGTH_MAX`](crate::proto::MESSAGE_LENGTH_MAX) after
    /// transform, *without* invoking the transform (which may advance
    /// internal state such as AEAD nonce counters).
    ///
    /// The default (64) covers common AEAD schemes (AES-GCM: 28 bytes,
    /// ChaCha20-Poly1305: 28 bytes) with comfortable margin. Override for
    /// transforms with larger expansion.
    fn max_overhead(&self) -> usize {
        64
    }
}

/// Serialize the AAD portion of a message header: `stream_id(4B BE) || type_(1B) || flags(1B)`.
///
/// `length` is intentionally excluded because it changes after transform
/// (e.g., AES-GCM adds nonce + tag). Only the immutable routing/control
/// fields are authenticated.
///
/// Always compiled so that call sites (sync and async) need no cfg gates.
/// The noop `ConnectionContext` ignores `aad` when the feature is disabled,
/// making the computation harmless.
#[allow(dead_code)]
pub(crate) fn serialize_aad(header: &MessageHeader) -> [u8; 6] {
    let mut buf = [0u8; 6];
    buf[0..4].copy_from_slice(&header.stream_id.to_be_bytes());
    buf[4] = header.type_;
    buf[5] = header.flags;
    buf
}

// Re-export: both module variants define ConnectionContext and ConnectionData.
// Since only one `mod hooks` exists at compile time, no cfg gate needed.
pub use hooks::{ConnectionContext, ConnectionData};

// Feature-only items re-exported separately.
#[cfg(all(feature = "async", feature = "security_extension"))]
pub(crate) use hooks::ServerExtensionConfig;
#[cfg(feature = "security_extension")]
pub use hooks::{AcceptHook, ConnectHook, HookError, HookOutput};

#[cfg(feature = "async")]
async fn reserve_message_slot<'a>(
    tx: &'a tokio::sync::mpsc::Sender<crate::asynchronous::SendingMessage>,
    deadline: Option<tokio::time::Instant>,
) -> Result<tokio::sync::mpsc::Permit<'a, crate::asynchronous::SendingMessage>, Error> {
    match tx.try_reserve() {
        Ok(permit) => Ok(permit),
        Err(_) => {
            let reserve = tx.reserve();
            if let Some(deadline) = deadline {
                tokio::time::timeout_at(deadline, reserve)
                    .await
                    .map_err(|_| crate::asynchronous::request_timeout_error())?
            } else {
                reserve.await
            }
            .map_err(|e| Error::Others(format!("reserve channel capacity failed: {e}")))
        }
    }
}

// ── Feature-gated hook types ───────────────────────────────────────────────
//
// All hook-related items live in this inner module behind a single cfg gate.
// Public re-exports below the module make them accessible as
// `crate::security_extension::HookError` etc.
#[cfg(feature = "security_extension")]
mod hooks {
    use super::*;
    #[cfg(feature = "async")]
    use crate::asynchronous::transport::Socket;
    use std::any::Any;
    use std::collections::HashMap;
    use std::fmt;
    use std::os::unix::io::RawFd;
    #[cfg(feature = "async")]
    use std::os::unix::io::{AsRawFd as _, FromRawFd as _, OwnedFd};
    #[cfg(feature = "async")]
    use tokio::{task, time::Duration};

    /// Type-erased per-connection data store.
    ///
    /// Applications attach whatever types they need. ttrpc stores and forwards
    /// them but never inspects the contents.
    ///
    /// `ConnectionData` is **immutable** after connection establishment.
    /// It is wrapped in [`std::sync::Arc`] and shared read-only across all request
    /// handlers on the connection. If the application needs mutable per-connection
    /// state (e.g., request counters, rate-limit tracking), it should maintain that
    /// externally, keyed by a connection identity (e.g., session_id).
    pub type ConnectionData = HashMap<String, Box<dyn Any + Send + Sync>>;

    impl super::ConnectionDataExt for ConnectionData {
        fn get_typed<T: 'static>(&self, key: &str) -> Option<&T> {
            self.get(key)?.downcast_ref::<T>()
        }
    }

    /// Framework-level timeout applied to [`AcceptHook::on_accept`] when invoked
    /// from the async server's accept loop. The hook runs on the blocking
    /// threadpool (`tokio::task::spawn_blocking`); if it does not complete within
    /// this window, ttrpc treats the connection as failed and drops it.
    ///
    /// Hooks are expected to set their own tighter per-I/O timeouts (e.g.,
    /// `SO_RCVTIMEO` / `SO_SNDTIMEO`) for finer-grained control; this value is a
    /// last-resort safety net against a misbehaving or malicious peer.
    #[cfg(feature = "async")]
    pub const ACCEPT_HOOK_TIMEOUT: Duration = Duration::from_secs(30);

    /// Structured error returned by [`AcceptHook::on_accept`] and [`ConnectHook::on_connect`].
    ///
    /// On the server side, ttrpc logs the error and closes the connection.
    /// On the client side, hook failures are propagated back to the caller
    /// and the connection is not used. Structured errors enable proper audit
    /// logging and monitoring.
    pub enum HookError {
        /// Application rejected the connection (e.g., bad CID, failed auth).
        Rejected(String),
        /// Handshake or I/O operation timed out.
        Timeout,
        /// I/O error during handshake.
        Io(std::io::Error),
        /// Other error.
        Other(String),
    }

    impl fmt::Debug for HookError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                HookError::Rejected(msg) => write!(f, "Rejected({})", msg),
                HookError::Timeout => write!(f, "Timeout"),
                HookError::Io(e) => write!(f, "Io({:?})", e),
                HookError::Other(msg) => write!(f, "Other({})", msg),
            }
        }
    }

    impl fmt::Display for HookError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                HookError::Rejected(msg) => write!(f, "connection rejected: {}", msg),
                HookError::Timeout => write!(f, "handshake timeout"),
                HookError::Io(e) => write!(f, "I/O error: {}", e),
                HookError::Other(msg) => write!(f, "{}", msg),
            }
        }
    }

    /// Output produced by [`AcceptHook`] for an accepted connection.
    ///
    /// # Data Propagation Path
    ///
    /// ```text
    /// accept(fd)
    ///   |
    ///   +-> AcceptHook::on_accept(fd) -> HookOutput { data, payload_transform }
    ///   |
    ///   +-> ServerReader { conn_data, payload_transform: Arc<...> }
    ///   |     |
    ///   |     +-- handle_msg(msg)
    ///   |     |     +- MESSAGE_TYPE_REQUEST -> handle_request()
    ///   |     |     |     +-> transform_inbound(msg.payload)       <- unary request (#2)
    ///   |     |     |
    ///   |     |     +- MESSAGE_TYPE_DATA -> transform_inbound -> route to stream_rx
    ///   |     |           |                                       (wire order, #10)
    ///   |     |           +-> StreamReceiver::recv()
    ///   |     |                 +-> pass-through (already decrypted)
    ///   |     |
    ///   |     +-- handle_method(method, req)
    ///   |     |     +- TtrpcContext { connection_data: Arc<data> }
    ///   |     |     |     +-> handler reads ctx.connection_data.get_typed::<T>(key)
    ///   |     |     |
    ///   |     |     +-> transform_outbound(response.payload)       <- unary response (#4)
    ///   |     |
    ///   |     +-- handle_stream(stream, req)
    ///   |           +-> StreamSender { payload_transform: Arc<...> }
    ///   |                 +-> transform_outbound(buf) on .send()   <- streaming data (#9)
    ///   |
    ///   +-> all responses/data written to transport
    /// ```
    ///
    /// **Important**: Streaming DATA inbound transform is applied in the
    /// connection reader (`handle_msg()`) before spawning a handler task,
    /// ensuring deterministic nonce sequencing for stateful transforms.
    /// `StreamReceiver::recv()` passes through the already-decrypted payload.
    pub struct HookOutput {
        /// Opaque metadata attached to this connection for its lifetime.
        /// Forwarded to method/stream handlers via `TtrpcContext.connection_data`.
        /// This data is immutable after `on_accept()` returns.
        pub data: ConnectionData,

        /// Optional payload transform for this connection.
        /// `None` = pass-through (no transformation).
        /// Stored as `Arc` internally so it can be shared with stream handlers.
        pub payload_transform: Option<Box<dyn PayloadTransform>>,
    }

    /// Called once per accepted connection.
    ///
    /// The hook receives the raw fd of the accepted connection and returns:
    ///   - `Ok(HookOutput)`: accept the connection, attach metadata and optional transform
    ///   - `Err(HookError)`: reject the connection; ttrpc closes it immediately
    ///
    /// The hook may inspect the peer address, perform a handshake, look up a
    /// certificate, or do anything else the application requires.
    ///
    /// The hook has exclusive use of the fd -- ttrpc has not started its message
    /// loop yet. The hook may read/write on the fd for handshake purposes.
    ///
    /// # Async server execution model
    ///
    /// On the async server, each accepted connection's hook is dispatched via
    /// [`tokio::task::spawn_blocking`] and wrapped in a framework-level timeout
    /// (`ACCEPT_HOOK_TIMEOUT`). This guarantees:
    ///   - The accept-loop worker is never blocked by a slow handshake, so other
    ///     connections keep being accepted in parallel.
    ///   - Current-thread runtimes cannot deadlock (the hook runs on a separate
    ///     blocking thread).
    ///   - A malicious peer cannot stall the accept loop beyond the timeout.
    ///
    /// Hooks are still expected to set their own per-I/O timeouts
    /// (`SO_RCVTIMEO` / `SO_SNDTIMEO`) for finer-grained control; the
    /// framework-level timeout is a last-resort safety net.
    pub trait AcceptHook: Send + Sync + std::fmt::Debug {
        /// Inspects or negotiates an accepted connection before ttrpc reads from it.
        ///
        /// The hook may use `fd` during this call but does not take ownership of it.
        ///
        /// # Errors
        ///
        /// Returns [`HookError`] to reject the connection or report a handshake failure.
        fn on_accept(&self, fd: RawFd) -> std::result::Result<HookOutput, HookError>;
    }

    impl<T: AcceptHook + ?Sized> AcceptHook for Box<T> {
        fn on_accept(&self, fd: RawFd) -> std::result::Result<HookOutput, HookError> {
            (**self).on_accept(fd)
        }
    }

    /// Client-side connection hook, symmetric to [`AcceptHook`].
    ///
    /// Called after a client connection is established, before RPC messages are sent.
    /// The hook receives the raw fd and can perform:
    ///   - Peer identity verification
    ///   - Handshake I/O (e.g., ECDH client-side)
    ///   - Attaching connection metadata
    ///   - Installing a [`PayloadTransform`]
    ///
    /// If the hook returns an error, [`Client::with_hook`](crate::asynchronous::Client::with_hook)
    /// returns the error and the connection is **not** used, preventing a silent
    /// downgrade to an untransformed connection.
    ///
    /// **WARNING**: The hook **must** set I/O timeouts before any read/write on the fd.
    pub trait ConnectHook: Send + Sync + std::fmt::Debug {
        /// Inspects or negotiates a client connection before ttrpc writes to it.
        ///
        /// The hook may use `fd` during this call but does not take ownership of it.
        ///
        /// # Errors
        ///
        /// Returns [`HookError`] to reject the connection or report a handshake failure.
        fn on_connect(&self, fd: RawFd) -> std::result::Result<HookOutput, HookError>;
    }

    impl<T: ConnectHook + ?Sized> ConnectHook for Box<T> {
        fn on_connect(&self, fd: RawFd) -> std::result::Result<HookOutput, HookError> {
            (**self).on_connect(fd)
        }
    }

    /// Server-wide extension configuration. Immutable after server build.
    ///
    /// Holds server-scoped hooks (currently only the [`AcceptHook`]) that apply to
    /// every accepted connection. Shared via `Arc<ServerExtensionConfig>` from
    /// `Server` down to the accept loop.
    ///
    /// Per-connection state (data, payload transform) lives in
    /// [`ConnectionContext`], which is constructed from the hook output after a
    /// connection is accepted.
    #[cfg(feature = "async")]
    #[derive(Debug, Default)]
    pub(crate) struct ServerExtensionConfig {
        /// Server-side hook called on each accepted connection.
        pub(crate) accept_hook: Option<Arc<dyn AcceptHook>>,
    }

    #[cfg(feature = "async")]
    impl ServerExtensionConfig {
        /// Invoke the accept hook for a newly accepted connection.
        ///
        /// The (synchronous) hook runs on tokio's blocking threadpool via
        /// [`task::spawn_blocking`] and is wrapped in a framework-level
        /// [`ACCEPT_HOOK_TIMEOUT`]. This keeps the async accept loop responsive:
        ///   - The runtime worker that accepts new connections is never blocked by
        ///     a slow handshake.
        ///   - A current-thread runtime cannot deadlock (the hook runs on a
        ///     separate blocking thread).
        ///   - A malicious peer cannot stall the accept loop beyond the timeout.
        ///
        /// Returns:
        /// - `Ok(None)`: no hook set → accept with empty defaults
        /// - `Ok(Some(output))`: hook accepted → use hook output
        /// - `Err(e)`: hook rejected the connection, timed out, or could not run
        pub(crate) async fn on_accept(
            &self,
            conn: &Socket,
        ) -> Result<Option<HookOutput>, HookError> {
            let hook = match self.accept_hook {
                Some(ref h) => Arc::clone(h),
                None => return Ok(None),
            };
            let fd = conn.as_raw_fd().ok_or_else(|| {
                HookError::Other(
                    "accept hook requires a raw fd; use Server::bind() or construct Socket from a platform stream (UnixStream, TcpStream, VsockStream)".to_string(),
                )
            })?;
            // Dup the fd so the blocking task owns its own copy. This prevents
            // fd reuse: if the outer task drops the connection on timeout, the
            // hook's dup'd fd remains valid (and is not reassigned by the OS)
            // until the hook finishes and drops it.
            let owned_fd: OwnedFd = unsafe {
                let dup_fd = libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0);
                if dup_fd < 0 {
                    return Err(HookError::Io(std::io::Error::last_os_error()));
                }
                OwnedFd::from_raw_fd(dup_fd)
            };
            // spawn_blocking moves the hook execution off the async worker so
            // other connections keep being accepted. A slow or malicious peer
            // cannot stall the accept loop beyond `ACCEPT_HOOK_TIMEOUT`.
            let mut handle = task::spawn_blocking(move || {
                let hook_fd = owned_fd.as_raw_fd();
                // owned_fd stays alive for the closure's lifetime, ensuring
                // the dup'd fd is not reused by the OS until the hook returns.
                let _guard = owned_fd;
                hook.on_accept(hook_fd)
            });
            tokio::select! {
                result = &mut handle => {
                    match result {
                        Ok(Ok(output)) => Ok(Some(output)),
                        Ok(Err(e)) => Err(e),
                        Err(join_err) => Err(HookError::Other(format!(
                            "accept hook task failed: {}",
                            join_err
                        ))),
                    }
                }
                _ = tokio::time::sleep(ACCEPT_HOOK_TIMEOUT) => {
                    // Cancel the blocking task. For queued tasks this prevents
                    // execution entirely. For already-running tasks, abort is
                    // best-effort (spawn_blocking cannot be forcibly killed);
                    // the hook's dup'd fd ensures no fd reuse occurs even if
                    // the task continues briefly after abort.
                    handle.abort();
                    Err(HookError::Timeout)
                }
            }
        }
    }

    /// Bundles all per-connection extension data into a single propagation unit.
    ///
    /// By passing a single `Arc<ConnectionContext>` through client and server
    /// internals, we avoid scattering feature gates across every struct and
    /// function. Server-wide configuration (e.g., the accept hook) lives in
    /// `ServerExtensionConfig`.
    #[derive(Debug, Default)]
    pub struct ConnectionContext {
        /// Opaque per-connection metadata. Immutable after accept.
        pub data: Arc<ConnectionData>,
        /// Optional payload transform. `None` = pass-through.
        pub payload_transform: Option<Arc<dyn PayloadTransform>>,
        /// Serializes outbound transform + enqueue to prevent wire-order
        /// races with stateful transforms (e.g., AEAD nonce counters).
        #[cfg(feature = "async")]
        async_outbound_lock: std::sync::Arc<tokio::sync::Mutex<()>>,
        /// Sync-path outbound lock (analogous to `async_outbound_lock` for tokio paths).
        #[cfg(feature = "sync")]
        sync_outbound_lock: std::sync::Mutex<()>,
    }

    impl ConnectionContext {
        /// Create a context from hook output. No hook → empty defaults.
        pub fn new(output: Option<HookOutput>) -> Self {
            match output {
                Some(o) => Self {
                    data: Arc::new(o.data),
                    payload_transform: o.payload_transform.map(Arc::from),
                    #[cfg(feature = "async")]
                    async_outbound_lock: Arc::new(tokio::sync::Mutex::new(())),
                    #[cfg(feature = "sync")]
                    sync_outbound_lock: std::sync::Mutex::new(()),
                },
                None => Self::default(),
            }
        }

        /// Maximum raw (pre-transform) payload size that is guaranteed to fit
        /// within [`MESSAGE_LENGTH_MAX`](crate::proto::MESSAGE_LENGTH_MAX)
        /// after `transform_outbound`.
        ///
        /// Use this to pre-check payload size *before* invoking the transform,
        /// avoiding state advancement (e.g., AEAD nonce counters) on payloads
        /// that would be rejected post-transform.
        pub fn max_raw_payload_len(&self) -> usize {
            match self.payload_transform {
                Some(ref xform) => {
                    crate::proto::MESSAGE_LENGTH_MAX.saturating_sub(xform.max_overhead())
                }
                None => crate::proto::MESSAGE_LENGTH_MAX,
            }
        }

        /// Apply inbound payload transform (if any).
        /// Returns the transformed payload, or the original if no transform is set.
        pub fn transform_inbound(&self, payload: Vec<u8>, aad: &[u8]) -> Result<Vec<u8>, String> {
            match self.payload_transform {
                Some(ref xform) => xform.transform_inbound(payload, aad),
                None => Ok(payload),
            }
        }

        /// Apply outbound payload transform (if any).
        /// Returns the transformed payload, or the original if no transform is set.
        pub fn transform_outbound(&self, payload: Vec<u8>, aad: &[u8]) -> Result<Vec<u8>, String> {
            match self.payload_transform {
                Some(ref xform) => xform.transform_outbound(payload, aad),
                None => Ok(payload),
            }
        }

        /// Inbound pipeline on a raw buffer: apply payload transform and enforce
        /// the post-transform size limit.
        ///
        /// Intended for sync paths that shuttle a `Vec<u8>` rather than a full
        /// `GenMessage` (e.g., the sync server reader and sync client sender /
        /// receiver threads). For message-level pipelines that also update
        /// `header.length`, use [`ConnectionContext::inbound`].
        ///
        /// `rpc_error` selects the error flavor (see [`ConnectionContext::inbound`]).
        pub fn inbound_buf(
            &self,
            data: Vec<u8>,
            aad: &[u8],
            rpc_error: bool,
        ) -> Result<Vec<u8>, Error> {
            let transformed = match self.payload_transform {
                Some(ref xform) => xform
                    .transform_inbound(data, aad)
                    .map_err(|e| Error::Others(format!("transform_inbound failed: {}", e)))?,
                None => data,
            };
            check_oversize(transformed.len(), rpc_error)?;
            Ok(transformed)
        }

        /// Outbound pipeline on a raw buffer: apply payload transform and enforce
        /// the post-transform size limit.
        ///
        /// Counterpart to [`ConnectionContext::inbound_buf`] for the send path.
        /// `rpc_error` selects the error flavor (see [`ConnectionContext::inbound`]).
        pub fn outbound_buf(
            &self,
            data: Vec<u8>,
            aad: &[u8],
            rpc_error: bool,
        ) -> Result<Vec<u8>, Error> {
            let transformed = match self.payload_transform {
                Some(ref xform) => {
                    let max_len = self.max_raw_payload_len();
                    if data.len() > max_len {
                        return Err(Error::Others(format!(
                            "payload {} bytes exceeds safe limit {} bytes \
                             (MESSAGE_LENGTH_MAX - transform overhead)",
                            data.len(),
                            max_len
                        )));
                    }
                    xform
                        .transform_outbound(data, aad)
                        .map_err(|e| Error::Others(format!("transform_outbound failed: {}", e)))?
                }
                None => data,
            };
            check_oversize(transformed.len(), rpc_error)?;
            Ok(transformed)
        }

        /// Inbound pipeline: apply payload transform in-place and enforce the
        /// post-transform size limit on a `GenMessage`.
        ///
        /// Updates `msg.header.length` to match the transformed payload. Then
        /// delegates to `check_oversize` for the Layer 2 size
        /// guard, which rejects payloads that expanded past `MESSAGE_LENGTH_MAX`
        /// (e.g., from a decompression transform).
        ///
        /// The header's routing fields (`stream_id`, `type_`, `flags`) are
        /// serialized and passed as AAD to the transform for authentication.
        ///
        /// `rpc_error` selects the error flavor returned by the size check:
        /// - `true` (server paths): returns `Error::RpcStatus(INVALID_ARGUMENT)`
        /// - `false` (client paths): returns `Error::Others(...)`
        pub fn inbound(
            &self,
            msg: &mut crate::proto::GenMessage,
            rpc_error: bool,
        ) -> Result<(), Error> {
            if let Some(ref xform) = self.payload_transform {
                let aad = serialize_aad(&msg.header);
                msg.payload = xform
                    .transform_inbound(std::mem::take(&mut msg.payload), &aad)
                    .map_err(|e| Error::Others(format!("transform_inbound failed: {}", e)))?;
                msg.header.length = msg.payload.len() as u32;
            }
            check_oversize(msg.payload.len(), rpc_error)
        }

        /// Outbound pipeline: apply payload transform in-place and enforce the
        /// post-transform size limit on a `GenMessage`.
        ///
        /// Updates `msg.header.length` to match the transformed payload. Then
        /// delegates to `check_oversize` for the Layer 2 size
        /// guard.
        ///
        /// The header's routing fields (`stream_id`, `type_`, `flags`) are
        /// serialized and passed as AAD to the transform for authentication.
        ///
        /// `rpc_error` selects the error flavor returned by the size check
        /// (see [`ConnectionContext::inbound`] for semantics).
        pub fn outbound(
            &self,
            msg: &mut crate::proto::GenMessage,
            rpc_error: bool,
        ) -> Result<(), Error> {
            if let Some(ref xform) = self.payload_transform {
                let max_len = self.max_raw_payload_len();
                if msg.payload.len() > max_len {
                    return Err(Error::Others(format!(
                        "payload {} bytes exceeds safe limit {} bytes \
                         (MESSAGE_LENGTH_MAX - transform overhead); \
                         reduce payload size",
                        msg.payload.len(),
                        max_len
                    )));
                }
                let aad = serialize_aad(&msg.header);
                msg.payload = xform
                    .transform_outbound(std::mem::take(&mut msg.payload), &aad)
                    .map_err(|e| Error::Others(format!("transform_outbound failed: {}", e)))?;
                msg.header.length = msg.payload.len() as u32;
            }
            check_oversize(msg.payload.len(), rpc_error)
        }

        /// Atomically: reserve capacity → lock outbound → transform → send.
        ///
        /// Reserves channel capacity first (cancellable await), then acquires
        /// the outbound lock and transforms (non-cancellable). This prevents
        /// nonce desynchronization if the future is cancelled after advancing
        /// the stateful transform but before the frame reaches the channel.
        ///
        /// When `await_ack` is true, waits for the writer task to confirm the
        /// frame reached the transport (needed for streaming paths that depend
        /// on write success before updating state). When false, returns after
        /// enqueue — suitable for unary request/response paths where the caller
        /// applies its own timeout on the reply.
        #[cfg(feature = "async")]
        pub async fn transform_send(
            &self,
            msg: &mut crate::proto::GenMessage,
            tx: &tokio::sync::mpsc::Sender<crate::asynchronous::SendingMessage>,
            rpc_error: bool,
            await_ack: bool,
        ) -> Result<(), Error> {
            self.transform_send_inner(msg, tx, rpc_error, await_ack, None)
                .await
        }

        #[cfg(feature = "async")]
        pub(crate) async fn transform_send_with_control(
            &self,
            msg: &mut crate::proto::GenMessage,
            tx: &tokio::sync::mpsc::Sender<crate::asynchronous::SendingMessage>,
            rpc_error: bool,
            await_ack: bool,
            control: crate::asynchronous::MessageControl,
        ) -> Result<(), Error> {
            self.transform_send_inner(msg, tx, rpc_error, await_ack, Some(control))
                .await
        }

        #[cfg(feature = "async")]
        async fn transform_send_inner(
            &self,
            msg: &mut crate::proto::GenMessage,
            tx: &tokio::sync::mpsc::Sender<crate::asynchronous::SendingMessage>,
            rpc_error: bool,
            await_ack: bool,
            control: Option<crate::asynchronous::MessageControl>,
        ) -> Result<(), Error> {
            let deadline = control.as_ref().and_then(|control| control.deadline());
            // Reserve capacity first — this is the only cancellable await point.
            // If cancelled here, no nonce has been advanced.
            let permit = reserve_message_slot(tx, deadline).await?;

            let _guard = match self.async_outbound_lock.try_lock() {
                Ok(guard) => guard,
                Err(_) => {
                    let lock = self.async_outbound_lock.lock();
                    if let Some(deadline) = deadline {
                        tokio::time::timeout_at(deadline, lock)
                            .await
                            .map_err(|_| crate::asynchronous::request_timeout_error())?
                    } else {
                        lock.await
                    }
                }
            };
            self.outbound(msg, rpc_error)?;
            let taken = std::mem::take(msg);

            // A stateful transform may advance a nonce or counter. Once that
            // happens, the frame must not be discarded by the writer on
            // timeout or cancellation, or the peers will become desynchronized.
            // The caller's deadline still bounds reserve and lock acquisition.
            let control = if self.payload_transform.is_some() {
                None
            } else {
                control
            };

            // From here on: no await until the frame is in the channel.
            // permit.send() is synchronous — cannot be cancelled.
            if await_ack {
                let (result_tx, result_rx) = tokio::sync::oneshot::channel();
                let mut sending_msg =
                    crate::asynchronous::SendingMessage::new_with_result(taken, result_tx);
                sending_msg.control = control;
                permit.send(sending_msg);
                drop(_guard);
                result_rx
                    .await
                    .map_err(|_| Error::Others("writer task dropped result channel".to_string()))?
            } else {
                let sending_msg = match control {
                    Some(control) => {
                        crate::asynchronous::SendingMessage::new_with_control(taken, control)
                    }
                    None => crate::asynchronous::SendingMessage::new(taken),
                };
                permit.send(sending_msg);
                Ok(())
            }
        }

        /// Sync-path equivalent of [`transform_send`](Self::transform_send).
        /// Serializes `outbound_buf` + `tx.send` under a std mutex to prevent
        /// nonce ordering races across concurrent handler threads.
        #[cfg(feature = "sync")]
        pub fn send_response_sync(
            &self,
            mut buf: Vec<u8>,
            aad: &[u8],
            tx: &std::sync::mpsc::Sender<(crate::proto::MessageHeader, Vec<u8>)>,
            mh: crate::proto::MessageHeader,
        ) -> Result<(), Error> {
            let _guard = self.sync_outbound_lock.lock().unwrap();
            buf = self.outbound_buf(buf, aad, true)?;
            let mh = crate::proto::MessageHeader {
                length: buf.len() as u32,
                ..mh
            };
            tx.send((mh, buf))
                .map_err(|e| Error::Others(format!("send to wire channel failed: {e}")))
        }
    }
} // mod hooks

// ── Noop types (feature disabled) ──────────────────────────────────────────
//
// When security_extension is disabled, these types replace the real ones with
// minimal-cost stubs (an Arc<ConnectionContext> is still allocated per connection
// for API uniformity, but its fields are unused). All items inside have no cfg gates.
#[cfg(not(feature = "security_extension"))]
mod hooks {
    use super::*;

    /// Zero-sized placeholder when `security_extension` is disabled.
    /// `Arc<ConnectionData>` costs ~24 bytes (ArcInner header only, no HashMap).
    #[derive(Clone, Debug, Default)]
    pub struct ConnectionData;

    impl super::ConnectionDataExt for ConnectionData {
        fn get_typed<T: 'static>(&self, _key: &str) -> Option<&T> {
            None
        }
    }

    /// No-op connection context used when `security_extension` is disabled.
    ///
    /// All pipeline methods (`inbound`, `outbound`, `inbound_buf`, `outbound_buf`)
    /// skip payload transform and only enforce the message size limit.
    ///
    /// The real [`ConnectionContext`] (available with `--features security_extension`)
    /// adds per-connection data and payload encryption on top of the same method
    /// signatures.
    #[derive(Clone, Debug, Default)]
    #[doc(hidden)]
    pub struct ConnectionContext {
        /// Always empty when security_extension is disabled.
        pub data: Arc<ConnectionData>,
        /// Always `None` when security_extension is disabled.
        pub payload_transform: Option<Arc<dyn PayloadTransform>>,
    }

    #[allow(dead_code)]
    impl ConnectionContext {
        /// No-op transform always reports the full limit (no overhead).
        pub fn max_raw_payload_len(&self) -> usize {
            crate::proto::MESSAGE_LENGTH_MAX
        }

        /// Inbound pipeline: size-check only (no transform).
        pub fn inbound(
            &self,
            msg: &mut crate::proto::GenMessage,
            rpc_error: bool,
        ) -> Result<(), Error> {
            check_oversize(msg.payload.len(), rpc_error)
        }

        /// Outbound pipeline: size-check only (no transform).
        pub fn outbound(
            &self,
            msg: &mut crate::proto::GenMessage,
            rpc_error: bool,
        ) -> Result<(), Error> {
            check_oversize(msg.payload.len(), rpc_error)
        }

        /// Inbound buffer pipeline: size-check only (no transform).
        pub fn inbound_buf(
            &self,
            data: Vec<u8>,
            _aad: &[u8],
            rpc_error: bool,
        ) -> Result<Vec<u8>, Error> {
            check_oversize(data.len(), rpc_error)?;
            Ok(data)
        }

        /// Outbound buffer pipeline: size-check only (no transform).
        pub fn outbound_buf(
            &self,
            data: Vec<u8>,
            _aad: &[u8],
            rpc_error: bool,
        ) -> Result<Vec<u8>, Error> {
            check_oversize(data.len(), rpc_error)?;
            Ok(data)
        }

        /// Size-check + enqueue (no transform, no lock when security_extension is disabled).
        #[cfg(feature = "async")]
        pub async fn transform_send(
            &self,
            msg: &mut crate::proto::GenMessage,
            tx: &tokio::sync::mpsc::Sender<crate::asynchronous::SendingMessage>,
            rpc_error: bool,
            await_ack: bool,
        ) -> Result<(), Error> {
            self.transform_send_inner(msg, tx, rpc_error, await_ack, None)
                .await
        }

        #[cfg(feature = "async")]
        pub(crate) async fn transform_send_with_control(
            &self,
            msg: &mut crate::proto::GenMessage,
            tx: &tokio::sync::mpsc::Sender<crate::asynchronous::SendingMessage>,
            rpc_error: bool,
            await_ack: bool,
            control: crate::asynchronous::MessageControl,
        ) -> Result<(), Error> {
            self.transform_send_inner(msg, tx, rpc_error, await_ack, Some(control))
                .await
        }

        #[cfg(feature = "async")]
        async fn transform_send_inner(
            &self,
            msg: &mut crate::proto::GenMessage,
            tx: &tokio::sync::mpsc::Sender<crate::asynchronous::SendingMessage>,
            rpc_error: bool,
            await_ack: bool,
            control: Option<crate::asynchronous::MessageControl>,
        ) -> Result<(), Error> {
            self.outbound(msg, rpc_error)?;
            let deadline = control.as_ref().and_then(|control| control.deadline());
            let permit = reserve_message_slot(tx, deadline).await?;
            let taken = std::mem::take(msg);
            if await_ack {
                let (result_tx, result_rx) = tokio::sync::oneshot::channel();
                let mut sending_msg =
                    crate::asynchronous::SendingMessage::new_with_result(taken, result_tx);
                sending_msg.control = control;
                permit.send(sending_msg);
                result_rx
                    .await
                    .map_err(|_| Error::Others("writer task dropped result channel".to_string()))?
            } else {
                let sending_msg = match control {
                    Some(control) => {
                        crate::asynchronous::SendingMessage::new_with_control(taken, control)
                    }
                    None => crate::asynchronous::SendingMessage::new(taken),
                };
                permit.send(sending_msg);
                Ok(())
            }
        }

        /// Sync-path size-check + enqueue (no transform, no lock needed when
        /// security_extension is disabled — no stateful transforms exist).
        #[cfg(feature = "sync")]
        pub fn send_response_sync(
            &self,
            buf: Vec<u8>,
            _aad: &[u8],
            tx: &std::sync::mpsc::Sender<(crate::proto::MessageHeader, Vec<u8>)>,
            mh: crate::proto::MessageHeader,
        ) -> Result<(), Error> {
            check_oversize(buf.len(), true)?;
            let mh = crate::proto::MessageHeader {
                length: buf.len() as u32,
                ..mh
            };
            tx.send((mh, buf))
                .map_err(|e| Error::Others(format!("send to wire channel failed: {e}")))
        }
    }
} // mod hooks

#[cfg(all(test, feature = "security_extension"))]
mod tests {
    use super::*;
    #[cfg(all(feature = "async", feature = "security_extension"))]
    use crate::asynchronous::transport::Socket;
    use crate::proto::GenMessage;
    #[cfg(feature = "async")]
    use std::os::unix::io::RawFd;

    // ── ConnectionData + ConnectionDataExt ────────────────────────────────

    #[test]
    fn connection_data_empty() {
        let data = ConnectionData::new();
        assert!(data.is_empty());
        assert_eq!(data.get_typed::<String>("missing"), None);
        assert_eq!(data.get_typed::<u64>("missing"), None);
    }

    #[test]
    fn connection_data_insert_and_get_typed() {
        let mut data = ConnectionData::new();
        data.insert("name".into(), Box::new(String::from("alice")));
        data.insert("count".into(), Box::new(42u64));
        data.insert("flag".into(), Box::new(true));

        assert_eq!(
            data.get_typed::<String>("name").map(String::as_str),
            Some("alice")
        );
        assert_eq!(data.get_typed::<u64>("count"), Some(&42u64));
        assert_eq!(data.get_typed::<bool>("flag"), Some(&true));
    }

    #[test]
    fn connection_data_wrong_type_returns_none() {
        let mut data = ConnectionData::new();
        data.insert("key".into(), Box::new(String::from("hello")));
        // Request as u64 — wrong type, should return None
        assert_eq!(data.get_typed::<u64>("key"), None);
        assert_eq!(data.get_typed::<bool>("key"), None);
    }

    #[test]
    fn connection_data_missing_key_returns_none() {
        let mut data = ConnectionData::new();
        data.insert("a".into(), Box::new(1u32));
        assert_eq!(data.get_typed::<u32>("b"), None);
    }

    // ── HookError ──────────────────────────────────────────────────────

    #[test]
    fn accept_error_debug_rejected() {
        let e = HookError::Rejected("bad cid".into());
        assert_eq!(format!("{:?}", e), "Rejected(bad cid)");
    }

    #[test]
    fn accept_error_debug_timeout() {
        let e = HookError::Timeout;
        assert_eq!(format!("{:?}", e), "Timeout");
    }

    #[test]
    fn accept_error_debug_io() {
        let e = HookError::Io(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe"));
        let dbg = format!("{:?}", e);
        assert!(dbg.starts_with("Io("));
    }

    #[test]
    fn accept_error_debug_other() {
        let e = HookError::Other("misc".into());
        assert_eq!(format!("{:?}", e), "Other(misc)");
    }

    #[test]
    fn accept_error_display_rejected() {
        let e = HookError::Rejected("no auth".into());
        assert_eq!(format!("{}", e), "connection rejected: no auth");
    }

    #[test]
    fn accept_error_display_timeout() {
        let e = HookError::Timeout;
        assert_eq!(format!("{}", e), "handshake timeout");
    }

    #[test]
    fn accept_error_display_io() {
        let e = HookError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "refused",
        ));
        let s = format!("{}", e);
        assert!(s.starts_with("I/O error:"));
    }

    #[test]
    fn accept_error_display_other() {
        let e = HookError::Other("something went wrong".into());
        assert_eq!(format!("{}", e), "something went wrong");
    }

    // ── HookOutput ─────────────────────────────────────────────────────

    #[test]
    fn accept_output_empty_data_no_transform() {
        let out = HookOutput {
            data: ConnectionData::new(),
            payload_transform: None,
        };
        assert!(out.data.is_empty());
        assert!(out.payload_transform.is_none());
    }

    #[test]
    fn accept_output_with_data_and_transform() {
        let mut data = ConnectionData::new();
        data.insert("peer".into(), Box::new(1234u32));

        let out = HookOutput {
            data,
            payload_transform: Some(Box::new(NoopTransform)),
        };
        assert_eq!(out.data.get_typed::<u32>("peer"), Some(&1234u32));
        assert!(out.payload_transform.is_some());
    }

    // ── Helper: NoopTransform ────────────────────────────────────────────

    #[derive(Debug)]
    struct NoopTransform;

    impl PayloadTransform for NoopTransform {
        fn transform_inbound(&self, data: Vec<u8>, _aad: &[u8]) -> Result<Vec<u8>, String> {
            Ok(data)
        }
        fn transform_outbound(&self, data: Vec<u8>, _aad: &[u8]) -> Result<Vec<u8>, String> {
            Ok(data)
        }
    }

    // ── ConnectionContext construction ───────────────────────────────────

    #[test]
    fn context_default_is_empty() {
        let ctx = ConnectionContext::default();
        assert!(ctx.data.is_empty());
        assert!(ctx.payload_transform.is_none());
    }

    #[test]
    fn context_new_none_equals_default() {
        let ctx = ConnectionContext::new(None);
        assert!(ctx.data.is_empty());
        assert!(ctx.payload_transform.is_none());
    }

    #[test]
    fn context_new_with_data_only() {
        let mut data = ConnectionData::new();
        data.insert("role".into(), Box::new(String::from("server")));
        let output = HookOutput {
            data,
            payload_transform: None,
        };
        let ctx = ConnectionContext::new(Some(output));
        assert_eq!(
            ctx.data.get_typed::<String>("role").map(String::as_str),
            Some("server")
        );
        assert!(ctx.payload_transform.is_none());
    }

    #[test]
    fn context_new_with_data_and_transform() {
        let mut data = ConnectionData::new();
        data.insert("cid".into(), Box::new(42u32));
        let output = HookOutput {
            data,
            payload_transform: Some(Box::new(NoopTransform)),
        };
        let ctx = ConnectionContext::new(Some(output));
        assert_eq!(ctx.data.get_typed::<u32>("cid"), Some(&42u32));
        assert!(ctx.payload_transform.is_some());
    }

    #[test]
    fn context_debug_format() {
        let ctx = ConnectionContext::default();
        let dbg = format!("{:?}", ctx);
        assert!(dbg.contains("ConnectionContext"));
    }

    // ── ConnectionContext transform pass-through (no transform) ──────────

    #[test]
    fn transform_inbound_no_transform_returns_original() {
        let ctx = ConnectionContext::default();
        let payload = vec![1, 2, 3, 4];
        let result = ctx.transform_inbound(payload.clone(), &[]).unwrap();
        assert_eq!(result, payload);
    }

    #[test]
    fn transform_outbound_no_transform_returns_original() {
        let ctx = ConnectionContext::default();
        let payload = vec![5, 6, 7, 8];
        let result = ctx.transform_outbound(payload.clone(), &[]).unwrap();
        assert_eq!(result, payload);
    }

    #[test]
    fn inbound_no_transform_is_noop() {
        let ctx = ConnectionContext::default();
        let mut msg = GenMessage {
            payload: vec![10, 20, 30],
            ..Default::default()
        };
        ctx.inbound(&mut msg, false).unwrap();
        assert_eq!(msg.payload, vec![10, 20, 30]);
    }

    #[test]
    fn outbound_no_transform_is_noop() {
        let ctx = ConnectionContext::default();
        let mut msg = GenMessage {
            payload: vec![10, 20, 30],
            ..Default::default()
        };
        ctx.outbound(&mut msg, false).unwrap();
        assert_eq!(msg.payload, vec![10, 20, 30]);
    }

    // ── ConnectionContext with NoopTransform ─────────────────────────────

    #[test]
    fn transform_inbound_noop_preserves_data() {
        let output = HookOutput {
            data: ConnectionData::new(),
            payload_transform: Some(Box::new(NoopTransform)),
        };
        let ctx = ConnectionContext::new(Some(output));
        let payload = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let result = ctx.transform_inbound(payload.clone(), &[]).unwrap();
        assert_eq!(result, payload);
    }

    #[test]
    fn transform_outbound_noop_preserves_data() {
        let output = HookOutput {
            data: ConnectionData::new(),
            payload_transform: Some(Box::new(NoopTransform)),
        };
        let ctx = ConnectionContext::new(Some(output));
        let payload = vec![0xCA, 0xFE];
        let result = ctx.transform_outbound(payload.clone(), &[]).unwrap();
        assert_eq!(result, payload);
    }

    // ── XOR-0xA5A5 mock encryption ──────────────────────────────────────
    //
    // Wire format: [algo_id: u16 BE][payload_len: u32 BE][encrypted_data...]
    // Header = 6 bytes. algo_id = 0x0001 for XOR-0xA5A5.
    // Encryption: XOR each u16 (big-endian) chunk with 0xA5A5.
    // Odd-length payloads are padded with 0x00 before encryption;
    // decryption truncates to original payload_len.

    const XOR_ALGO_ID: u16 = 0x0001;
    const XOR_KEY: u16 = 0xA5A5;
    const XOR_HEADER_LEN: usize = 6; // 2 (algo_id) + 4 (payload_len)

    #[derive(Debug)]
    struct XorPayloadTransform;

    impl PayloadTransform for XorPayloadTransform {
        /// Encrypt: prepend header, XOR payload with 0xA5A5 in u16 chunks.
        fn transform_outbound(&self, data: Vec<u8>, _aad: &[u8]) -> Result<Vec<u8>, String> {
            let payload_len = data.len();
            // Pad to even length for u16 XOR
            let mut padded = data;
            if padded.len() % 2 != 0 {
                padded.push(0x00);
            }
            // XOR encrypt in u16 big-endian chunks
            let mut encrypted = Vec::with_capacity(XOR_HEADER_LEN + padded.len());
            // Header: algo_id (u16 BE) + payload_len (u32 BE)
            encrypted.extend_from_slice(&XOR_ALGO_ID.to_be_bytes());
            encrypted.extend_from_slice(&(payload_len as u32).to_be_bytes());
            // Encrypted body
            for chunk in padded.chunks(2) {
                let val = u16::from_be_bytes([chunk[0], chunk[1]]);
                let xored = val ^ XOR_KEY;
                encrypted.extend_from_slice(&xored.to_be_bytes());
            }
            Ok(encrypted)
        }

        /// Decrypt: verify header, XOR encrypted data with 0xA5A5, truncate.
        fn transform_inbound(&self, data: Vec<u8>, _aad: &[u8]) -> Result<Vec<u8>, String> {
            if data.len() < XOR_HEADER_LEN {
                return Err(format!(
                    "xor: packet too short ({} < {})",
                    data.len(),
                    XOR_HEADER_LEN
                ));
            }
            let algo_id = u16::from_be_bytes([data[0], data[1]]);
            if algo_id != XOR_ALGO_ID {
                return Err(format!("xor: unknown algo 0x{:04X}", algo_id));
            }
            let payload_len = u32::from_be_bytes([data[2], data[3], data[4], data[5]]) as usize;
            let encrypted = &data[XOR_HEADER_LEN..];
            if encrypted.len() % 2 != 0 {
                return Err("xor: encrypted data has odd length".into());
            }
            // Decrypt: XOR with same key (symmetric)
            let mut decrypted = Vec::with_capacity(encrypted.len());
            for chunk in encrypted.chunks(2) {
                let val = u16::from_be_bytes([chunk[0], chunk[1]]);
                let xored = val ^ XOR_KEY;
                decrypted.extend_from_slice(&xored.to_be_bytes());
            }
            // Truncate to original payload length
            decrypted.truncate(payload_len);
            Ok(decrypted)
        }
    }

    /// Helper: build a ConnectionContext with XorPayloadTransform.
    fn xor_context() -> ConnectionContext {
        let output = HookOutput {
            data: ConnectionData::new(),
            payload_transform: Some(Box::new(XorPayloadTransform)),
        };
        ConnectionContext::new(Some(output))
    }

    // ── XOR: roundtrip tests ────────────────────────────────────────────

    #[test]
    fn xor_roundtrip_basic() {
        let ctx = xor_context();
        let original = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        let encrypted = ctx.transform_outbound(original.clone(), &[]).unwrap();
        let decrypted = ctx.transform_inbound(encrypted, &[]).unwrap();
        assert_eq!(decrypted, original);
    }

    #[test]
    fn xor_roundtrip_empty_payload() {
        let ctx = xor_context();
        let original = vec![];
        let encrypted = ctx.transform_outbound(original.clone(), &[]).unwrap();
        // Header only, no body
        assert_eq!(encrypted.len(), XOR_HEADER_LEN);
        let decrypted = ctx.transform_inbound(encrypted, &[]).unwrap();
        assert_eq!(decrypted, original);
    }

    #[test]
    fn xor_roundtrip_odd_length() {
        let ctx = xor_context();
        let original = vec![0xAA, 0xBB, 0xCC]; // 3 bytes
        let encrypted = ctx.transform_outbound(original.clone(), &[]).unwrap();
        // Header (6) + 2 u16 chunks (4 bytes, padded)
        assert_eq!(encrypted.len(), XOR_HEADER_LEN + 4);
        let decrypted = ctx.transform_inbound(encrypted, &[]).unwrap();
        assert_eq!(decrypted, original);
    }

    #[test]
    fn xor_roundtrip_single_byte() {
        let ctx = xor_context();
        let original = vec![0xFF];
        let encrypted = ctx.transform_outbound(original.clone(), &[]).unwrap();
        let decrypted = ctx.transform_inbound(encrypted, &[]).unwrap();
        assert_eq!(decrypted, original);
    }

    #[test]
    fn xor_roundtrip_large_payload() {
        let ctx = xor_context();
        let original: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
        let encrypted = ctx.transform_outbound(original.clone(), &[]).unwrap();
        let decrypted = ctx.transform_inbound(encrypted, &[]).unwrap();
        assert_eq!(decrypted, original);
    }

    // ── XOR: header verification ─────────────────────────────────────────

    #[test]
    fn xor_header_algo_id() {
        let xform = XorPayloadTransform;
        let encrypted = xform.transform_outbound(vec![0x00, 0x01], &[]).unwrap();
        let algo_id = u16::from_be_bytes([encrypted[0], encrypted[1]]);
        assert_eq!(algo_id, XOR_ALGO_ID);
    }

    #[test]
    fn xor_header_payload_length() {
        let xform = XorPayloadTransform;
        let payload = vec![0x10, 0x20, 0x30, 0x40, 0x50]; // 5 bytes
        let encrypted = xform.transform_outbound(payload, &[]).unwrap();
        let recorded_len =
            u32::from_be_bytes([encrypted[2], encrypted[3], encrypted[4], encrypted[5]]) as usize;
        assert_eq!(recorded_len, 5);
    }

    #[test]
    fn xor_encrypted_data_differs_from_plaintext() {
        let xform = XorPayloadTransform;
        let original = vec![0x00, 0x00, 0x00, 0x00]; // all zeros
        let encrypted = xform.transform_outbound(original.clone(), &[]).unwrap();
        let body = &encrypted[XOR_HEADER_LEN..];
        // XOR(0x0000, 0xA5A5) = 0xA5A5, so body != plaintext
        assert_ne!(body, &original[..]);
        assert_eq!(body, &[0xA5, 0xA5, 0xA5, 0xA5]);
    }

    // ── XOR: error handling ─────────────────────────────────────────────

    #[test]
    fn xor_inbound_too_short() {
        let xform = XorPayloadTransform;
        let result = xform.transform_inbound(vec![0x00, 0x01], &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("too short"));
    }

    #[test]
    fn xor_inbound_wrong_algo() {
        let xform = XorPayloadTransform;
        // algo_id = 0x0099, payload_len = 0, 2-byte body (even length)
        let data = vec![0x00, 0x99, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let result = xform.transform_inbound(data, &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown algo"));
    }

    #[test]
    fn xor_inbound_odd_body_length() {
        let xform = XorPayloadTransform;
        // Valid header + 3-byte body (odd)
        let data = vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0xAB, 0xCD, 0xEF];
        let result = xform.transform_inbound(data, &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("odd length"));
    }

    // ── XOR: GenMessage integration ─────────────────────────────────────

    #[test]
    fn xor_outbound_msg() {
        let ctx = xor_context();
        let mut msg = GenMessage {
            payload: vec![0x01, 0x02, 0x03, 0x04],
            ..Default::default()
        };
        ctx.outbound(&mut msg, false).unwrap();
        // Encrypted: header (6) + body (4)
        assert_eq!(msg.payload.len(), XOR_HEADER_LEN + 4);
        // Verify algo_id in header
        let algo = u16::from_be_bytes([msg.payload[0], msg.payload[1]]);
        assert_eq!(algo, XOR_ALGO_ID);
    }

    #[test]
    fn xor_inbound_msg() {
        let ctx = xor_context();
        let original = vec![0xDE, 0xAD, 0xBE, 0xEF];

        // Simulate outbound (encrypt)
        let mut msg = GenMessage {
            payload: original.clone(),
            ..Default::default()
        };
        ctx.outbound(&mut msg, false).unwrap();

        // Simulate inbound (decrypt)
        ctx.inbound(&mut msg, false).unwrap();
        assert_eq!(msg.payload, original);
    }

    #[test]
    fn xor_msg_roundtrip_updates_header_length() {
        let ctx = xor_context();
        let mut msg = GenMessage::default();
        msg.header.stream_id = 42;
        msg.header.type_ = 1;
        msg.header.flags = 0x80;
        msg.payload = vec![0x11, 0x22, 0x33];

        // Original payload is 3 bytes, XOR adds 6-byte header + 1 byte padding = 10 bytes
        ctx.outbound(&mut msg, false).unwrap();
        assert_eq!(msg.header.length, 10); // Updated to transformed length
        assert_eq!(msg.header.stream_id, 42); // Other fields unchanged

        ctx.inbound(&mut msg, false).unwrap();
        // After roundtrip: payload restored, header.length reflects original
        assert_eq!(msg.header.length, 3); // Back to original payload length
        assert_eq!(msg.payload, vec![0x11, 0x22, 0x33]);
    }

    // ── Failing transform ───────────────────────────────────────────────

    #[derive(Debug)]
    struct FailTransform;

    impl PayloadTransform for FailTransform {
        fn transform_inbound(&self, _data: Vec<u8>, _aad: &[u8]) -> Result<Vec<u8>, String> {
            Err("inbound failed".into())
        }
        fn transform_outbound(&self, _data: Vec<u8>, _aad: &[u8]) -> Result<Vec<u8>, String> {
            Err("outbound failed".into())
        }
    }

    #[test]
    fn failing_transform_propagates_error() {
        let output = HookOutput {
            data: ConnectionData::new(),
            payload_transform: Some(Box::new(FailTransform)),
        };
        let ctx = ConnectionContext::new(Some(output));
        assert!(ctx.transform_inbound(vec![1], &[]).is_err());
        assert!(ctx.transform_outbound(vec![1], &[]).is_err());
    }

    #[test]
    fn failing_transform_pipeline_propagates_error() {
        let output = HookOutput {
            data: ConnectionData::new(),
            payload_transform: Some(Box::new(FailTransform)),
        };
        let ctx = ConnectionContext::new(Some(output));
        let mut msg = GenMessage {
            payload: vec![1, 2, 3],
            ..Default::default()
        };
        assert!(ctx.inbound(&mut msg, false).is_err());
        assert!(ctx.outbound(&mut msg, false).is_err());
    }

    // ── ConnectionContext Arc sharing ────────────────────────────────────

    #[test]
    fn context_data_is_arc_shared() {
        let mut data = ConnectionData::new();
        data.insert("key".into(), Box::new(99u64));
        let output = HookOutput {
            data,
            payload_transform: None,
        };
        let ctx = ConnectionContext::new(Some(output));
        // Clone the Arc — should point to same data
        let data2 = ctx.data.clone();
        assert_eq!(Arc::strong_count(&ctx.data), 2);
        assert_eq!(data2.get_typed::<u64>("key"), Some(&99u64));
    }

    #[test]
    fn context_transform_is_arc_shared() {
        let output = HookOutput {
            data: ConnectionData::new(),
            payload_transform: Some(Box::new(NoopTransform)),
        };
        let ctx = ConnectionContext::new(Some(output));
        let t1 = ctx.payload_transform.clone().unwrap();
        let t2 = ctx.payload_transform.clone().unwrap();
        // Both Arc clones point to the same transform
        assert!(Arc::ptr_eq(&t1, &t2));
    }

    // ── on_accept unit tests (async, requires Socket) ──────────────────

    #[cfg(feature = "async")]
    #[derive(Debug)]
    struct MockAcceptHook {
        should_reject: bool,
    }

    #[cfg(feature = "async")]
    impl AcceptHook for MockAcceptHook {
        fn on_accept(&self, _fd: RawFd) -> std::result::Result<HookOutput, HookError> {
            if self.should_reject {
                Err(HookError::Rejected("mock rejection".into()))
            } else {
                Ok(HookOutput {
                    data: ConnectionData::new(),
                    payload_transform: None,
                })
            }
        }
    }

    /// Helper: build a ServerExtensionConfig with an optional accept_hook.
    #[cfg(feature = "async")]
    fn cfg_with_hook(hook: Option<Arc<dyn AcceptHook>>) -> ServerExtensionConfig {
        ServerExtensionConfig { accept_hook: hook }
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn on_accept_no_hook_returns_ok_none() {
        let cfg = ServerExtensionConfig::default();
        // Create a real socket via UnixStream pair
        let (client_stream, _server_stream) = tokio::net::UnixStream::pair().unwrap();
        let socket = Socket::from(client_stream);
        let result = cfg.on_accept(&socket).await;
        assert!(result.is_ok(), "on_accept should not fail");
        assert!(result.unwrap().is_none(), "No hook → Ok(None)");
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn on_accept_socket_without_raw_fd_returns_err() {
        let hook: Arc<dyn AcceptHook> = Arc::new(MockAcceptHook {
            should_reject: false,
        });
        let cfg = cfg_with_hook(Some(hook));
        // Socket::new() does NOT capture raw_fd
        let (client_stream, _) = tokio::net::UnixStream::pair().unwrap();
        let socket = Socket::new(client_stream);
        assert_eq!(socket.as_raw_fd(), None);
        let err = match cfg.on_accept(&socket).await {
            Ok(_) => panic!("hook configured but no fd → should fail"),
            Err(e) => e,
        };
        let err_str = format!("{}", err);
        assert!(
            err_str.contains("accept hook requires a raw fd"),
            "error should tell caller to use Server::bind() or Socket::from: {}",
            err_str
        );
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn on_accept_hook_succeeds_returns_ok_some() {
        let hook: Arc<dyn AcceptHook> = Arc::new(MockAcceptHook {
            should_reject: false,
        });
        let cfg = cfg_with_hook(Some(hook));
        // Socket::from() DOES capture raw_fd
        let (client_stream, _) = tokio::net::UnixStream::pair().unwrap();
        let socket = Socket::from(client_stream);
        assert!(socket.as_raw_fd().is_some());
        let result = cfg.on_accept(&socket).await;
        assert!(result.is_ok());
        assert!(
            result.unwrap().is_some(),
            "Hook succeeded → Ok(Some(output))"
        );
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn on_accept_hook_rejected_returns_err() {
        let hook: Arc<dyn AcceptHook> = Arc::new(MockAcceptHook {
            should_reject: true,
        });
        let cfg = cfg_with_hook(Some(hook));
        let (client_stream, _) = tokio::net::UnixStream::pair().unwrap();
        let socket = Socket::from(client_stream);
        let result = cfg.on_accept(&socket).await;
        assert!(result.is_err(), "Hook rejected → Err(HookError)");
    }

    // ── XOR: additional edge cases ──────────────────────────────────────

    #[test]
    fn xor_roundtrip_all_zeros() {
        let ctx = xor_context();
        let original = vec![0x00; 16];
        let encrypted = ctx.transform_outbound(original.clone(), &[]).unwrap();
        // All zeros XOR 0xA5A5 → all 0xA5A5
        let body = &encrypted[XOR_HEADER_LEN..];
        assert!(body.iter().all(|&b| b == 0xA5));
        let decrypted = ctx.transform_inbound(encrypted, &[]).unwrap();
        assert_eq!(decrypted, original);
    }

    #[test]
    fn xor_roundtrip_all_ones() {
        let ctx = xor_context();
        let original = vec![0xFF; 20];
        let encrypted = ctx.transform_outbound(original.clone(), &[]).unwrap();
        let decrypted = ctx.transform_inbound(encrypted, &[]).unwrap();
        assert_eq!(decrypted, original);
    }

    #[test]
    fn xor_inbound_empty_header_exactly() {
        let xform = XorPayloadTransform;
        // Exactly XOR_HEADER_LEN bytes = valid header with 0-length body
        let data = vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x00];
        let result = xform.transform_inbound(data, &[]);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn xor_transform_inbound_idempotent_after_outbound() {
        // Encrypt, then decrypt twice — second decrypt should fail (header mismatch)
        let xform = XorPayloadTransform;
        let original = vec![0x01, 0x02, 0x03, 0x04];
        let encrypted = xform.transform_outbound(original.clone(), &[]).unwrap();
        let decrypted = xform.transform_inbound(encrypted.clone(), &[]).unwrap();
        assert_eq!(decrypted, original);
        // Decrypting already-decrypted data should fail (algo_id mismatch)
        let result = xform.transform_inbound(decrypted, &[]);
        assert!(result.is_err(), "Double-decrypt should fail");
    }
}
