# Testing

## Automated (host, every push)

| Suite | Count | Covers |
|---|---|---|
| `luma-core` unit tests | 32 | rotation states and SRS kick tables (incl. I-piece), wall/floor kicks, blocked rotation, collisions, spawn, block out and lock out, seven-bag invariants and drought bound, deterministic replay, hold rules, single to four-line clears, T-spin double, perfect clear, back-to-back and combo scoring, lock delay and bounded resets, gravity table, soft and hard drop points, Sprint finish, Marathon level-up, practice reset, save/restore round trip, corrupt-state rejection |
| `luma-input` unit tests | 27 | quaternion twist, sign and wrap; axis probes (documented and mirrored axes); pointing range; LG's documented sensor2 and legacy examples; missing fields stay missing; error classification (sleep, duplicate, interval, unsupported, permission denied, malformed); DAS/ARR, platform repeats ignored, soft drop and lost key-up, OK/click de-duplication, long press, colour remap; trace round trip and live-vs-replay equality |
| `luma-input` gesture scenarios | 38 | both directions of twist, circle, swing and flick at 50/60/100 Hz; **swing moves proportionally and the pull back is ignored** (sizes, speeds, return speeds); ratcheting repeated swings; chained strokes; a deliberate opposite swing after resting; slow drift never moves; twisting never swings; gyro reported in deg/s detected; gyro-only and orientation-only streams; hold and repeat; slow roll; posture re-base and return; ±180° wraparound; no column drag while twisting; circle drift and return to the column; partial circles, curved swipes, scribbles; recoil; repeated flicks; slow sweeps, diagonals and noisy idle; drop including recoil and the next spawn; bouncy recoil; no drop from a circle's descending arc; slow downward pointing; drop disabled; all four profiles in one mixed sequence; button jiggle; reset requires stillness; sample gaps; invalid samples; motion-off mode; pointer-only fallback (circle, flick and drop, never twist); boundary flicker; button suspension of pointing; **20 varied natural attempts per gesture and direction in all six profiles, and 20 varied swings per direction**; **a 120-gesture mixed session with zero accidental drops** |
| `luma-app` unit tests | 7 | atomic persistence, corrupt-file isolation, settings migration from 0.1.0, records, menu navigation, remote scancodes, desktop key mapping |
| `--flow-test` (headless app) | 22 checks | onboarding skip, the full motion setup with simulated motion (hold, twist, swing, try, saved axes), start, Ready guard, button play, Back pauses, resume, save-and-quit, relaunch shows the resume prompt, continue restores paused, focus loss and gain, exit, motion loss → options |

Result at release 0.2.0: **all pass**. The synthetic attempt tests scored 20/20 for every gesture
and direction in all six profiles and for swings, with zero wrong actions or wrong-way moves.

**Synthetic is not hardware.** Synthetic motion uses LG's documented frame and a modelled
tremor. It guards the logic against regressions; it says nothing about real-remote accuracy.
Real traces recorded on the TV (Input diagnostics → Red) replay through exactly the same code:
`luma-blocks --analyze-trace trace.jsonl`. They should be added under
`crates/luma-input/tests/traces/` as soon as they exist.

## Automated (TV build, every push)

* ELF/ABI audit (armv7, softfp, interpreter, RPATH, `NEEDED`, glibc ≤ 2.28, no linked Luna).
* `--self-test` under qemu-arm on a Debian armel userland: engine replay, gesture recognition,
  mrcu parsing, atomic store. Luna is reported unavailable there, as expected.
* `webosbrew-ipk-verify` against firmware symbol dumps for webOS 5 and newer (CI).

## Manual (hardware)

See [TV_CHECKLIST.md](TV_CHECKLIST.md). Not yet performed.
