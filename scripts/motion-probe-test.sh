#!/usr/bin/env bash
# Runs `luma-blocks --motion-probe` against a stand-in for the TV's libhelpers/mrcu service
# (tests/fake-mrcu) to check the connection logic: the stream must reach 50/s even though the
# service starts at 1/s, a rejected sensorType must fall back, a sleeping remote must be picked
# up once it wakes, a stream that silently dies must be renewed, a pause in the data (button
# mode) must be waited out on the same subscription, and denial or silence must end in a clean
# failure.
# Usage: scripts/motion-probe-test.sh [path/to/luma-blocks]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${1:-$ROOT/target/debug/luma-blocks}"
LIB="$(mktemp -d)"
trap 'rm -rf "$LIB"' EXIT
cc -shared -fPIC -O1 -o "$LIB/libhelpers.so.2" "$ROOT/tests/fake-mrcu/fake_helpers.c" -lpthread -lm
fail=0
check() { # mode expected-exit [text that must appear]
  local out rc=0
  out="$(FAKE_MRCU=$1 LD_LIBRARY_PATH="$LIB" timeout 60 "$BIN" --motion-probe "${4:-5}" 2>&1)" || rc=$?
  if [ "$rc" = "$2" ] && { [ -z "${3:-}" ] || grep -q "$3" <<<"$out"; }; then
    echo "PASS $1 (exit $rc): $(echo "$out" | tail -1)"
  else
    echo "FAIL $1 (exit $rc, expected $2)"; echo "$out"; fail=1
  fi
}
check_absent() { # mode text-that-must-not-appear seconds
  local out
  out="$(FAKE_MRCU=$1 LD_LIBRARY_PATH="$LIB" timeout 60 "$BIN" --motion-probe "$3" 2>&1)" || true
  if grep -q "$2" <<<"$out" || ! grep -q "state: streaming" <<<"$(tail -3 <<<"$out")"; then
    echo "FAIL $1 (must not show \"$2\" and must end streaming)"; echo "$out"; fail=1
  else
    echo "PASS $1 (no \"$2\", ends streaming)"
  fi
}
check ok 0
check invalidtypes 0
check sleepy 0
check stall 0 "renewing" 10
check keymode 0 "state: paused" 7
check_absent keymode "renewing" 7
check denied 1
check silent 1
exit $fail
