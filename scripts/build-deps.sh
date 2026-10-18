#!/usr/bin/env bash
# Fetch the native dependencies for the TV build: SDL2 with webOS integration (webosbrew
# SDL-webOS backport, upstream struct ABI, pinned and SHA-256 verified). It is bundled into the
# IPK under lib/ because the TV's own SDL2 has LG's modified event ABI. Luna and glib are not
# needed at build time: the app loads them from the TV at runtime.
#
#   scripts/build-deps.sh webos      # -> deps-out/webos/{include,lib,licenses}
set -euo pipefail
TARGET="${1:-webos}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/deps-out/$TARGET"
SRC="$ROOT/build/deps-src"
mkdir -p "$SRC" "$OUT/lib" "$OUT/include" "$OUT/licenses"
fetch() { # url sha file
  if [ ! -f "$SRC/$3" ]; then curl -fsSL -o "$SRC/$3.part" "$1" && mv "$SRC/$3.part" "$SRC/$3"; fi
  echo "$2  $SRC/$3" | sha256sum -c - >/dev/null || { echo "checksum mismatch for $3" >&2; exit 1; }
}
[ "$TARGET" = webos ] || { echo "only the webos target needs fetched dependencies (host uses libsdl2-dev)" >&2; exit 2; }
if [ ! -f "$OUT/lib/libSDL2-2.0.so.0" ]; then
  SDL_TAG=release-2.30.12-webos.6
  SDL_SHA=63dc9ea268bdf64801fce5e0c19585ee6aaf79f0b08486c853c47f922385c698
  fetch "https://github.com/webosbrew/SDL-webOS/releases/download/$SDL_TAG/SDL2-2.30.12-webos.tar.gz" "$SDL_SHA" "SDL2-2.30.12-webos.tar.gz"
  rm -rf "$OUT/sdl2" && mkdir -p "$OUT/sdl2" && tar xzf "$SRC/SDL2-2.30.12-webos.tar.gz" -C "$OUT/sdl2"
  cp -a "$OUT/sdl2/lib/libSDL2-2.0.so"* "$OUT/sdl2/lib/libSDL2.so" "$OUT/lib/"
  cp -a "$OUT/sdl2/include/SDL2" "$OUT/include/"
  cp -a "$OUT/sdl2/share/licenses/SDL2" "$OUT/licenses/" 2>/dev/null || true
fi
echo "deps ready in $OUT"
ls -la "$OUT/lib"/
