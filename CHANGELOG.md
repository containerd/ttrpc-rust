# Changelog

All notable changes to the `ttrpc` crate are documented in this file.

The format is based on [Keep a Changelog], and this project follows
[Semantic Versioning]. Historical entries were reconstructed from the
published crates and Git history. Release dates are crates.io publication
dates. Because several release lines were maintained in parallel, releases
are ordered by publication date rather than version number.

## [0.9.0] - 2025-07-15

### API changes

- **Breaking:** `asynchronous::Client::connect` is now asynchronous and must
  be awaited.
- **Breaking:** Async clients and servers now use the public
  `transport::Socket` and `transport::Listener` abstractions. Raw descriptor
  constructors are transport-specific and explicitly `unsafe`.
- **Breaking:** Direct users of `ClientStreamReceiver::new` must pass a
  cloned `Client` to keep the underlying connection alive.
- **Added:** Async transport constructors now cover Unix sockets, TCP,
  vsock, and Windows named pipes.
- **Migration:** Regenerate service bindings with a compatible compiler and
  code generator after upgrading the Protocol Buffers dependencies.

### Added

- Added TCP transport support and TCP client and server examples.
- Added async Windows named-pipe support.
- Converted the repository into a Cargo workspace.

### Changed

- Updated the Protocol Buffers dependencies and related generated code.

### Fixed

- Fixed async server and streaming client lifetime handling.
- Fixed client creation when the process reaches its file descriptor limit.
- Fixed a synchronous server connection file descriptor leak.

## [0.8.6] - 2025-06-24

### Fixed

- Fixed a synchronous server connection file descriptor leak.

## [0.5.10] - 2025-06-24

### Fixed

- Backported the synchronous server connection file descriptor leak fix.

## [0.8.5] - 2025-05-09

### Fixed

- Fixed async server shutdown when the server object is dropped.
- Fixed a panic when creating a client after reaching the file descriptor
  limit.
- Kept streaming client connections alive until all stream handles are
  released.

### Changed

- Added end-to-end coverage for all examples.

## [0.8.4] - 2025-01-22

### Fixed

- Avoided applying unsupported socket options to Unix and vsock sockets.

## [0.8.3] - 2025-01-17

### Added

- Added vsock CID parsing and generated `mod.rs` support.
- Added a server-streaming example.

### Changed

- Improved request context ergonomics and async request failure handling.

### Fixed

- Fixed the synchronous server accept loop after errors.
- Avoided panics caused by poisoned internal locks.
- Fixed build failures and removed unused code.

## [0.5.9] - 2025-01-14

### Fixed

- Backported the fix for infinite synchronous server accept loops.

## [0.8.2] - 2024-09-27

### Added

- Added delivery acknowledgements for streamed messages.
- Declared the minimum supported Rust version.

### Fixed

- Sent pending responses before shutdown and fixed streaming timing races.
- Fixed hangs while waiting for connections to exit.
- Ensured connection file descriptors are released during shutdown.
- Retried interrupted operating-system calls and kept accepting after
  recoverable errors.

## [0.5.8] - 2024-09-25

### Fixed

- Backported the fix that keeps the server running after an accept error.

## [0.7.2] - 2024-09-25

### Added

- Added Windows synchronous client and server support and Android support.
- Declared the minimum supported Rust version.

### Changed

- Updated socket handling to the newer `nix` APIs.

### Fixed

- Kept the server running after recoverable accept errors.
- Fixed Windows connection cleanup and server shutdown behavior.

## [0.5.7] - 2023-10-24

### Fixed

- Ignored harmless errors while closing synchronous server descriptors.

## [0.8.1] - 2023-08-29

### Added

- Exposed the stream handle types used by generated streaming APIs.
- Added request cancellation state to synchronous server contexts.

### Fixed

- Enforced frame size limits in synchronous and asynchronous transports.
- Fixed missing default data in streams.
- Closed listener descriptors during async server shutdown.

## [0.5.6] - 2023-07-19

### Changed

- Reverted a source-level Clippy adjustment for compatibility.

## [0.5.5] - 2023-07-19

### Fixed

- Rejected oversized packets in synchronous and asynchronous transports.
- Refactored response delivery to apply frame validation consistently.

## [0.8.0] - 2023-06-19

### API changes

- **Breaking:** `sync::Client::new` now returns `Result<Client>` so connection
  failures can be handled by callers.
