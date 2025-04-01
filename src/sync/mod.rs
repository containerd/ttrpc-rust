// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

//! Server and Client in sync mode.

mod channel;
mod client;
mod server;
mod sys;

#[macro_use]
pub mod utils;

pub use client::Client;
pub use server::Server;

#[doc(hidden)]
pub use utils::{MethodHandler, TtrpcContext};
