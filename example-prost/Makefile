#
# Build
#

.PHONY: build
build:
ifeq ($(OS),Windows_NT)
	@echo "Skipping build on Windows"
else
	cargo build
endif

.PHONY: build-examples
build-examples: build
ifeq ($(OS),Windows_NT)
	@echo "Skipping build-examples on Windows"
else
	cargo build --example server
	cargo build --example client
	cargo build --example async-server
	cargo build --example async-client
	cargo build --example async-stream-server
	cargo build --example async-stream-client
endif

.PHONY: deps
deps:
	rustup update stable
	rustup default stable
	rustup component add rustfmt clippy
	../install_protoc.sh
