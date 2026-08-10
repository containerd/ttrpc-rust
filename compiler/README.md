# A compiler of ttrpc-rust

generate rust version ttrpc code from proto files.

## Usage

- [Manual Generation](https://github.com/containerd/ttrpc-rust#1-generate-with-protoc-command) uses ttrpc-compiler as a protoc plugin
- [Programmatic Generation](https://github.com/containerd/ttrpc-rust#2-generate-programmatically) uses ttrpc-compiler as a rust crate

## Well-known types

RPC inputs and outputs from canonical Google well-known proto dependencies reference the
corresponding types provided by the `protobuf` runtime. Well-known proto files explicitly selected
for generation continue to use their locally generated modules.

## Versions
| ttrpc-compiler version | ttrpc version |
| ------------- | ------------- |
| 0.3.x | <= 0.4.x |
| 0.4.x | == 0.5.x  |
| 0.5.x | == 0.6.x |
| 0.6.x | >= 0.7.x |
