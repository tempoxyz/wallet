.PHONY: build release clean check test fix install uninstall sign

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
	cargo nextest run --workspace

check:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo nextest run --workspace

fix:
	cargo fmt
	cargo clippy --fix --allow-dirty --allow-staged

# Sign macOS debug binaries for Secure Enclave access (requires Apple Developer cert).
# Usage: make sign IDENTITY="Developer ID Application: Your Name (TEAMID)"
sign: build
	@if [ -z "$(IDENTITY)" ]; then echo "Usage: make sign IDENTITY=\"Developer ID Application: ...\""; exit 1; fi
	codesign --force --sign "$(IDENTITY)" --entitlements assets/entitlements.plist --options runtime target/debug/tempo-wallet
	codesign --force --sign "$(IDENTITY)" --entitlements assets/entitlements.plist --options runtime target/debug/tempo-mpp
	codesign --force --sign "$(IDENTITY)" --entitlements assets/entitlements.plist --options runtime target/debug/tempo-request

# Generate coverage locally (requires cargo-llvm-cov and llvm-tools-preview)
# Install once: `rustup component add llvm-tools-preview` and `cargo install cargo-llvm-cov`
coverage:
	cargo llvm-cov --all-features --workspace --fail-under-lines 85 --lcov --output-path lcov.info
