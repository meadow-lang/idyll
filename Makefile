.PHONY: all build release test lint run clean

all: build

# The compiler is a workspace of its own, in compiler/ (one crate per pass;
# the command is the crate `idyllic`).  The reference interpreter, idyll-eval/,
# is a separate package that the compiler is tested against.  Both build into
# the one target/ at the root (see .cargo/config.toml).
COMPILER := cd compiler && cargo
EVAL := cd idyll-eval && cargo

build:
	$(COMPILER) build

release:
	$(COMPILER) build --release

# The compiler's own tests, then the specification: every program in the
# corpus and the tutorial must compute what the reference interpreter does.
test:
	$(COMPILER) test
	$(EVAL) test

lint:
	$(COMPILER) clippy --all-targets
	$(EVAL) clippy --all-targets

# Start the REPL.  Check a file with: cd compiler && cargo run -- path/to/file.idl
run:
	$(COMPILER) run

clean:
	$(COMPILER) clean