- **Breaking:** Low-level synchronous transport APIs use `PipeConnection`
  instead of accepting raw file descriptors directly.
- **Added:** Cross-platform `PipeConnection` and `PipeListener` abstractions
  support Unix sockets and Windows named pipes.

### Added

- Added synchronous Windows named-pipe support and Android support.
- Added APIs for constructing clients from existing connections.
- Added integration tests for the synchronous examples.

### Changed

- Updated socket handling for the newer `nix` APIs.
- Changed client construction to report connection errors with `Result`.

### Fixed

- Fixed connection cleanup on Windows.
- Fixed a server thread that could remain blocked during shutdown.

## [0.5.4] - 2023-06-19

### Changed

- Allowed asynchronous requests to be sent through a shared `&Client`.
- Updated the Protocol Buffers 2.x dependency line.

## [0.7.1] - 2022-09-14

### Changed

- Updated Protocol Buffers support from version 2 to version 3.
- Updated generated examples and Rust 1.63 compatibility.

## [0.7.0] - 2022-08-12

> This release was yanked from crates.io.

### API changes

- **Added:** Generated services can use client-streaming, server-streaming,
  and bidirectional RPC methods.
- **Added:** Public stream handles, stream handlers, codecs, and cooperative
  shutdown primitives support the new generated interfaces.
- **Migration:** Regenerate service bindings to use streaming RPCs; existing
  unary services can continue to use the unary interfaces.

### Added

- Added client-streaming, server-streaming, and bidirectional RPC support.
- Added cooperative async server shutdown and streaming examples.

### Changed

- Reworked async connection handling, framing, and message codecs.
- Removed the unused `bytes` dependency.

## [0.6.1] - 2022-05-11

### Added

- Added timeout support to the asynchronous client.

### Fixed

- Improved frame header validation and async socket error handling.

## [0.5.3] - 2022-05-11

### Added

- Backported asynchronous client timeout support.

### Fixed

- Corrected the descriptor stored in asynchronous server contexts.

## [0.6.0] - 2022-01-18

### API changes

- **Breaking:** The protocol module moved from `ttrpc::ttrpc` to
  `ttrpc::proto`.
- **Breaking:** Internal runtime helper modules are no longer public; use the
  documented top-level and runtime-specific re-exports.
- **Changed:** Async requests can now be sent through a shared `&Client`
  instead of requiring `&mut Client`.

### Added

- Added normal filesystem Unix sockets on Linux.
- Allowed asynchronous requests to be sent through a shared `&Client`.

### Changed

- Hid runtime implementation details that did not need to be public.
- Simplified Unix socket address parsing and removed `Domain::AbstractUnix`.
- Stored the connected stream descriptor in async server contexts.

## [0.5.2] - 2021-11-22

### Added

- Added macOS support and filesystem Unix socket connections.
- Added request timeouts to server-side `TtrpcContext`.
- Added raw client connection helpers.

### Changed

- Removed redundant Unix socket path suffix handling.
- Updated socket creation and audited dependencies.

### Fixed

- Fixed synchronous server descriptor cleanup.

## [0.4.16] - 2021-11-18

### Fixed

- Closed connection descriptors from the synchronous server reaper thread.
- Corrected inline rustdoc rendering.

## [0.5.1] - 2021-04-25

### Added

- Added async server timeout enforcement.
- Made request contexts cloneable.

### Changed

- Improved async handler error propagation and cancellation when clients
  disconnect.

## [0.4.15] - 2021-04-20

### Fixed

- Cancelled async server handlers after their clients disconnect.

## [0.5.0] - 2021-02-24

### API changes

- **Breaking:** Generated client and server method signatures now carry a
  `Context` containing request metadata and timeout information.
- **Migration:** Regenerate service bindings and pass a `Context` when making
  client calls or implementing service methods.

### Added

- Added request metadata to generated client and server APIs.
- Added `Context` as the common carrier for metadata and timeouts.

### Changed

- Updated the async runtime to Tokio 1.0.

## [0.4.14] - 2020-12-25

### Fixed

- Fixed a synchronous server method-handler thread leak.

## [0.4.13] - 2020-12-23

### Fixed

- Fixed failures to wake synchronous client handlers.

## [0.4.12] - 2020-11-25

### Changed

