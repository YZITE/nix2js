#!/bin/sh

# this is a script which i usually use to debug nix2js js outputs
# USAGE: debug_tr_out.sh BASE_PATH FILES_RELATIVE_TO_BASE...

NIXPKGS="$1"; shift

for i; do
  echo "$i"
  src="$NIXPKGS/$i" inx="${i%.*}"
  dst="target/$inx"
  mkdir -p "$(dirname "$dst")"
  time target/release/nix2js "$src" "$dst.js"
  npx prettier "$dst.js" > "$dst.exp.js"
done
