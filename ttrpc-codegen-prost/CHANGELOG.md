# Changelog

All notable changes to the `ttrpc-codegen-prost` crate are documented here.
The format is based on [Keep a Changelog].

## [Unreleased]

The first release is being prepared as `0.1.0`.

### Added

- Added a standalone Prost 0.13 generator with a fluent `Codegen` builder,
  an explicit `.prost()` backend selector, and `Customize` options. ([#286])
- Generate synchronous, asynchronous, client-streaming, server-streaming,
  and bidirectional ttrpc service bindings. ([#286])
- Support `OUT_DIR` as the default destination, module declarations through
  `mod.rs` or `_include.rs`, and optional Serde derives on generated messages.
  ([#286])
- Added standalone generator tests and sync, async, and streaming examples.
  Building generated bindings requires `protoc` and the runtime's `prost`
  feature with default features disabled. ([#286])

### Changed

- Renamed the unpublished Prost package from `ttrpc-codegen` version `1.0.0`
  to `ttrpc-codegen-prost` version `0.1.0`. Rust imports now use
  `ttrpc_codegen_prost`; the rust-protobuf `ttrpc-codegen` package retains
  its existing name and version line.

### Fixed

- Avoid writing the generated-file header multiple times when several proto
  descriptors share the same output package file. ([#286])

[Keep a Changelog]: https://keepachangelog.com/en/2.0.0/
[Unreleased]: https://github.com/containerd/ttrpc-rust/commits/master/ttrpc-codegen-prost
[#286]: https://github.com/containerd/ttrpc-rust/pull/286
