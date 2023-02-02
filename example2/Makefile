#
# Build
#

.PHONY: build
build:
	cargo build

.PHONY: deps
deps:
	rustup update stable
	rustup default stable
	rustup component add rustfmt clippy
