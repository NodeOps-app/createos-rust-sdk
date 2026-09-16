.PHONY: check fmt fmt-check lint test doc

check: fmt-check lint test doc

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test --all-features

doc:
	cargo doc --no-deps --all-features
