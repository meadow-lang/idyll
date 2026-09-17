#!/usr/bin/env bash
# Regenerate the Unicode tables and the Unicode test oracle.
#
# The Rust program in this directory links the exact unicode-width and
# unicode-segmentation versions rustyline depends on. Their table modules are
# private, so we copy them out of the cargo registry and widen visibility.
# Needs a cargo registry that already contains both crates (works offline).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
reg="${CARGO_HOME:-$HOME/.cargo}/registry/src"
uw="$(ls -d "$reg"/*/unicode-width-0.2.2 | head -1)"
us="$(ls -d "$reg"/*/unicode-segmentation-1.13.3 | head -1)"

cp "$uw/src/tables.rs" "$here/src/uw_tables.rs"
cp "$us/src/tables.rs" "$here/src/us_tables.rs"
sed -i.bak \
  -e 's/^fn lookup_width(/pub fn lookup_width(/' \
  -e 's/^fn is_transparent_zero_width(/pub fn is_transparent_zero_width(/' \
  -e 's/^fn is_ligature_transparent(/pub fn is_ligature_transparent(/' \
  -e 's/^struct WidthInfo(u16);/pub struct WidthInfo(pub u16);/' \
  -e 's/^static /pub static /' \
  -e 's/^struct Align\([0-9]*\)<T>(T);/pub struct Align\1<T>(pub T);/' \
  "$here/src/uw_tables.rs"
rm -f "$here/src/uw_tables.rs.bak"

cargo build --offline --release --manifest-path "$here/Cargo.toml" --quiet
bin="$here/target/release/ucdgen"
tmp="$(mktemp -d)"
"$bin" tables > "$tmp/tables.txt"
"$bin" oracle > "$tmp/oracle.txt"
python3 "$here/gen_tables.py" "$tmp/tables.txt" \
  > "$root/src/System/Console/Rustyline/Internal/Unicode/Tables.hs"
cp "$tmp/oracle.txt" "$root/test/data/unicode-oracle.txt"
rm -rf "$tmp" "$here/target" "$here/src/uw_tables.rs" "$here/src/us_tables.rs"
echo "regenerated Tables.hs and test/data/unicode-oracle.txt"
