# Release Process

This document describes the steps to release a new version of the crate or wasi-demo-app images.

## Crate Release Process

### Release Steps

1. Add a new dated release section to the relevant crate changelog:
   * `./CHANGELOG.md` for `ttrpc`.
   * `./compiler/CHANGELOG.md` for `ttrpc-compiler`.
   * `./ttrpc-codegen/CHANGELOG.md` for `ttrpc-codegen`.
   * `./ttrpc-codegen-prost/CHANGELOG.md` for `ttrpc-codegen-prost`.
2. Bump package and dependency versions in:
   * `./compiler/Cargo.toml`: Bump the package version as needed.
   * `./ttrpc-codegen/Cargo.toml`: Bump the package version as needed.
   * `./Cargo.toml`: Bump package version as needed. Then bump the workspace dependencies version to match the respective crates versions.
   * `./ttrpc-codegen-prost/Cargo.toml`: Bump `ttrpc-codegen-prost` as needed and update its dependency version in `./example-prost/Cargo.toml`.
3. Commit the changes and get them merged in the repo.
4. Dry run the `cargo publish` command as follows:
   ```bash
   cargo +nightly publish \
     -Z package-workspace \
     --dry-run \
     --locked \
     -p ttrpc \
     -p ttrpc-codegen \
     -p ttrpc-compiler
   ```
5. If the dry run succeeds, publish the crates that need publishing using
   `cargo publish -p <crate>` in the following order:
   1. `ttrpc-compiler`
   2. `ttrpc-codegen`
   3. `ttrpc`

### Standalone Prost Generator

`ttrpc-codegen-prost` lives in `ttrpc-codegen-prost/` and is a separate workspace. It is
not included in the root workspace publish commands above. Validate and
publish it separately when selected for release:

```bash
cargo test --manifest-path ttrpc-codegen-prost/Cargo.toml
cargo build --manifest-path example-prost/Cargo.toml --examples
cargo publish --manifest-path ttrpc-codegen-prost/Cargo.toml --dry-run --locked
# After the dry run succeeds and the release changes are merged:
cargo publish --manifest-path ttrpc-codegen-prost/Cargo.toml --locked
```

Keep its package version and release notes independent from the
rust-protobuf `ttrpc-codegen` package.
