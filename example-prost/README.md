# Prost examples

These examples use the local ttrpc runtime and the standalone Prost generator in
[`codegen/`](../codegen). They share the `.proto` definitions in
[`example/protocols/protos/`](../example/protocols/protos) with the rust-protobuf
examples. `build.rs` generates sync and async bindings when Cargo builds them.

Install `protoc` and make it available on `PATH`. These example programs use Unix
sockets and are skipped on Windows. They are a separate workspace, so run Cargo
with `--manifest-path` from the repository root.

## Run a server and client

Run the server command in one terminal and the matching client in another:

| Mode | Server | Client |
| --- | --- | --- |
| Sync unary | `cargo run --manifest-path example-prost/Cargo.toml --example server` | `cargo run --manifest-path example-prost/Cargo.toml --example client` |
| Async unary | `cargo run --manifest-path example-prost/Cargo.toml --example async-server` | `cargo run --manifest-path example-prost/Cargo.toml --example async-client` |
| Async streaming | `cargo run --manifest-path example-prost/Cargo.toml --example async-stream-server` | `cargo run --manifest-path example-prost/Cargo.toml --example async-stream-client` |

Stop the server before trying the next pair: all pairs use
`unix:///tmp/ttrpc-test`, which is also used by the rust-protobuf examples.
The unary clients verify expected responses and a deliberately short deadline;
a timeout in that case is expected. The streaming client checks unary,
client-streaming, server-streaming, and bidirectional calls. Client assertions
cause a nonzero exit if a check fails.

These programs currently use Unix sockets only; they do not implement the
`--tcp` option provided by the rust-protobuf examples.

## Configuration

The runtime dependency disables default features and explicitly enables `sync`,
`async`, and `prost`. Default features include `rustprotobuf`, which cannot be
enabled together with `prost`.

Generated message and service APIs follow protobuf package names. The checked-in
module glue re-exports the generated `grpc` and streaming packages under the
names used by the example sources. See the [generator guide](../codegen/README.md)
for adding Prost generation to your own application.

## Build and validate

From the repository root:

```bash
make -C example-prost build-examples
make -C example-prost check

# Starts the three server/client pairs and verifies client exit status.
# Stop any manually started example servers before running this command.
cargo test -p ttrpc --no-default-features --features sync,async,prost \
  --test run-examples -- --nocapture
```

`check` runs rustfmt and strict Clippy. Generated upstream protobuf names and
comments have narrowly scoped lint allowances; the handwritten example code
is checked normally.

To check the runtime's security extension tests with this backend on Unix:

```bash
cargo test -p ttrpc --no-default-features \
  --features sync,async,prost,security_extension \
  --test hook_integration_async_unix --test hook_integration_sync_unix
```

To build and open the corresponding runtime API documentation:

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --no-default-features \
  --features sync,async,prost,security_extension --open
```
