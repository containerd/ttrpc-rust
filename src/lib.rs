// Copyright (c) 2019 Ant Financial
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! A lightweight [ttrpc] client and server implementation for Rust.
//!
//! ttrpc combines Protocol Buffers service definitions with a compact framing protocol that does
//! not require HTTP/2. It is designed for resource-constrained system components such as
//! container runtime shims and sandbox agents. ttrpc and gRPC can share `.proto` definitions, but
//! they are not wire-compatible.
//!
//! ## Highlights
//!
//! - Synchronous and Tokio-based asynchronous clients and servers.
//! - Unary, client-streaming, server-streaming, and bidirectional RPCs.
//! - Pure-Rust client and server generation from `.proto` files.
//! - Unix domain socket, TCP, vsock, and Windows named-pipe transports.
//! - Per-request metadata, deadlines, and structured RPC status errors.
//!
//! # Getting started
//!
//! Most applications use this crate together with [`ttrpc-codegen`]:
//!
//! ```toml
//! [dependencies]
//! ttrpc = "0.9"
//!
//! [build-dependencies]
//! ttrpc-codegen = "0.6"
//! ```
//!
//! To use the Tokio runtime, disable the default synchronous runtime or enable both:
//!
//! ```toml
//! # Async only
//! ttrpc = { version = "0.9", default-features = false, features = ["async"] }
//!
//! # Sync and async
//! # ttrpc = { version = "0.9", features = ["async"] }
//! ```
//!
//! Generate message types and service bindings from `build.rs`:
//!
//! ```no_run
//! use ttrpc_codegen::{Codegen, Customize, ProtobufCustomize};
//!
//! fn main() -> std::io::Result<()> {
//!     Codegen::new()
//!         .input("proto/greeter.proto")
//!         .include("proto")
//!         .rust_protobuf()
//!         .customize(Customize {
//!             // Set to `true` for Tokio-based bindings.
//!             async_all: false,
//!             gen_mod: true,
//!             ..Default::default()
//!         })
//!         .rust_protobuf_customize(ProtobufCustomize::default().gen_mod_rs(true))
//!         .run()
//! }
//! ```
//!
//! Generated bindings provide a typed client, a service trait to implement, and a registration
//! helper for the server. See the repository's complete [client, server, and streaming examples]
//! for working programs.
//!
//! # Choosing a runtime
//!
//! | Runtime | Feature | Execution model | Streaming |
//! | --- | --- | --- | --- |
//! | `sync` | `sync` (default) | Blocking client calls and a server worker pool | Unary RPCs |
//! | `asynchronous` | `async` | Tokio tasks and asynchronous I/O | All RPC styles |
//!
//! Choose `sync` for blocking applications and existing thread-based services. Choose
//! `asynchronous` when the application already uses Tokio, needs many concurrent connections, or
//! uses streaming RPCs.
//!
//! # A tour of ttrpc
//!
//! ## Generated clients and services
//!
//! [`ttrpc-codegen`] reads `.proto` files during the Cargo build and generates Protocol Buffers
//! messages, typed clients, server traits, and service registration helpers. Applications usually
//! interact with these generated APIs instead of constructing low-level [`Request`] and
//! [`Response`] messages directly.
//!
//! ## Clients and servers
//!
//! The `sync` module contains the default thread-based `Client` and `Server`. The
//! `asynchronous` module contains the Tokio-based client, server, typed streaming handles, shutdown
//! coordination, and pluggable transports.
//!
//! ## Request context
//!
//! Every generated client call accepts a [`context::Context`]. It carries request metadata and a
//! relative timeout that the client converts into the wire deadline:
//!
//! ```
//! use std::time::Duration;
//! use ttrpc::context;
//!
//! let mut ctx = context::with_duration(Duration::from_millis(500));
//! ctx.add("request-id".into(), "7f3a".into());
//!
//! assert_eq!(ctx.timeout_nano, 500_000_000);
//! assert_eq!(ctx.metadata["request-id"], ["7f3a"]);
//! ```
//!
//! ## Errors and status
//!
//! Fallible operations return this crate's [`Result`] alias. [`Error::RpcStatus`] represents a
//! status returned by the remote service, while the other variants cover transport, protocol,
//! timeout, and local stream failures. [`Code`] and [`Status`] expose the Protocol Buffers status
//! model used on the wire.
//!
//! # Feature flags
//!
//! - `sync` (default): thread-based client and server.
//! - `async`: Tokio-based client and server, streaming RPCs, and asynchronous transports.
//! - `security_extension`: connection hooks and payload transforms for application-defined
//!   authentication, encryption, and other per-connection policies. Unix only.
//!
//! Documentation on docs.rs is built with both features enabled.
//!
//! # Transport addresses
//!
//! | Address | Transport | Platforms |
//! | --- | --- | --- |
//! | `unix:///run/service.sock` | Unix domain socket | Unix |
//! | `unix://@service` | Abstract Unix domain socket | Linux, Android |
//! | `tcp://127.0.0.1:5000` | TCP | Unix |
//! | `vsock://3:1024` | VM socket | Linux, Android |
//! | `\\.\pipe\service` | Named pipe | Windows |
//!
//! For macOS, ttrpc-rust **only** supports normal Unix domain socket:
//!
//! The async transport layer can also wrap application-defined byte streams and listeners. See the
//! crate's transport modules for the ownership and runtime requirements of raw descriptors.
//!
//! # Security and compatibility
//!
//! ttrpc does not provide TLS. Protect TCP traffic at the deployment or network layer when it
//! crosses an untrusted boundary. Do not connect a ttrpc endpoint directly to a gRPC endpoint;
//! their framing protocols are different even when their service definitions match.
//!
//! [client, server, and streaming examples]: https://github.com/containerd/ttrpc-rust/tree/master/example
//! [ttrpc]: https://github.com/containerd/ttrpc
//! [`ttrpc-codegen`]: https://docs.rs/ttrpc-codegen

#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

// The security_extension feature requires Unix platform.
#[cfg(all(not(unix), feature = "security_extension"))]
compile_error!("The 'security_extension' feature is only supported on Unix platforms.");

#[macro_use]
extern crate log;

#[macro_use]
pub mod error;
#[cfg(feature = "sync")]
#[macro_use]
mod common;

#[macro_use]
mod macros;

/// Per-connection hooks, metadata, and payload transformation support.
pub mod security_extension;

/// Request metadata and timeout configuration.
pub mod context;

/// Low-level ttrpc wire messages and Protocol Buffers codec support.
pub mod proto;
#[doc(inline)]
pub use self::proto::{Code, MessageHeader, Request, Response, Status};

#[doc(inline)]
pub use crate::error::{get_status, Error, Result};

// Core extension types are always available.
#[doc(inline)]
pub use crate::security_extension::{ConnectionData, ConnectionDataExt, PayloadTransform};

#[cfg(feature = "security_extension")]
#[doc(inline)]
pub use crate::security_extension::{
    AcceptHook, ConnectHook, ConnectionContext, HookError, HookOutput,
};

#[cfg(not(feature = "security_extension"))]
#[doc(hidden)]
pub use crate::security_extension::ConnectionContext;

cfg_sync! {
    pub mod sync;
    #[doc(hidden)]
    #[allow(deprecated)]
    pub use sync::response_to_channel;
    #[doc(inline)]
    pub use sync::{send_response, MethodHandler, TtrpcContext};
    pub use sync::Client;
    #[doc(inline)]
    pub use sync::Server;
}

cfg_async! {
    pub mod asynchronous;
    #[doc(hidden)]
    pub use asynchronous as r#async;
}
