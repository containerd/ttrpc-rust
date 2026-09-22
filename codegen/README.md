# ttrpc code generation with Prost

This standalone crate generates Prost messages and ttrpc clients, server traits,
and service registration helpers from `.proto` files. It requires `protoc` on
`PATH` and uses Prost 0.13. The runtime must also use the `prost` backend.

## Build a service

The following example assumes your application and a checkout of this repository
are sibling directories:

```text
parent/
  ttrpc-rust/
  prost-greeter/
    Cargo.toml
    build.rs
    proto/greeter.proto
    src/lib.rs
```

In the application's `Cargo.toml`:

```toml
[package]
name = "prost-greeter"
version = "0.1.0"
edition = "2021"

[dependencies]
prost = "0.13"
ttrpc = { path = "../ttrpc-rust", default-features = false, features = ["sync", "prost"] }

[build-dependencies]
ttrpc-codegen = { path = "../ttrpc-rust/codegen" }
```

The `codegen/` directory contains the Prost generator; `ttrpc-codegen/` contains
the rust-protobuf generator. Their package names currently match, so use the
explicit path above to select this generator from the checkout.

Define `proto/greeter.proto`:

```proto
syntax = "proto3";
package example;

message HelloRequest { string name = 1; }
message HelloResponse { string message = 1; }

service Greeter {
  rpc SayHello(HelloRequest) returns (HelloResponse);
}
```

Generate the bindings in `build.rs`:

```rust
use ttrpc_codegen::{Codegen, Customize};

fn main() {
    println!("cargo:rerun-if-changed=proto/greeter.proto");

    Codegen::new()
        .out_dir(std::env::var("OUT_DIR").unwrap())
        .input("proto/greeter.proto")
        .include("proto")
        .prost()
        .customize(Customize {
            gen_mod: true,
            ..Default::default()
        })
        .run()
        .expect("failed to generate Prost bindings");
}
```

Include the generated modules in `src/lib.rs`:

```rust
pub mod rpc {
    include!(concat!(env!("OUT_DIR"), "/mod.rs"));
}
```

Run `cargo check` in the application directory. Because the schema declares
`package example;`, generation produces `example.rs` and a `mod.rs` that declares
`rpc::example`. The message types `HelloRequest` and `HelloResponse`, the `Greeter`
trait, `GreeterClient`, and `create_greeter` registration helper are all accessible
through that module. There is no separate `greeter_ttrpc.rs` file.

Use the generated client with `ttrpc::Client`, implement the service trait, and
register it with `ttrpc::Server`. See the [synchronous server](../example-prost/server.rs)
and [client](../example-prost/client.rs) for complete applications.

## Async and streaming

Replace the application's runtime dependencies with:

```toml
[dependencies]
async-trait = "0.1"
prost = "0.13"
ttrpc = { path = "../ttrpc-rust", default-features = false, features = ["async", "prost"] }
tokio = { version = "1", features = ["macros", "rt"] }
```

Keep the same build dependency and set `async_all: true` in `Customize`:

```rust
.customize(Customize {
    async_all: true,
    gen_mod: true,
    ..Default::default()
})
```

Use `ttrpc::asynchronous::Client` and `Server`, and await their operations.
Streaming services require async bindings. The [Prost examples](../example-prost/README.md)
cover unary, client-streaming, server-streaming, and bidirectional calls.

## Generator options

| Option | Behavior |
| --- | --- |
| `async_all` | Generate async clients and servers |
| `async_server` | Generate async servers |
| `async_client` | Generate async clients |
| `gen_mod` | Write module declarations to `mod.rs`; otherwise use `_include.rs` |
| `serde` | Derive `serde::Serialize` and `serde::Deserialize` on messages; add `serde` with its `derive` feature to the application |

`.inputs(...)` and `.includes(...)` accept multiple schema files and include
paths. When `.out_dir(...)` is omitted, the generator uses `OUT_DIR`, falling back
to the current directory outside a Cargo build script. Always call `.prost()`
before `.run()`.

## Checks and API documentation

This crate has its own workspace. Run its checks explicitly from the repository
root:

```bash
make -C codegen test
make -C codegen check
```

`check` runs formatting, strict Clippy, and strict API documentation generation.
To open the generator documentation directly:

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --manifest-path codegen/Cargo.toml --no-deps --open
```

The runtime's Prost and security extension documentation is a separate build
(on Unix):

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --no-default-features \
  --features sync,async,prost,security_extension --open
```
