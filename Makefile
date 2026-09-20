.PHONY: all build release test lint run clean

all: build

# The compiler lives in compiler/ (crate `idyllic`); the workspace is at the root.
build:
	cargo build

release:
	cargo build --release

# Includes the parity suite: compiler/tests/fixtures pins behaviour to the
# original Haskell implementation.
test:
	cargo test

lint:
	cargo clippy --all-targets

# Start the REPL.  Check a file with: cargo run -- path/to/file.idl
run:
	cargo run

clean:
	cargo clean
