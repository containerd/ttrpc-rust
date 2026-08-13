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
# (see the compile_error! in src/lib.rs), so avoid `--all-features` on
# Windows. Sub-crate Makefiles (compiler, ttrpc-codegen) pre-set FEATURES
# before including this file, as they don't have these features.
ifeq ($(OS),Windows_NT)
FEATURES ?= --features sync,async
else
FEATURES ?= --all-features
endif

.PHONY: test
test:
	cargo test $(FEATURES) --verbose

.PHONY: check
check:
	cargo fmt --all -- --check
	cargo clippy --all-targets $(FEATURES) -- -D warnings

.PHONY: check-all
check-all:
	$(MAKE) check
	$(MAKE) -C compiler check
	$(MAKE) -C ttrpc-codegen check
