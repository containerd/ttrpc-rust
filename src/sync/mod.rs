// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

//! Thread-based ttrpc clients and servers.
//!
//! The synchronous runtime is enabled by the default `sync` feature. Generated client methods
//! block the calling thread until a response arrives or the request timeout expires. A [`Server`]
//! accepts connections and dispatches generated service handlers on a configurable worker pool.
//!
//! # Examples
//!
//! ```no_run
//! use ttrpc::sync::Client;
//!
//! # fn main() -> ttrpc::Result<()> {
//! let client = Client::connect("unix:///run/my-service.sock")?;
//! // Pass `client` to a generated service client.
//! # drop(client);
//! # Ok(())
//! # }
//! ```

mod channel;
mod client;
mod server;
mod sys;

#[macro_use]
mod utils;

pub use client::Client;
pub use server::Server;

#[doc(hidden)]
#[allow(deprecated)]
pub use utils::response_to_channel;
pub use utils::{send_response, MethodHandler, TtrpcContext};
