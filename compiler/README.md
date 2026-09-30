# ttrpc-compiler

Generate rust-protobuf ttrpc service bindings from Protocol Buffers descriptors.

## Usage

- For build-script generation, use [`ttrpc-codegen`](../ttrpc-codegen/README.md).
  The [quick start](../README.md#add-ttrpc-to-your-project) includes the runtime
  dependencies, proto definition, and build script.
- For manual generation, install the `ttrpc_rust_plugin` binary with
  `cargo install ttrpc-compiler --version 0.9.0 --locked` and configure `protoc`
  to use it as the `protoc-gen-ttrpc` plugin. This generates service bindings;
  generate the message types separately with `protobuf-codegen`.

## Well-known types

RPC inputs and outputs from canonical Google well-known proto dependencies reference the
corresponding types provided by the `protobuf` runtime. Well-known proto files explicitly selected
for generation continue to use their locally generated modules.

## Versions

Use these release pairs:

| ttrpc-compiler version | ttrpc version |
| ------------- | ------------- |
| 0.8.0 | 0.9.x |
| 0.9.x | 0.10.x |

Version 0.9 requires Rust 1.80 or newer. It removes the legacy `prost_codegen`
module and changes the generated APIs for well-known types. See the
[runtime changelog](../CHANGELOG.md) and
[compiler changelog](./CHANGELOG.md) before upgrading.
