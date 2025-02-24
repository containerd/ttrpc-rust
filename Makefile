PROTOC ?= $(shell which protoc 2>/dev/null || echo $(HOME)/protoc/bin/protoc)

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
ifeq ($OS,Windows_NT)
	# async isn't enabled for windows, don't test that feature
	cargo test --verbose
else
	# cargo test --all-features --verbose
	cargo test --features sync,async,rustprotobuf
	cargo test --no-default-features --features sync,async,prost
endif
	
.PHONY: check
check:
	cargo fmt --all -- --check
	cargo clippy --all-targets --features sync,async -- -D warnings
	cargo clippy --all-targets --no-default-features --features sync,async,prost -- -D warnings

.PHONY: deps
deps:
	rustup update stable
	rustup default stable
	rustup component add rustfmt clippy
	./install_protoc.sh
