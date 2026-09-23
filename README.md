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
| Code generation | Pure-Rust rust-protobuf generation, a `protoc` plugin, or Prost generation using `protoc` |
| Request context | Timeouts, metadata, and typed RPC status codes |
| Transports | Unix sockets, TCP, Linux/Android vsock, and Windows named pipes |
| Server lifecycle | Service registration, listener control, and graceful shutdown |
| Platforms | Linux, macOS, Windows, and Android |

The synchronous API and `rustprotobuf` backend are enabled by default. Enable the `async` Cargo feature for the Tokio implementation and streaming RPCs. See [Using Prost](#using-prost) for the alternative protobuf backend.

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

Canonical Google well-known type imports, such as `google/protobuf/timestamp.proto`, are available
automatically and do not require an additional include path.

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

## Using Prost

Prost support in this checkout uses `prost` 0.14.4 and requires `protoc` on `PATH`
for both the runtime build and application code generation. Use a local dependency
on the checkout to try the current implementation:

```toml
[dependencies]
prost = "0.14.4"
ttrpc = { path = "../ttrpc-rust", default-features = false, features = ["sync", "prost"] }

[build-dependencies]
ttrpc-codegen = { path = "../ttrpc-rust/codegen" }
```

Adjust the paths to your checkout. The Prost generator in `codegen/` is a separate
crate from the rust-protobuf generator in `ttrpc-codegen/`; select the appropriate
path. The two protobuf backend features are mutually exclusive. Because disabling
default features also disables `sync`, list the runtime features explicitly.

Use `.prost()` in `build.rs`. Set `Customize::async_all = true` for async bindings
and enable the runtime's `async` feature; generated async bindings also require
`async-trait` in your application. Streaming requires async bindings. The
[Prost generator guide](./codegen/README.md) includes a complete dependency setup,
service definition, build script, and generated-module import.

Generated Rust files and modules follow the protobuf package rather than the
input filename. For example, `package example;` produces `example.rs`, containing
both message types and service bindings. Rust identifier casing may also differ
from rust-protobuf, such as `Cpu` instead of `CPU`; use the generated APIs for your
selected backend. The protobuf schema and ttrpc wire protocol remain the same.

When upgrading from Prost 0.13, update the application's `prost` dependency to
0.14.4 and regenerate its bindings with the matching generator. Message types
using different Prost minor versions implement different `prost::Message` traits.

The [Prost examples](./example-prost/README.md) demonstrate synchronous, asynchronous,
and streaming calls over Unix sockets. Run a server and client in separate terminals:

```bash
cargo run --manifest-path example-prost/Cargo.toml --example server
cargo run --manifest-path example-prost/Cargo.toml --example client
```

On Unix, `security_extension` is available with either backend. Generate local
API documentation for Prost, both runtimes, and the security extension with:

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --no-default-features \
  --features sync,async,prost,security_extension --open
```

The docs.rs configuration selects `rustprotobuf`. The local command above lets
you inspect the Prost APIs and fails on documentation warnings. The generator's
own documentation is built separately:

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --manifest-path codegen/Cargo.toml --no-deps --open
```

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
| [`example`](https://github.com/containerd/ttrpc-rust/tree/master/example) | End-to-end unary and streaming examples using rust-protobuf |
| [Prost generator](./codegen) | Standalone build-script generator using Prost and `protoc` |
| [`example-prost`](./example-prost) | Standalone unary and streaming examples using Prost |

## Compatibility

- Runtime and code generators minimum supported Rust version: **1.85**
- Repository development toolchain: see [`rust-toolchain.toml`](https://github.com/containerd/ttrpc-rust/blob/master/rust-toolchain.toml)
- Default features: `sync`, `rustprotobuf`
- Optional features: `async`, `prost`, `security_extension` (Unix only)
- Enable exactly one of `rustprotobuf` and `prost`; never use `--all-features` for the runtime.
- Keep `protobuf`, `protobuf-codegen`, and generated sources on matching versions. Regenerate bindings after changing the Protocol Buffers runtime version.

## Development

```bash
# The "prost" and "rustprotobuf" features are mutually exclusive, so never
# build the root crate with --all-features; `make test` covers both backends.
make test

# Run formatting, Clippy, and strict API documentation checks
make check-all

# The Prost generator is a separate workspace
make -C codegen test
make -C codegen check
```

## Project details

`ttrpc-rust` is a **non-core** containerd subproject, licensed under the [Apache License 2.0](./LICENSE).

As a containerd subproject, you will find the:

- [Project governance](https://github.com/containerd/.project/blob/main/GOVERNANCE.md),
- [Maintainers](./MAINTAINERS),
- and [Contributing guidelines](https://github.com/containerd/.project/blob/main/CONTRIBUTING.md)

information in the [`containerd/.project`](https://github.com/containerd/.project) repository.
