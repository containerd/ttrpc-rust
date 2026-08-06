<div align="center">

# ttrpc-rust

**Lightweight RPC for Rust, built for memory-constrained systems.**

[![Crates.io](https://img.shields.io/crates/v/ttrpc.svg)](https://crates.io/crates/ttrpc)
[![Documentation](https://docs.rs/ttrpc/badge.svg)](https://docs.rs/ttrpc)
[![BVT](https://github.com/containerd/ttrpc-rust/actions/workflows/bvt.yml/badge.svg)](https://github.com/containerd/ttrpc-rust/actions/workflows/bvt.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/containerd/ttrpc-rust/blob/master/LICENSE)

[API documentation](https://docs.rs/ttrpc) · [Examples](https://github.com/containerd/ttrpc-rust/tree/master/example) · [ttrpc protocol](https://github.com/containerd/ttrpc/blob/main/PROTOCOL.md) · [Report an issue](https://github.com/containerd/ttrpc-rust/issues)

</div>

_ttrpc-rust is a **non-core** subproject of containerd._

It is the Rust implementation of [ttrpc](https://github.com/containerd/ttrpc): a simple RPC protocol designed for environments where memory usage and binary size matter. It uses Protocol Buffers service definitions while replacing the HTTP/2 stack with lightweight framing—making it a natural fit for container runtimes, sandboxed workloads, sidecars, and embedded system services.

> [!IMPORTANT]
> ttrpc reuses `.proto` service definitions, but it does **not** use the gRPC wire protocol. A ttrpc client must communicate with a ttrpc server.

## Features

| Capability | Support |
| --- | --- |
| Client and server APIs | Synchronous and Tokio-based asynchronous implementations |
| RPC styles | Unary; client, server, and bidirectional streaming in async mode |
| Code generation | Pure-Rust build-time generation or a `protoc` plugin |
| Request context | Timeouts, metadata, and typed RPC status codes |
| Transports | Unix sockets, TCP, Linux/Android vsock, and Windows named pipes |
| Server lifecycle | Service registration, listener control, and graceful shutdown |
| Platforms | Linux, macOS, Windows, and Android |

The synchronous API is enabled by default. Enable the `async` Cargo feature for the Tokio implementation and streaming RPCs.

## Quick start

### Run the examples

Clone the repository and start a synchronous server:

```bash
cargo run -p ttrpc-example --example server
```

In another terminal, run the client:

```bash
cargo run -p ttrpc-example --example client
```

Async and streaming examples are available with the same workflow:

```bash
# Unary async RPC
cargo run -p ttrpc-example --example async-server
cargo run -p ttrpc-example --example async-client

# Unary + client/server/bidirectional streaming
cargo run -p ttrpc-example --example async-stream-server
cargo run -p ttrpc-example --example async-stream-client
```

On Unix, append `-- --tcp` to any example command to use TCP instead of a Unix socket.

### Add ttrpc to your project

Add the runtime, Protocol Buffers support, and build-time generator:

```toml
[dependencies]
protobuf = "3.7"
ttrpc = "0.9"

[build-dependencies]
ttrpc-codegen = "0.6"
```

For async clients, servers, and streaming, use the following dependency set:

```toml
[dependencies]
async-trait = "0.1"
protobuf = "3.7"
ttrpc = { version = "0.9", features = ["async"] }
tokio = { version = "1", features = ["macros", "rt"] }

[build-dependencies]
ttrpc-codegen = "0.6"
```

Define a service in `proto/greeter.proto`:

```proto
syntax = "proto3";

package example;

message HelloRequest  { string name = 1; }
message HelloResponse { string message = 1; }

service Greeter {
  rpc SayHello(HelloRequest) returns (HelloResponse);
}
```

Generate the message types, client, and server trait from `build.rs`—no `protoc` installation is required:

```rust
use ttrpc_codegen::{Codegen, Customize, ProtobufCustomize};

fn main() {
    println!("cargo:rerun-if-changed=proto/greeter.proto");

    Codegen::new()
        .out_dir(std::env::var("OUT_DIR").unwrap())
        .input("proto/greeter.proto")
        .include("proto")
        .rust_protobuf()
        .customize(Customize {
            gen_mod: true,
            ..Default::default()
        })
        .rust_protobuf_customize(ProtobufCustomize::default().gen_mod_rs(true))
        .run()
        .expect("failed to generate ttrpc bindings");
}
```

Include the generated modules in your crate:

```rust
mod rpc {
    include!(concat!(env!("OUT_DIR"), "/mod.rs"));
}
```

The generator creates:

- `greeter.rs` — Protocol Buffers messages
- `greeter_ttrpc.rs` — the `Greeter` service trait, `GreeterClient`, and service registration helper
- `mod.rs` — generated module declarations

Implement the generated service trait, register it with `ttrpc::Server`, and connect with `ttrpc::Client`:

```rust
// Server
let service = rpc::greeter_ttrpc::create_greeter(Arc::new(GreeterService));
let mut server = ttrpc::Server::new()
    .bind("unix:///tmp/greeter.sock")?
    .register_service(service);
server.start()?;

// Client
let channel = ttrpc::Client::connect("unix:///tmp/greeter.sock")?;
let client = rpc::greeter_ttrpc::GreeterClient::new(channel);
let response = client.say_hello(Default::default(), &request)?;
```

See the complete [synchronous](https://github.com/containerd/ttrpc-rust/blob/master/example/server.rs) and [asynchronous](https://github.com/containerd/ttrpc-rust/blob/master/example/async-server.rs) servers, plus the [streaming example](https://github.com/containerd/ttrpc-rust/blob/master/example/async-stream-server.rs), for production-shaped implementations.

### Generate async bindings

Set `async_all` during code generation:

```rust
.customize(Customize {
    async_all: true,
    gen_mod: true,
    ..Default::default()
})
```

You can generate only one side with `async_client` or `async_server`. Streaming services require async bindings.

## Transport addresses

| Address | Transport | Platforms |
| --- | --- | --- |
| `unix:///run/service.sock` | Unix domain socket | Unix |
| `unix://@service` | Abstract Unix domain socket | Linux, Android |
| `tcp://127.0.0.1:5000` | TCP | Unix |
| `vsock://3:1024` | VM socket | Linux, Android |
| `\\.\pipe\service` | Windows named pipe | Windows |

ttrpc does not provide TLS. If you expose TCP beyond a trusted boundary, secure the transport at the deployment or network layer.

## Workspace

| Crate | Purpose |
| --- | --- |
| [`ttrpc`](https://crates.io/crates/ttrpc) | Sync and async client/server runtime |
| [`ttrpc-codegen`](https://crates.io/crates/ttrpc-codegen) | Build-script API for parsing `.proto` files and generating Rust code |
| [`ttrpc-compiler`](https://crates.io/crates/ttrpc-compiler) | Service code generator and `protoc` plugin |
| [`example`](https://github.com/containerd/ttrpc-rust/tree/master/example) | End-to-end unary and streaming examples |

## Compatibility

- `ttrpc` runtime minimum supported Rust version: **1.70**
- Repository development toolchain: see [`rust-toolchain.toml`](https://github.com/containerd/ttrpc-rust/blob/master/rust-toolchain.toml)
- Default feature: `sync`
- Optional feature: `async`
- Keep `protobuf`, `protobuf-codegen`, and generated sources on matching versions. Regenerate bindings after changing the Protocol Buffers runtime version.

## Development

```bash
# Build and test every crate with all features
cargo test --workspace --all-features

# Run formatting and Clippy checks
make check-all
```

## Project details

`ttrpc-rust` is a non-core [containerd](https://containerd.io/) subproject. Governance and contribution guidelines are maintained in the [containerd project repository](https://github.com/containerd/project); repository maintainers are listed in [MAINTAINERS](https://github.com/containerd/ttrpc-rust/blob/master/MAINTAINERS).

Licensed under the [Apache License 2.0](https://github.com/containerd/ttrpc-rust/blob/master/LICENSE).
