.PHONY: build release clean check test fix install uninstall

build:
	cargo build

# make run ARGS="http://localhost:3000/api/data"
run:
	cargo run -q -p tempo-mpp -- $(ARGS)

release:
	cargo build --release

install: release
	mkdir -p $(HOME)/.local/bin
	cp target/release/tempo-wallet $(HOME)/.local/bin/tempo-wallet
	cp target/release/tempo-mpp $(HOME)/.local/bin/tempo-mpp
	cp target/release/tempo-request $(HOME)/.local/bin/tempo-request
	chmod +x $(HOME)/.local/bin/tempo-wallet $(HOME)/.local/bin/tempo-mpp $(HOME)/.local/bin/tempo-request

uninstall:
	rm -f $(HOME)/.local/bin/tempo-wallet $(HOME)/.local/bin/tempo-mpp $(HOME)/.local/bin/tempo-request

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

# Generate coverage locally (requires cargo-llvm-cov and llvm-tools-preview)
# Install once: `rustup component add llvm-tools-preview` and `cargo install cargo-llvm-cov`
coverage:
	cargo llvm-cov --all-features --workspace --fail-under-lines 85 --lcov --output-path lcov.info
