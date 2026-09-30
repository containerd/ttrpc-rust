# Release Process

This document describes how to release the ttrpc runtime and code generators.

## Crate Release Process

### Versioning

Choose versions relative to each crate's latest published release, not an
unpublished version already present on the development branch. For `0.x.y`
crates, incompatible changes require incrementing `x` and resetting `y` to zero,
following [Cargo's compatibility rules]. Check both the generator's public API
(including re-exported dependency types) and the generated bindings.

[Cargo's compatibility rules]: https://doc.rust-lang.org/cargo/reference/semver.html#change-categories

### Release Steps

1. Add a release section for the target version to the relevant crate changelog:
   * `./CHANGELOG.md` for `ttrpc`.
   * `./compiler/CHANGELOG.md` for `ttrpc-compiler`.
   * `./ttrpc-codegen/CHANGELOG.md` for `ttrpc-codegen`.
   * `./ttrpc-codegen-prost/CHANGELOG.md` for `ttrpc-codegen-prost`.
2. Bump package and dependency versions in:
   * `./compiler/Cargo.toml`: Bump the package version as needed.
   * `./ttrpc-codegen/Cargo.toml`: Bump the package version as needed.
   * `./Cargo.toml`: Bump package version as needed. Then bump the workspace dependencies version to match the respective crates versions.
   * `./ttrpc-codegen-prost/Cargo.toml`: Bump `ttrpc-codegen-prost` as needed and update its dependency version in `./example-prost/Cargo.toml`.
3. Update dependency examples, compatibility tables, and migration notes in
   the READMEs and crate-level API documentation (`src/lib.rs`).
4. Validate the release using the toolchain pinned in `rust-toolchain.toml`:
   ```bash
   cargo build -p ttrpc-example --examples
   cargo test --workspace --features sync,async,security_extension
   cargo test -p ttrpc --no-default-features --features sync,async,prost,security_extension
   make check-all
   ```
   These commands are for Unix; `security_extension` is not supported on
   Windows. Run the standalone Prost generator checks below as well.
   Cargo.lock files are not tracked in this repository. These validation
   commands generate them; keep them for the locked publish checks. Run
   validation sequentially because example tests start Cargo subprocesses.
5. Commit the release changes and dry run publishing the workspace crates:
   ```bash
   cargo publish \
     --dry-run \
     --locked \
     -p ttrpc \
     -p ttrpc-codegen \
     -p ttrpc-compiler
   ```
6. After the release changes are merged and all checks pass, publish the
   selected crates from the validated revision using
   `cargo publish --locked -p <crate>` in the following order:
   1. `ttrpc-compiler`
   2. `ttrpc-codegen`
   3. `ttrpc`

### Standalone Prost Generator

`ttrpc-codegen-prost` lives in `ttrpc-codegen-prost/` and is a separate workspace. It is
not included in the root workspace publish commands above. Validate and
publish it separately when selected for release:

```bash
cargo test --manifest-path ttrpc-codegen-prost/Cargo.toml
make -C ttrpc-codegen-prost check
cargo build --manifest-path example-prost/Cargo.toml --examples
cargo publish --manifest-path ttrpc-codegen-prost/Cargo.toml --dry-run --locked
# After the dry run succeeds and the release changes are merged:
cargo publish --manifest-path ttrpc-codegen-prost/Cargo.toml --locked
```

Keep its package version and release notes independent from the
rust-protobuf `ttrpc-codegen` package.
