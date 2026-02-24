.PHONY: build release clean check test fix install e2e

build:
	cargo build

# make run ARGS="http://localhost:3000/api/data"
run:
	cargo run -q -- $(ARGS)

release:
	cargo build --release

install:
	cargo install --path .

clean:
	cargo clean

# Run all tests (uses mocks, no network required)
test:
	cargo test -- --quiet

check:
	cargo fmt --check
	cargo clippy -- -D warnings
	cargo test -- --quiet
	cargo build

fix:
	cargo fmt
	cargo clippy --fix --allow-dirty --allow-staged

# Run e2e tests against live mpp-proxy (requires funded wallet)
e2e: build
	cargo test --test live -- --ignored --nocapture
