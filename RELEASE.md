# Release Process

This document describes the steps to release a new version of the crate or wasi-demo-app images.

## Crate Release Process

### Release Steps

1. Add a new dated release section to the relevant crate changelog:
   * `./CHANGELOG.md` for `ttrpc`.
   * `./compiler/CHANGELOG.md` for `ttrpc-compiler`.
   * `./ttrpc-codegen/CHANGELOG.md` for `ttrpc-codegen`.
2. Bump package and dependency versions in:
   * `./compiler/Cargo.toml`: Bump the package version as needed.
   * `./ttrpc-codegen/Cargo.toml`: Bump the package version as needed.
   * `./Cargo.toml`: Bump package version as needed. Then bump the workspace dependencies version to match the respective crates versions.
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
