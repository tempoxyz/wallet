.PHONY: build release clean check test fix install uninstall e2e

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
	chmod +x $(HOME)/.local/bin/tempo-wallet

uninstall:
	rm -f $(HOME)/.local/bin/tempo-wallet

clean:
	cargo clean

# Run all tests (uses mocks, no network required)
test:
	cargo nextest run --workspace

check:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo nextest run --workspace
	cargo doc --no-deps

fix:
	cargo fmt
	cargo clippy --fix --allow-dirty --allow-staged

# Run e2e tests against live mpp-proxy (requires funded wallet)
e2e: build
	cargo test -p tempo-wallet --test live -- --ignored --nocapture
# Generate coverage locally (requires cargo-llvm-cov and llvm-tools-preview)
# Install once: `rustup component add llvm-tools-preview` and `cargo install cargo-llvm-cov`
coverage:
	cargo llvm-cov --all-features --workspace --fail-under-lines 85 --lcov --output-path lcov.info