- Reduced noisy log levels for frequent, non-actionable events.

## [0.4.11] - 2020-11-23

### Fixed

- Fixed a client file descriptor leak.
- Logged failures while closing the client receiver descriptor.

## [0.4.10] - 2020-11-20

### Changed

- Replaced `select` with `poll` in the synchronous transport loop.

## [0.4.9] - 2020-10-22

### Changed

- Accepted any `ToString` status message in status helper APIs.
- Renamed internal macros to follow Rust naming conventions.

## [0.4.8] - 2020-10-09

### Added

- Added async raw file descriptor conversions.
- Added graceful async shutdown and server restart support.

### Changed

- Refined server restart lifecycle APIs.

## [0.4.7] - 2020-09-21

### Added

- Added synchronous server restart support.

### Changed

- Reported message read failures as socket errors.

## [0.4.6] - 2020-09-04

### Changed

- Removed excessive informational logging.

## [0.4.5] - 2020-08-31

### Fixed

- Fixed a client panic when a request times out.

## [0.4.4] - 2020-08-24

### Fixed

- Set close-on-exec on async server sockets.

## [0.4.3] - 2020-08-18

### Added

- Added request timeout support to synchronous and async clients.
- Added raw file descriptor conversions.
- Implemented `std::error::Error` for the crate error type.

### Fixed

- Fixed client hangs after empty responses and socket failures.

## [0.4.2] - 2020-07-20

### Fixed

- Fixed docs.rs generation.

## [0.4.1] - 2020-07-20

### Changed

- Made async dependencies optional and improved feature separation.
- Added crate-level API documentation.
- Updated examples for the renamed code generator.

## [0.4.0] - 2020-07-10

### API changes

- **Added:** Asynchronous client and server APIs are available through the
  async feature and runtime-specific modules.
- **Changed:** Synchronous and asynchronous APIs, examples, and generated
  output are separated more clearly.
- **Migration:** Enable the async feature and regenerate protocol bindings
  before adopting the asynchronous interfaces.

### Added

- Added asynchronous clients and servers.
- Added async vsock support.
- Added `Debug` support for server request contexts.

### Changed

- Separated synchronous and asynchronous examples and features.
- Moved generated protocol code to Cargo's `OUT_DIR`.

### Fixed

- Reaped completed synchronous connection threads promptly.

## [0.3.0] - 2020-04-22

### API changes

- **Breaking:** `Server::start` now borrows `&mut self` instead of consuming
  the server.
- **Added:** Servers can add custom listeners, shut down explicitly, and be
  managed after startup.
- **Added:** Protocol bindings can be generated programmatically from Rust.

### Added

- Added pure-Rust programmatic Protocol Buffers code generation.
- Added server shutdown and custom listener APIs.
- Added close-on-exec socket creation.

### Fixed

- Fixed partial frame reads and writes and peer-close detection.
- Fixed potential server deadlocks and connection leaks.
- Fixed server panics after clients disconnect.

## [0.2.1] - 2020-02-20

### Changed

- Updated crate metadata and introductory documentation.

## [0.2.0] - 2020-01-15

### API changes

- **Breaking:** Generated types, fields, and status helpers were renamed to
  follow idiomatic Rust naming conventions.
- **Migration:** Regenerate bindings and update call sites to the new
  snake-case methods and fields.

### Added

- Added vsock server support.
- Generated example protocol code from `build.rs`.

### Changed

- Updated the `nix` dependency and removed compiler warnings.

## [0.1.0] - 2019-09-23

### API changes

- Initial public API for synchronous ttrpc clients, servers, and generated
  services.

### Added

- Initial synchronous ttrpc client and server implementation.
- Added Protocol Buffers service generation and client/server examples.

