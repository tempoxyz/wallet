.PHONY: build release clean check test test-fast fix install

build:
	cargo build

# make run ARGS="http://localhost:3000/api/data"
run:
	cargo run -q -- $(ARGS)

release:
	cargo build --release

install:
	cargo install --path cli

clean:
	cargo clean

# Run all tests (uses mocks, no network required)
test:
	cargo test

# Unit tests only (fastest, library tests only)
test-fast:
	cargo test --lib

check:
	cargo fmt --check
	cargo clippy -- -D warnings
	cargo test
	cargo build

fmt:
	cargo fmt
	cargo clippy --fix --allow-dirty --allow-staged
