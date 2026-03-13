.PHONY: build release clean check test fix install uninstall run coverage

build:
	cargo build

# make run ARGS="http://localhost:3000/api/data"
run:
	cargo run -q -p tempo-wallet -- $(ARGS)

release:
	cargo build --release

install: release
	mkdir -p $(HOME)/.local/bin
	cp target/release/tempo-wallet $(HOME)/.local/bin/tempo-wallet
	cp target/release/tempo-request $(HOME)/.local/bin/tempo-request
	chmod +x $(HOME)/.local/bin/tempo-wallet $(HOME)/.local/bin/tempo-request

uninstall:
	rm -f $(HOME)/.local/bin/tempo-wallet $(HOME)/.local/bin/tempo-request

clean:
	cargo clean

# Run all tests (uses mocks, no network required)
test:
	cargo test --workspace --all-features --locked

# Full local parity with CI lint+test gates.
# Requires `typos` and `cargo-deny` to be installed.
check:
	cargo +nightly fmt --all -- --check
	cargo +nightly clippy --workspace --all-targets --all-features --locked -- -D warnings
	cargo test --workspace --all-features --locked
	RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked
	typos
	cargo deny check

fix:
	cargo +nightly fmt --all
	cargo clippy --fix --allow-dirty --allow-staged

# Generate coverage locally (requires cargo-llvm-cov and llvm-tools-preview)
# Install once: `rustup component add llvm-tools-preview` and `cargo install cargo-llvm-cov`
coverage:
	cargo llvm-cov --all-features --workspace --fail-under-lines 85 --lcov --output-path lcov.info