[Keep a Changelog]: https://keepachangelog.com/en/2.0.0/
[Semantic Versioning]: https://semver.org/spec/v2.0.0.html
[0.9.0]: https://github.com/containerd/ttrpc-rust/compare/cfe37a2c...f31f5925
[0.8.6]: https://github.com/containerd/ttrpc-rust/compare/af812a6f...44d34d5c
[0.5.10]: https://github.com/containerd/ttrpc-rust/compare/9a79290f...5784dc00
[0.8.5]: https://github.com/containerd/ttrpc-rust/compare/cfe37a2c...af812a6f
[0.8.4]: https://github.com/containerd/ttrpc-rust/compare/0610015a...cfe37a2c
[0.8.3]: https://github.com/containerd/ttrpc-rust/compare/b9e9dd8a...0610015a
[0.5.9]: https://github.com/containerd/ttrpc-rust/compare/674ada47...9a79290f
[0.8.2]: https://github.com/containerd/ttrpc-rust/compare/f669c050...b9e9dd8a
[0.5.8]: https://github.com/containerd/ttrpc-rust/compare/1c185cbb...674ada47
[0.7.2]: https://github.com/containerd/ttrpc-rust/compare/5d1d5dcd...593f1312
[0.5.7]: https://github.com/containerd/ttrpc-rust/compare/29560c8a...1c185cbb
[0.8.1]: https://github.com/containerd/ttrpc-rust/compare/b13d3fd5...f669c050
[0.5.6]: https://github.com/containerd/ttrpc-rust/compare/9bef9c6a...29560c8a
[0.5.5]: https://github.com/containerd/ttrpc-rust/compare/8968bfad...9bef9c6a
[0.8.0]: https://github.com/containerd/ttrpc-rust/compare/5d1d5dcd...b13d3fd5
[0.5.4]: https://github.com/containerd/ttrpc-rust/compare/66326830...8968bfad
[0.7.1]: https://github.com/containerd/ttrpc-rust/compare/0145f972...5d1d5dcd
[0.7.0]: https://github.com/containerd/ttrpc-rust/compare/499d5e5b...0145f972
[0.6.1]: https://github.com/containerd/ttrpc-rust/compare/e5a5373c...499d5e5b
[0.5.3]: https://github.com/containerd/ttrpc-rust/compare/4a6173d5...66326830
[0.6.0]: https://github.com/containerd/ttrpc-rust/compare/4a6173d5...e5a5373c
[0.5.2]: https://github.com/containerd/ttrpc-rust/compare/f8b10c67...4a6173d5
[0.4.16]: https://github.com/containerd/ttrpc-rust/compare/e5fc2b4a...cc0d4056
[0.5.1]: https://github.com/containerd/ttrpc-rust/compare/eef20041...f8b10c67
[0.4.15]: https://github.com/containerd/ttrpc-rust/compare/a9c9032e...e5fc2b4a
[0.5.0]: https://github.com/containerd/ttrpc-rust/compare/a9c9032e...eef20041
[0.4.14]: https://github.com/containerd/ttrpc-rust/compare/ea5972a0...a9c9032e
[0.4.13]: https://github.com/containerd/ttrpc-rust/compare/51853590...ea5972a0
[0.4.12]: https://github.com/containerd/ttrpc-rust/compare/40d21d75...51853590
[0.4.11]: https://github.com/containerd/ttrpc-rust/compare/6a242e34...40d21d75
[0.4.10]: https://github.com/containerd/ttrpc-rust/compare/961ad613...6a242e34
[0.4.9]: https://github.com/containerd/ttrpc-rust/compare/8f99147a...961ad613
[0.4.8]: https://github.com/containerd/ttrpc-rust/compare/d8597956...8f99147a
[0.4.7]: https://github.com/containerd/ttrpc-rust/compare/59381ad4...d8597956
[0.4.6]: https://github.com/containerd/ttrpc-rust/compare/9470d136...59381ad4
[0.4.5]: https://github.com/containerd/ttrpc-rust/compare/42e86180...9470d136
[0.4.4]: https://github.com/containerd/ttrpc-rust/compare/dfd7d40d...42e86180
[0.4.3]: https://github.com/containerd/ttrpc-rust/compare/98eb4363...dfd7d40d
[0.4.2]: https://github.com/containerd/ttrpc-rust/compare/0997a9ca...98eb4363
[0.4.1]: https://github.com/containerd/ttrpc-rust/compare/9a9248cd...0997a9ca
[0.4.0]: https://github.com/containerd/ttrpc-rust/compare/b0194b38...9a9248cd
[0.3.0]: https://github.com/containerd/ttrpc-rust/compare/90d85de6...b0194b38
[0.2.1]: https://github.com/containerd/ttrpc-rust/compare/fce4b488...90d85de6
[0.2.0]: https://github.com/containerd/ttrpc-rust/compare/76888ad7...fce4b488
[0.1.0]: https://github.com/containerd/ttrpc-rust/commits/76888ad7
