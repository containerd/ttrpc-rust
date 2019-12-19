# ttrpc-rust

_ttrpc-rust is a **non-core** subproject of containerd_

`ttrpc-rust` is the Rust version of [ttrpc](https://github.com/containerd/ttrpc). [ttrpc](https://github.com/containerd/ttrpc) is GRPC for low-memory environments.

The ttrpc compiler of `ttrpc-rust` `ttrpc_rust_plugin` is modified from gRPC compiler of [gRPC-rs](https://github.com/pingcap/grpc-rs) [grpcio-compiler](https://github.com/pingcap/grpc-rs/tree/master/compiler).

## Usage

To generate the sources from proto files:

1. Install protoc from github.com/protocolbuffers/protobuf

2. Install protobuf-codegen from github.com/pingcap/grpc-rs

3. Install ttrpc_rust_plugin from ttrpc-rust/compiler

4. Generate the sources:

```
$ protoc --rust_out=. --ttrpc_out=. --plugin=protoc-gen-ttrpc=`which ttrpc_rust_plugin` example.proto
```
