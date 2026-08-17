// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

//! Tokio-based ttrpc clients, servers, and streaming RPCs.
//!
//! This module is available with the `async` feature and is also exported as `ttrpc::r#async` for
//! compatibility. Generated async bindings use [`Client`], [`Server`], and the typed stream
//! wrappers re-exported here.
//!
//! # Runtime
//!
//! Clients and servers spawn background tasks and must be created from within a Tokio runtime.
//! Dropping a [`Client`] closes it after all clones and active stream guards have been dropped.
//! Use [`Server::shutdown`] for an orderly server shutdown.

mod client;
mod server;
mod stream;
#[macro_use]
#[doc(hidden)]
mod utils;
mod connection;
/// Cooperative shutdown notification used by the async server.
pub mod shutdown;
/// Pluggable asynchronous listeners and byte streams.
pub mod transport;

pub use self::stream::{
    CSReceiver, CSSender, ClientStream, ClientStreamReceiver, ClientStreamSender, Kind, SSReceiver,
    SSSender, ServerStream, ServerStreamReceiver, ServerStreamSender, StreamInner, StreamReceiver,
    StreamSender,
};
pub(crate) use self::stream::{MessageControl, SendingMessage};
pub(crate) use connection::request_timeout_error;
#[doc(inline)]
pub use crate::r#async::client::Client;
#[doc(inline)]
pub use crate::r#async::server::{Server, Service};
#[doc(inline)]
pub use utils::{MethodHandler, StreamHandler, TtrpcContext};
