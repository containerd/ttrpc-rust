# Changelog

All notable changes to the `ttrpc-compiler` crate are documented in this
file.

The format is based on [Keep a Changelog], and this project follows
[Semantic Versioning]. Historical entries were reconstructed from the
published crates and Git history. Release dates are crates.io publication
dates. Releases are ordered by publication date because multiple version
lines were maintained in parallel.

## [0.8.0] - 2025-07-15

### API changes

- **Changed:** Generated bindings target the newer Protocol Buffers APIs;
  regenerate checked-in bindings when upgrading.
- The public `Customize` options remain compatible with the 0.7 release
  line.

### Changed

- Updated generated code for the newer Protocol Buffers APIs.
- Reorganized the generator into more focused internal modules.
- Moved the repository to a Cargo workspace.

### Fixed

- Avoided unnecessary mutable bindings and unused-method warnings in
  generated server registration code.

## [0.7.0] - 2025-01-15

### API changes

- **Added:** `Customize::gen_mod` can generate a `mod.rs` for the emitted
  modules.
- **Breaking:** Constructing `Customize` with a struct literal must now set
  `gen_mod` or use `..Default::default()`.

### Added

- Added generated `mod.rs` support for more convenient module inclusion.

### Changed

- Updated generated code for Rust 1.81 Clippy compatibility.

## [0.6.3] - 2024-09-27

> This release was yanked from crates.io.

### Changed

- Simplified generated shared handlers from `Arc<Box<T>>` to `Arc<T>`.

## [0.6.2] - 2023-09-15

### Changed

- Added Clippy allowances to generated files so downstream lint runs do not
  report generator-owned warnings.

## [0.4.4] - 2023-06-19

### Changed

- Generated async clients now send requests through a shared `&Client`.

## [0.4.3] - 2023-02-27

### Changed

- Updated the Protocol Buffers 2.x dependency line.

## [0.5.1] - 2022-09-15

### Added

- Backported proto3 optional field support to the 0.5 release line.

## [0.4.2] - 2022-09-15

### Added

- Backported proto3 optional field support to the 0.4 release line.

## [0.6.1] - 2022-09-14

### Added

- Declared proto3 optional field support through the `protoc` plugin
  protocol.

## [0.6.0] - 2022-08-12

> This release was yanked from crates.io.

### API changes

- **Added:** Generated service traits and clients support client-streaming,
  server-streaming, and bidirectional RPC methods.
- **Migration:** Regenerate bindings to expose streaming methods; existing
  unary service definitions retain their unary interfaces.

### Added

- Added generation for client-streaming, server-streaming, and
  bidirectional RPC bindings.

## [0.5.0] - 2021-12-22

### API changes

- **Changed:** Generated async client methods accept a shared `&Client`
  instead of requiring mutable access.
- The compiler's hand-written `Customize` API remains compatible with 0.4.

### Changed

- Generated async clients now allow requests through a shared `&Client`.

## [0.4.1] - 2021-11-26

### Added

- Added macOS-compatible generated bindings.

### Changed

- Updated Prost code generation dependencies.

## [0.4.0] - 2021-02-24

### API changes

- **Breaking:** Generated client and server method signatures now carry
  request metadata and timeout contexts.
- **Migration:** Regenerate bindings and update service implementations and
  client calls to pass the new context values.

### Added

- Added metadata and timeout contexts to generated client and server method
  signatures.

## [0.3.2] - 2020-08-17

### Changed

- Improved crate documentation and crates.io metadata.

## [0.3.1] - 2020-07-17

### Added

- Added async client and server binding generation.
- Added programmatic generation without invoking `protoc`.

### Changed

- Added repository and homepage metadata.

## [0.3.0] - 2020-07-17

### API changes

- Initial public compiler API, `Customize` options, synchronous service
  generation, and `protoc` plugin interface.

### Added

- Initial standalone ttrpc compiler crate and `protoc` plugin.
- Added synchronous client and server binding generation.

[Keep a Changelog]: https://keepachangelog.com/en/2.0.0/
[Semantic Versioning]: https://semver.org/spec/v2.0.0.html
[0.8.0]: https://github.com/containerd/ttrpc-rust/compare/1d4cdeaf...f31f5925
[0.7.0]: https://github.com/containerd/ttrpc-rust/compare/b9e9dd8a...1d4cdeaf
[0.6.3]: https://github.com/containerd/ttrpc-rust/compare/6fe7d395...b9e9dd8a
[0.6.2]: https://github.com/containerd/ttrpc-rust/compare/8c275840...6fe7d395
[0.4.4]: https://github.com/containerd/ttrpc-rust/compare/4acc2d06...e2115960
[0.4.3]: https://github.com/containerd/ttrpc-rust/compare/bd4afad1...4acc2d06
[0.5.1]: https://github.com/containerd/ttrpc-rust/compare/8ab2d30d...5a18aa1d
[0.4.2]: https://github.com/containerd/ttrpc-rust/compare/790638a3...bd4afad1
[0.6.1]: https://github.com/containerd/ttrpc-rust/compare/fbe8cb95...8c275840
[0.6.0]: https://github.com/containerd/ttrpc-rust/compare/8ab2d30d...fbe8cb95
[0.5.0]: https://github.com/containerd/ttrpc-rust/compare/790638a3...8ab2d30d
[0.4.1]: https://github.com/containerd/ttrpc-rust/compare/e14aa825...790638a3
[0.4.0]: https://github.com/containerd/ttrpc-rust/compare/4bebaa0f...e14aa825
[0.3.2]: https://github.com/containerd/ttrpc-rust/compare/7e3634e0...4bebaa0f
[0.3.1]: https://github.com/containerd/ttrpc-rust/commits/7e3634e0
[0.3.0]: https://crates.io/crates/ttrpc-compiler/0.3.0
