# Testing

## Automated (host, every push)

| Suite | Count | Covers |
|---|---|---|
| `luma-core` unit tests | 36 | rotation states and SRS kick tables (incl. I-piece), wall/floor kicks, blocked rotation, collisions, spawn, block out and lock out, seven-bag invariants and drought bound, deterministic replay, hold rules, single to four-line clears, T-spin double, perfect clear, back-to-back and combo scoring, lock delay and bounded resets, sliding after a hard drop (late move, falling off a ledge, second drop locks, holding the slide, old saves load), gravity table, soft and hard drop points, Sprint finish, Marathon level-up, practice reset, save/restore round trip, corrupt-state rejection |
| `luma-input` unit tests | 27 | quaternion twist, sign and wrap; axis probes (documented and mirrored axes); pointing range; LG's documented sensor2 and legacy examples; missing fields stay missing; error classification (sleep, duplicate, interval, unsupported, permission denied, malformed); DAS/ARR, platform repeats ignored, soft drop and lost key-up, OK/click de-duplication, long press, colour remap; trace round trip and live-vs-replay equality |
| `luma-input` gesture scenarios | 39 | both directions of twist, circle, swing and flick at 50/60/100 Hz; **swing moves proportionally and the pull back is ignored** (sizes, speeds, return speeds); ratcheting repeated swings; chained strokes; a deliberate opposite swing after resting; slow drift never moves; twisting never swings; gyro reported in deg/s detected; pointing still follows the remote when the orientation arrives in the opposite convention; gyro-only and orientation-only streams; hold and repeat; slow roll; posture re-base and return; ±180° wraparound; no column drag while twisting; circle drift and return to the column; partial circles, curved swipes, scribbles; recoil; repeated flicks; slow sweeps, diagonals and noisy idle; drop including recoil and the next spawn; bouncy recoil; no drop from a circle's descending arc; slow downward pointing; drop disabled; all six profiles in one mixed sequence; progressive swing columns; button jiggle; reset requires stillness; sample gaps; invalid samples; motion-off mode; pointer-only fallback (circle, flick and drop, never twist); boundary flicker; button suspension of pointing; **20 varied natural attempts per gesture and direction in all six profiles, and 20 varied swings per direction**; **a 120-gesture mixed session with zero accidental drops** |
| `luma-input` natural motion | 12 (by default the accelerometer never changes a direction; with roll compensation on, a rolled remote (±35–90°) drops and swings the right way, also with an inverted accelerometer; the pointer fit agrees with gravity; rolling the remote during play keeps drops working; 400 swings, 120 sloppy drops, a slide swing right after a drop, busy-hand drops, re-arming, the first gesture after a data pause, the pointer never taking over from the sensor) | imperfect swings as people make them: curved paths, wrist roll, no pause at the far end, a pull back that returns on the other side (a flat loop), uneven speeds, 50/60/100 Hz, in the circle and twist styles. Result: **0 rotations, 0 wrong-way moves**; 3 of 200 (circle style) and 4 of 200 (twist style) moved nothing, the smallest, slowest ~3° swings, which are as slow as drifting, and in the twist style swings with a strong wrist roll, which must not be confused with a twist. Also: 60 sloppy drops per rotation style (slightly diagonal, with a small sideways wind-up before and a sideways recoil after) all drop and never move the piece; with strong tremor and a hand that never rests, 12 of 12 drops work in both rotation styles; after a reset, gestures come back without the remote ever being perfectly still |
| `luma-app` unit tests | 9 | atomic persistence, corrupt-file isolation, settings migration from 0.1.0 and of the old slide times, records, menu navigation, remote scancodes, desktop key mapping, pointer image |
| `scripts/motion-probe-test.sh` | 11 checks | the real motion worker (`--motion-probe`) against a stand-in for the TV's `libhelpers.so.2` and mrcu service (`tests/fake-mrcu`) that starts at one sample per second: the stream must reach 50/s; a rejected `sensorType` falls back; a sleeping remote is picked up when it wakes; a stream that silently dies is renewed; a pause in the data as in button mode is waited out on the same subscription, without renewing; a service that sends no accelerometer is asked once for "all" (gravity arrives), and one that refuses "all" is returned to the usual request without a stall; denial and silence end in a clean failure after trying every connection |
| `--flow-test` (headless app) | 26 checks | onboarding skip, the full motion setup with simulated motion (hold, twist, swing, try, saved axes), start, Ready guard, button play, sliding after a drop, Back pauses, resume, save-and-quit, relaunch shows the resume prompt, continue restores paused, focus loss and gain, exit, a short motion gap keeps playing with a note, no note while the buttons are in use, lasting motion loss → options, play continues by itself when motion is back |

Result at release 0.2.14: **all pass**. The synthetic attempt tests scored 20/20 for every gesture
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
