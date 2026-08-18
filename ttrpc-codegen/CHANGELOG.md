# Changelog

All notable changes to the `ttrpc-codegen` crate are documented in this file.

The format is based on [Keep a Changelog], and this project follows
[Semantic Versioning]. Historical entries were reconstructed from the
published crates and Git history. Release dates are crates.io publication
dates. Releases are ordered by publication date because multiple version
lines were maintained in parallel.

## [0.6.0] - 2025-07-15

### API changes

- **Changed:** Generated bindings target the newer Protocol Buffers APIs;
  regenerate checked-in bindings when upgrading.
- The `Codegen` builder interface remains compatible with the 0.5 release
  line.

### Changed

- Updated Protocol Buffers dependencies and adapted to their newer APIs.
- Moved dependency management to the Cargo workspace.

## [0.5.0] - 2025-01-15

### API changes

- **Changed:** `Codegen::run` uses Cargo's `OUT_DIR` when `out_dir` was not
  configured explicitly.
- **Migration:** Build scripts that require another location should continue
  to call `Codegen::out_dir`.

### Added

- Used Cargo's `OUT_DIR` when no explicit output directory is configured.

### Changed

- Updated `protobuf-codegen` and Rust 1.81 Clippy compatibility.

## [0.2.4] - 2023-06-19

### Changed

- Updated `ttrpc-compiler` to 0.4.4 so generated async clients can use a
  shared `&Client`.

## [0.4.2] - 2023-04-14

### Added

- Added `rust-protobuf` element customization callbacks.

### Changed

- Updated Protocol Buffers dependencies to 3.2.

## [0.2.3] - 2023-02-27

### Fixed

- Fixed Clippy warnings in the 0.2 compatibility release line.

## [0.3.2] - 2022-10-14

### Changed

- Updated the pure Protocol Buffers generator dependency.
- Updated generated code for current Clippy checks.

## [0.2.2] - 2022-10-14

### Changed

- Backported Protocol Buffers generator and Clippy compatibility updates.

## [0.3.1] - 2022-09-15

### Changed

- Replaced the former pure codegen dependency with the optional pure mode in
  `protobuf-codegen`.

## [0.2.1] - 2022-09-15

### Changed

- Backported optional Protocol Buffers code generation support.

## [0.4.1] - 2022-09-14

### Changed

- Updated generated bindings from Protocol Buffers 2 to Protocol Buffers 3.

## [0.4.0] - 2022-08-12

> This release was yanked from crates.io.

### API changes

- **Added:** Generated clients and services support client-streaming,
  server-streaming, and bidirectional RPC methods.
- **Migration:** Regenerate bindings to expose streaming methods; existing
  unary service definitions retain their unary interfaces.

### Added

- Added generation for client-streaming, server-streaming, and
  bidirectional RPC services through the updated compiler.

## [0.3.0] - 2021-12-22

### API changes

- **Changed:** Generated async clients send requests through a shared
  `&Client` instead of requiring mutable access.
- The `Codegen` builder interface remains compatible with the 0.2 release
  line.

### Changed

- Updated the compiler so generated async clients can send through a shared
  `&Client`.
- Corrected and tested build-script documentation.

## [0.2.0] - 2021-02-24

### API changes

- **Breaking:** Generated client and server method signatures now carry
  request metadata and timeout contexts.
- **Migration:** Regenerate bindings and update service implementations and
  client calls to pass the new context values.

### Added

- Added metadata and timeout context generation for service methods.

### Changed

- Updated the compiler dependency to 0.4.

## [0.1.2] - 2020-09-03

### Fixed

- Preserved `rust-protobuf` customization when generating message code.

## [0.1.1] - 2020-07-20

### API changes

- Initial public `Codegen` builder for build-script-driven ttrpc binding
  generation.

### Added

- Initial pure-Rust `Codegen` builder for generating ttrpc bindings from
  `.proto` files.

[Keep a Changelog]: https://keepachangelog.com/en/2.0.0/
[Semantic Versioning]: https://semver.org/spec/v2.0.0.html
[0.6.0]: https://github.com/containerd/ttrpc-rust/compare/1d4cdeaf...f31f5925
[0.5.0]: https://github.com/containerd/ttrpc-rust/compare/22cd9ca4...1d4cdeaf
[0.2.4]: https://github.com/containerd/ttrpc-rust/compare/4b90ee15...8968bfad
[0.4.2]: https://github.com/containerd/ttrpc-rust/compare/5d1d5dcd...22cd9ca4
[0.2.3]: https://github.com/containerd/ttrpc-rust/compare/6b787f9d...4b90ee15
[0.3.2]: https://github.com/containerd/ttrpc-rust/compare/5a18aa1d...a42b31cb
[0.2.2]: https://github.com/containerd/ttrpc-rust/compare/bd4afad1...6b787f9d
[0.3.1]: https://github.com/containerd/ttrpc-rust/compare/8ab2d30d...5a18aa1d
[0.2.1]: https://github.com/containerd/ttrpc-rust/compare/eef20041...bd4afad1
[0.4.1]: https://github.com/containerd/ttrpc-rust/compare/fbe8cb95...5d1d5dcd
[0.4.0]: https://github.com/containerd/ttrpc-rust/compare/8ab2d30d...fbe8cb95
[0.3.0]: https://github.com/containerd/ttrpc-rust/compare/eef20041...8ab2d30d
[0.2.0]: https://github.com/containerd/ttrpc-rust/compare/9ea607a6...eef20041
[0.1.2]: https://github.com/containerd/ttrpc-rust/compare/ec2a9193...9ea607a6
[0.1.1]: https://github.com/containerd/ttrpc-rust/commits/ec2a9193
