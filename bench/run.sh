#!/usr/bin/env bash
# Build every benchmark in every language that is installed, check that they
# agree, and time them: the best and the median of RUNS runs, wall clock, in
# milliseconds (process start-up included).
#
#   bench/run.sh [RUNS]
set -u
cd "$(dirname "$0")"
RUNS=${1:-7}
IDYLLIC=${IDYLLIC:-../target/release/idyllic.exe}
mkdir -p build
have() { command -v "$1" >/dev/null 2>&1; }

build() { # benchmark
  local b=$1
  [ -x "$IDYLLIC" ] && "$IDYLLIC" $b/idyll.idl -o build/$b-idyll.exe 2>build/$b-idyll.log
  have clang && clang -O2 -Wno-deprecated-declarations $b/c.c -o build/$b-c.exe 2>build/$b-c.log
  have clang && [ -f $b/c_pool.c ] && clang -O2 -Wno-deprecated-declarations $b/c_pool.c -o build/$b-c_pool.exe 2>build/$b-c_pool.log
  have rustc && rustc -C opt-level=3 -o build/$b-rust.exe $b/rust.rs 2>build/$b-rust.log
  have go && go build -o build/$b-go.exe $b/go.go 2>build/$b-go.log
  have ghc && ghc -O2 -outputdir build/$b-hs -o build/$b-haskell.exe $b/haskell.hs >build/$b-haskell.log 2>&1
  have meadow && meadow_build $b glade && meadow_build $b silo
  true
}

meadow_build() { # benchmark runtime: Meadow on the runtime Idyll runs on
  local b=$1 rt=$2 out native
  out=build/$b-meadow-$rt
  # (an executable needs a package: the file is its `src/Main.mw`)
  mkdir -p $out/src
  printf '[package]\nname = "Bench"\nversion = "0.1.0"\n' >$out/Meadow.toml
  cp $b/meadow.mw $out/src/Main.mw
  native=$out/target/release/native/Bench.exe
  if [ $rt = silo ]; then
    meadow build --release --emit exe --runtime silo $out >build/$b-meadow_silo.log 2>&1
    native=$out/target/release/native/silo/Bench.exe
    cp $native build/$b-meadow_silo.exe
  else
    meadow build --release --emit exe $out >build/$b-meadow.log 2>&1
    cp $native build/$b-meadow.exe
  fi
}

time_ms() { # command... -> "best median" over RUNS runs
  local times=() start end
  for _ in $(seq "$RUNS"); do
    start=$(date +%s%N); "$@" >/dev/null 2>&1; end=$(date +%s%N)
    times+=($(( (end - start) / 1000000 )))
  done
  local sorted=($(printf '%s\n' "${times[@]}" | sort -n))
  echo "${sorted[0]} ${sorted[$((RUNS / 2))]}"
}

printf '%-6s %-11s %10s %10s   %s\n' bench language best_ms median_ms answer
for b in fib list map ask state triples mixed; do
  build $b
  for lang in idyll meadow_silo meadow c c_pool rust go haskell python js; do
    case $lang in
      python) have py || continue; cmd=(py $b/python.py) ;;
      js) have node || continue; cmd=(node --stack-size=4000 $b/js.js) ;;
      *) [ -x build/$b-$lang.exe ] || continue; cmd=(build/$b-$lang.exe) ;;
    esac
    answer=$("${cmd[@]}" 2>&1 | tr -d '\r' | tail -1)
    read -r best median <<<"$(time_ms "${cmd[@]}")"
    printf '%-6s %-11s %10s %10s   %s\n' $b $lang $best $median "$answer"
  done
done
