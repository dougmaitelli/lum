.PHONY: fmt check

fmt:
	cargo fmt --all
	npm --prefix web run format

check:
	cargo fmt --all -- --check
	npm --prefix web run check
	cargo clippy --locked --all-targets -- -D warnings
	cargo clippy --locked --all-targets --all-features -- -D warnings
	cargo test --locked --all-targets --all-features
