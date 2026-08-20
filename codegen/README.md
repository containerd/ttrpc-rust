# Ttrpc-rust Codegen (Prost backend)

Rust code generation for ttrpc services using the
[Prost](https://crates.io/crates/prost) protobuf compiler.

## Getting started

`protoc` must be installed on the local environment. Then write the following
code into `build.rs`:

```rust
use ttrpc_codegen::{Codegen, Customize};

fn main() {
    let mut protos = vec![
        "../example/protocols/protos/health.proto",
        "../example/protocols/protos/agent.proto",
        "../example/protocols/protos/oci.proto",
    ];

    Codegen::new()
        .out_dir("protocols/sync")
        .inputs(&protos)
        .include("../example/protocols/protos")
        .prost()
        .customize(Customize::default())
        .run()
        .unwrap();
}
```

The fluent API matches the rust-protobuf based `ttrpc-codegen` crate; call
`.prost()` to select this backend and `.customize(...)` to configure service
generation:

- `async_all`: generate async code for both server and client
- `async_server`: generate async code for server
- `async_client`: generate async code for client
- `gen_mod`: emit module declarations in `mod.rs` instead of `_include.rs`
- `serde`: derive `serde::Serialize`/`serde::Deserialize` on messages

See `example-prost` in the repository for a complete example.
