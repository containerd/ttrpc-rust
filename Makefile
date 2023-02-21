export PROTOC=${HOME}/protoc/bin/protoc

all: debug test

#
# Build
#

.PHONY: debug
debug:
	cargo build --verbose --all-targets

.PHONY: release
release:
	cargo build --release

.PHONY: build
build: debug

#
# Tests and linters
#

.PHONY: test
test:
	cargo test --features sync,async --verbose
	cargo test --features sync,async,prost --verbose

.PHONY: check
check:
	cargo fmt --all -- --check
	cargo clippy --all-targets --features sync,async -- -D warnings
	cargo clippy --all-targets --features sync,async,prost -- -D warnings

.PHONY: deps
deps:
	rustup update stable
	rustup default stable
	rustup component add rustfmt clippy
	./install_protoc.sh
