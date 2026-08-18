# API to generate .rs files for ttrpc from protobuf

API to generate `.rs` files to be used e. g. [from build.rs](../example/build.rs).

## Example

build.rs:

```rust
use ttrpc_codegen::Codegen;
use ttrpc_codegen::{Customize, ProtobufCustomize};

fn main() {
    let protos = vec![
        "protos/a.proto",
        "protos/b.proto",
    ];

    Codegen::new()
        .out_dir("protocols/sync")
        .inputs(&protos)
        .include("protocols/protos")
        .rust_protobuf()
        .customize(Customize {
            ..Default::default()
        })
        .rust_protobuf_customize(ProtobufCustomize::default())
        .run()
        .expect("Gen code failed.");
}

```

## Well-known types

Canonical Google well-known type imports such as `google/protobuf/timestamp.proto` and
`google/protobuf/empty.proto` are resolved automatically. They do not need to be copied into the
source tree or added through an extra include directory.

When an imported well-known type is used as an RPC input or output, generated services reference
the type provided by the `protobuf` runtime. A well-known proto explicitly listed as an input keeps
using its locally generated module for compatibility. Proto definitions outside the standard
well-known type set, including Google API definitions, must still be available through an include
directory.

Cargo.toml:

```
[build-dependencies]
ttrpc-codegen = "0.2"
```

## Versions
| ttrpc-codegen version | ttrpc version |
| ------------- | ------------- |
| 0.1.x | <= 0.4.x  |
| 0.2.x | == 0.5.x  |
| 0.3.x | == 0.6.x  |
| 0.4.x | >= 0.7.x  |
| 0.5.x | >= 0.7.x  |

## Alternative
The alternative is to use
[protoc-rust crate](https://github.com/stepancheg/rust-protobuf),
which relies on `protoc` command to parse descriptors. Both crates should produce the same result,
otherwise please file a bug report.
