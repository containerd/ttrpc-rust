//! Generate Prost messages and ttrpc service bindings from Protocol Buffers schemas.
//!
//! Use [`Codegen`] from a Cargo build script with `.prost()` to select this generator.
//! Install `protoc` before building and enable the `prost` backend in the ttrpc runtime,
//! with default features disabled. Generated messages require Prost 0.14.4.
//!
//! Bindings are synchronous by default. Set [`Customize::async_all`] for async clients,
//! servers, and streaming, and enable the runtime's `async` feature. Applications using
//! async bindings also need the `async-trait` dependency.
//!
//! Generated files follow protobuf package names and contain both messages and service
//! bindings. Set [`Customize::gen_mod`] to generate a `mod.rs` that can be included from
//! the application's `OUT_DIR`; the default include filename is `_include.rs`.
//!
//! See the [setup guide] for a complete build script and application configuration,
//! and the [examples] for working clients and servers.
//!
//! [setup guide]: https://github.com/containerd/ttrpc-rust/blob/master/codegen/README.md
//! [examples]: https://github.com/containerd/ttrpc-rust/tree/master/example-prost

mod codegen;
mod svcgen;
mod util;

pub use codegen::{Backend, Codegen, Customize};
pub use svcgen::AsyncMode;
