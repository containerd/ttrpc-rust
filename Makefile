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
	cargo test --features sync,async,rustprotobuf
else
	# cargo test --all-features --verbose
	cargo test --features sync,async,rustprotobuf
	cargo test --no-default-features --features sync,async,prost
endif
	
.PHONY: check
check:
	cargo fmt --all -- --check
	cargo clippy --all-targets --features sync,async -- -D warnings
	# Skip prost check on Windows
ifeq ($(OS),Windows_NT)
	@echo "Skipping prost check on Windows"
else
	cargo clippy --all-targets --no-default-features --features sync,async,prost -- -D warnings
endif

.PHONY: check-all
check-all:
	$(MAKE) check
	$(MAKE) -C compiler check
	$(MAKE) -C ttrpc-codegen check
