// Copyright 2017 PingCAP, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// See the License for the specific language governing permissions and
// limitations under the License.

//! Low-level service generator for ttrpc Rust bindings.
//!
//! This crate turns Protocol Buffers file descriptors into generated ttrpc clients, server traits,
//! and registration helpers. Most projects should use [`ttrpc-codegen`] from `build.rs`; it parses
//! `.proto` files and calls this compiler internally. The `ttrpc_rust_plugin` binary exposes the
//! same generator through the standard `protoc` plugin protocol.
//!
//! [`ttrpc-codegen`]: https://docs.rs/ttrpc-codegen

#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

/// Generates ttrpc service bindings from Protocol Buffers descriptors.
pub mod codegen;
/// Legacy Prost-based code generation helpers.
pub mod prost_codegen;
mod util;

/// Customize generated code.
#[derive(Default, Debug, Clone)]
pub struct Customize {
    /// Indicates whether to generate async code for both server and client.
    pub async_all: bool,
    /// Indicates whether to generate async code for the client.
    pub async_client: bool,
    /// Indicates whether to generate async code for server.
    pub async_server: bool,
    /// Generates or updates `mod.rs` in the output directory.
    pub gen_mod: bool,
}
