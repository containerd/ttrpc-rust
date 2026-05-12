# Release Process

This document describes the steps to release a new version of the crate or wasi-demo-app images.

## Crate Release Process

### Release Steps

1. Bump package and dependency versions in:
   * `./compiler/Cargo.toml`: Bump the package version as needed.
   * `./ttrpc-codegen/Cargo.toml`: Bump the package version as needed.
   * `./Cargo.toml`: Bump package version as needed. Then bump the workspace dependencies version to match the respective crates versions.
2. Commit the changes and get them merged in the repo.
2. Dry run the `cargo publish` command as follow
   ```bash
   cargo +nightly publish \
     -Z package-workspace \
     --dry-run \
     --locked \
     -p ttrpc \
     -p ttrpc-codegen \
     -p ttrpc-compiler
   ```
2. If the dry run succeeds, publish the crates that need publishing using `cargo publihs -p <crate>` in the following order
   1. `ttrpc-compiler`
   2. `ttrpc-codegen`
   3. `ttrpc`