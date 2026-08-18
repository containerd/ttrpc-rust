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

# The `security_extension` feature is only supported on Unix platforms
# (see the compile_error! in src/lib.rs), and the "prost" and
# "rustprotobuf" backends are mutually exclusive, so never use
# `--all-features` here. Sub-crate Makefiles (compiler, ttrpc-codegen)
# pre-set FEATURES before including this file, as they don't have these
# features.
ifeq ($(OS),Windows_NT)
FEATURES ?= --features sync,async,rustprotobuf
else
FEATURES ?= --features sync,async,rustprotobuf,security_extension
endif

.PHONY: test
test:
	cargo test $(FEATURES) --verbose
ifneq ($(OS),Windows_NT)
	cargo test --no-default-features --features sync,async,prost --verbose
endif

.PHONY: check
check:
	cargo fmt --all -- --check
	cargo clippy --all-targets $(FEATURES) -- -D warnings
ifneq ($(OS),Windows_NT)
	cargo clippy --all-targets --no-default-features --features sync,async,prost -- -D warnings
endif

.PHONY: check-all
check-all:
	$(MAKE) check
	$(MAKE) -C compiler check
	$(MAKE) -C ttrpc-codegen check
