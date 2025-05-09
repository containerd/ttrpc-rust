.PHONY: check
check:
	cargo fmt --all -- --check
	cargo clippy --all-targets -- -D warnings

.PHONY: test
test:
	cargo test --verbose