.PHONY: all build release test lint run clean

all: build

# Three packages, each a workspace of its own, all building into the one
# target/ at the root (see .cargo/config.toml):
#   compiler/      the compiler, one crate per pass; the command is `idyllic`
#   idyll-eval/    the reference interpreter the compiler is tested against
#   idyll-driver/  the `idyll` command: the REPL, and the build system
COMPILER := cd compiler && cargo
EVAL := cd idyll-eval && cargo
DRIVER := cd idyll-driver && cargo

build:
	$(COMPILER) build
	$(DRIVER) build

release:
	$(COMPILER) build --release
	$(DRIVER) build --release

# The compiler's own tests, then the specification: every program in the
# corpus and the tutorial must compute what the reference interpreter does.
test:
	$(COMPILER) test
	$(EVAL) test
	$(DRIVER) test

lint:
	$(COMPILER) clippy --all-targets
	$(EVAL) clippy --all-targets
	$(DRIVER) clippy --all-targets

# Start the REPL.  Check a file with: cd compiler && cargo run -- path/to/file.idl
run:
	$(DRIVER) run

# One clean suffices: every package shares target/.
clean:
	$(COMPILER) clean
