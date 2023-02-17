#
# Build
#

.PHONY: build
build:
	cargo build

.PHONY: build-examples
build-examples: build
	cargo build --example server
	cargo build --example client
	cargo build --example async-server
	cargo build --example async-client
	cargo build --example async-stream-server
	cargo build --example async-stream-client

.PHONY: deps
deps:
	rustup update stable
	rustup default stable
	rustup component add rustfmt clippy
