# Project Names
HS_EXE_NAME = idyllic
RTS_LIB_NAME = rts
DRV_BIN = driver/target/debug/driver

# Dynamic paths
GET_HS_BIN = $(shell cd compiler && cabal list-bin exe:$(HS_EXE_NAME) 2>/dev/null)

.PHONY: all hs_build rts_build drv_build run clean

all: drv_build

# 1. Build the SKI Reduction Engine (Rust Library)
rts_build:
	cd rts && cargo build

# 2. Build the Haskell Compiler (Frontend)
hs_build:
	cd compiler && cabal build exe:$(HS_EXE_NAME)

# 3. Build the Driver (links against rts_build)
drv_build: rts_build hs_build
	cd driver && cargo build

# 4. Run the REPL
run: drv_build
	@HS_PATH="$(GET_HS_BIN)"; \
	cd driver && cargo run -- --compiler-path "$$HS_PATH"

clean:
	cabal clean
	cd driver && cargo clean
	cd rts && cargo clean