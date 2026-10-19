# Building

GitHub Actions (`.github/workflows/build.yml`) is the reference build. Every push runs the host
checks, cross-builds both IPK identities, audits them and uploads them as a workflow artifact.
A `v*` tag, or a manual run with `release_tag`, publishes a pre-release.

## Pinned inputs

| Input | Pin |
|---|---|
| Rust | 1.97.0 (`rust-toolchain.toml`), target `armv7-unknown-linux-gnueabi` |
| Rust crates | `Cargo.lock` (`--locked` in CI) |
| webOS SDK | openlgtv/buildroot-nc4 `webos-a38c582` x86_64, sha256 `04ad3311…3616` (gcc 14.2, glibc 2.12 sysroot, softfp) |
| SDL2 | webosbrew/SDL-webOS `release-2.30.12-webos.6`, sha256 `63dc9ea2…c698` (bundled in the IPK) |
| Fonts | Inter 4.1 (OFL), committed under `crates/luma-app/assets/fonts` |
| GitHub Actions | pinned by commit SHA |

## Host platform support

| Host | Desktop simulator + tests | TV package |
|---|---|---|
| Linux x86_64 (Ubuntu 24.04, CI) | **verified** | **verified** (buildroot-nc4 SDK) |
| Windows 11 via WSL2 Ubuntu 24.04 | expected to work (same Linux path), not tested here | expected to work, not tested here |
| Linux arm64 | expected to work (simulator) | not supported (the SDK is x86_64-only) |
| macOS | not tested | not supported by this SDK |

## Tests and simulator (any Linux desktop)

```sh
sudo apt-get install -y libsdl2-dev
cargo test --workspace              # engine, gestures (all six profiles), persistence, parsing
cargo run -p luma-app -- --flow-test  # headless app state machine
cargo run -p luma-app               # simulator window
```

Simulator controls:

| Key / mouse | Acts as |
|---|---|
| Arrows, Enter, Esc / Backspace | remote D-pad, OK, Back |
| F1 F2 F3 F4 | Red, Green, Yellow, Blue |
| Mouse | Magic Remote pointer |
| Hold right mouse button + move | aim the simulated motion sensor (yaw / pitch) |
| Q / E (hold) | twist anticlockwise / clockwise |
| 1 2 | scripted twist clockwise / anticlockwise |
| 3 4 | scripted circle clockwise / anticlockwise |
| 5 6 | scripted flick left / right |
| 7 | scripted downward flick (drop) |
| 8 9 | scripted swing left / right (with the natural pull back) |

Simulated input is labelled "Simulated (not hardware)" in diagnostics. It shows that the logic
works, not how a real remote behaves.

Other entry points: `--replay trace.jsonl` (play a recorded trace into the app),
`--analyze-trace trace.jsonl [--profile circle-flick]` (offline replay through the gesture
engine), `--shots DIR` (render the documentation screenshots).

## TV package (Linux x86_64)

```sh
curl -fsSLO https://github.com/openlgtv/buildroot-nc4/releases/download/webos-a38c582/arm-webos-linux-gnueabi_sdk-buildroot-x86_64.tar.gz
echo "04ad3311b48b4557a7002aef56ae2e167478e8e129f37daac04649bddf813616  arm-webos-linux-gnueabi_sdk-buildroot-x86_64.tar.gz" | sha256sum -c
sudo tar xzf arm-webos-linux-gnueabi_sdk-buildroot-x86_64.tar.gz -C /opt
/opt/arm-webos-linux-gnueabi_sdk-buildroot/relocate-sdk.sh
export WEBOS_SDK=/opt/arm-webos-linux-gnueabi_sdk-buildroot
sudo apt-get install -y qemu-user binutils

scripts/build-deps.sh webos
cargo build --locked --release --target armv7-unknown-linux-gnueabi -p luma-app
scripts/elf-audit.sh target/armv7-unknown-linux-gnueabi/release/luma-blocks deps-out/webos/lib/libSDL2-2.0.so.0
scripts/smoke-armel.sh target/armv7-unknown-linux-gnueabi/release/luma-blocks deps-out/webos/lib/libSDL2-2.0.so.0
python3 scripts/gen-notices.py --licenses-dir build/licenses && cp -r deps-out/webos/licenses/* build/licenses/
for f in release debug; do
  python3 scripts/package-ipk.py --binary target/armv7-unknown-linux-gnueabi/release/luma-blocks \
    --sdl-lib deps-out/webos/lib/libSDL2-2.0.so.0 --version 0.2.6 --flavor $f \
    --strip $WEBOS_SDK/bin/arm-webos-linux-gnueabi-strip --licenses build/licenses --out dist
done
```

What each check proves:

* `elf-audit.sh` checks for 32-bit ARM, the softfp calling convention, the
  `/lib/ld-linux.so.3` interpreter, DT_RPATH `$ORIGIN/../lib` and no DT_RUNPATH. It also checks
  the allowed `NEEDED` libraries, that glibc symbols are no newer than webOS 5's 2.28, and that
  Luna/glib are not linked (they are `dlopen`ed).
* `smoke-armel.sh` runs `--self-test` under qemu-arm on a Debian armel userland.
* CI additionally runs `webosbrew-ipk-verify` against firmware symbol dumps for webOS 5 and
  newer.
* None of these prove on-TV behaviour.

## Identities and versions

* `packaging/webos/appinfo.json` holds the provisional id `io.github.danzig666.lumablocks`.
  **Set the final id there.**
* `--flavor debug` packages `io.github.danzig666.lumablocks.debug` titled "Luma Blocks (Dev)".
  It installs beside the release identity, with its own data directory, so testing never
  overwrites normal saves.
* The webOS package version is the numeric `X.Y.Z` part of the release tag.
