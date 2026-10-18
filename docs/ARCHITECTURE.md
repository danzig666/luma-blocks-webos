# Architecture

## Crates

| Crate | Role | Depends on SDL/webOS? |
|---|---|---|
| `luma-core` | Deterministic engine: board, SRS, seven-bag, gravity, lock delay, hold, scoring, modes, snapshot validation | no |
| `luma-input` | Motion sample type, mrcu reply parsing, calibration, recognizers, arbitration, button mapping, traces, synthetic motion | no |
| `luma-app` | SDL2 front end: renderer, UI, audio, persistence, lifecycle, Luna client, sensor adapter, simulator | yes |

Everything a test needs (rules, gesture recognition, normalization, trace replay) lives in the two
pure crates and runs on any desktop.

## Input pipeline

```
 acquisition            normalization        frames             recognizers           arbitration       game
 ───────────            ─────────────        ──────             ───────────           ───────────       ────
 mrcu worker thread ──► mrcu::parse_* ──┐                      Twist   (twist°)
 (Luna, sensor2 or      MotionSample   ├─► AimTracker ──► Frame ─► Circle  (aim xy)  ─► GestureEngine ─► Action
  legacy)               (rad/s, m/s²,  │   calibrated       Flick   (aim vx)          ownership,        (engine)
 simulator / trace ─────► quat w,x,y,z) │   aim (screen      Drop    (aim vy)          guards, re-arm,
 SDL pointer ───────────────────────────┘   heights), twist   pointing column ──────► target column ─► legal moves
 SDL keys ─► keys::poll ─► ButtonMapper (DAS/ARR, dedupe, long press) ──────────────────────────────► Action
```

* **Acquisition** never blocks the frame loop. The sensor worker owns the Luna handle and its
  glib context on its own thread and pushes samples into a bounded queue (512, oldest dropped and
  counted).
* **Normalization**: `luma_input::mrcu` maps LG's documented fields into `MotionSample`. It uses
  monotonic arrival timestamps and explicit units. Missing fields stay `None` and implausible
  values are dropped, never zeroed. Errors are classified: unsupported, permission denied,
  asleep, duplicate subscription, too many clients, bad interval, malformed.
* **Frames**: the orientation relative to the calibrated neutral pose is projected to yaw and
  pitch. A swing-twist decomposition gives the twist about the calibrated long axis, sign-safe
  and wrap-safe. Aim is expressed in *screen heights* (SH), so pointer pixels and sensor angles
  share one threshold scale. Velocity is timestamp-aware with light exponential smoothing.
  Position is never estimated by integrating acceleration twice. A gyro-only stream is
  integrated short-term with a leak back to zero.
* **Recognizers** (`recognize.rs`) are explicit state machines: idle → candidate →
  committed/latched → re-armed.
  * *Twist*: crossing an angle threshold from a slowly adapting rest baseline commits one
    rotation. The latch releases only after returning within the re-arm angle. Holding a new
    posture re-bases it, and a later return to the old pose is recognized as a return, not an
    opposite rotation.
  * *Circle*: signed turning around the window centroid, with radius, roundness, path-length,
    duration and continuity checks. Lines, L-shapes, scribbles and partial arcs are rejected.
  * *Flick*: a short, horizontally dominant burst moves one column. Recoil in the opposite
    direction is ignored, and a burst must calm down before the next flick counts.
  * *Drop*: a high peak speed, a straight-down heading throughout, minimum travel and a short
    duration. After a commit it stays latched across piece spawns until a minimum time has
    passed *and* the remote has been still.
* **Arbitration** (`engine.rs`):
  * A circle candidate owns the trajectory and blocks flicks and drops.
  * A twist candidate blocks drops. Any candidate or recent commit freezes pointing, and pointing
    re-anchors without a jump.
  * Only one discrete action is accepted per trajectory.
  * A button press cancels candidates and adds a short guard against press jiggle.
  * After `reset()` (pause, resume, focus change, remote wake or reconnection) nothing commits
    until the remote has been still.
* **Pointing** anchor convention: the aim selects the **horizontal centre of the piece's
  occupied cells**. Column hysteresis prevents flicker. The piece moves only through legal
  one-column moves, and the ghost shows the actual reachable placement. A small target marker
  dims when the target cannot be reached. An arrow-button correction suspends pointing until a
  deliberate new pointing movement.
* **Every threshold** lives in `config.rs` with its unit and meaning. The developer screen edits
  them on the TV, and traces replay with any threshold set.

## Game engine

`luma_core::Game` is advanced only by `apply(Action)` and `tick(µs)`. Gravity uses an integer
microsecond table, the RNG is SplitMix64 with a seven-bag, and serde snapshots carry the full
state. The app runs a 120 Hz fixed step with bounded catch-up (at most 8 steps per frame). Clears
and locks are immediate; animations only decorate them. Scoring tables and T-spin rules are in one
file (`scoring.rs`).

## Platform choices (with evidence)

| Choice | Why | Evidence status |
|---|---|---|
| Rust + SDL2 renderer (OpenGL ES 2 backend), no engine | Same proven route as the owner's RemoteDesk TV on this C3; SDL's renderer is itself GLES2 on webOS | Build/ABI verified; on-TV rendering unverified for this app |
| Bundled webosbrew **SDL-webOS 2.30.12** in `lib/`, DT_RPATH `$ORIGIN/../lib` | The TV's own SDL2 uses LG's shifted event layout; the launcher's `LD_LIBRARY_PATH` would beat DT_RUNPATH | Proven by RemoteDesk TV on the C3; this app also detects the loaded layout at runtime (`keys.rs`) |
| openlgtv **buildroot-nc4** SDK (x86_64 Linux host), armv7 softfp, glibc 2.12 sysroot | Builds on CI runners. The plx-native NDK route requires macOS or arm64 Linux hosts (its `docs/building.md`) | Verified in this environment and CI |
| **Luna via `dlopen`** of `libluna-service2.so.3` + glib, own thread and glib context | No link-time webOS dependency; the app degrades to "motion unavailable" | Headers checked against SDK sysroot (LS2 3.21); on-TV behaviour unverified |
| Registration shapes tried in order: app service (app id), named (app id), anonymous, anonymous with app-id-tagged calls | Which shape the hub accepts and whether `mrcu` answers depend on the generated role file | Unverified; every attempt is logged and shown in diagnostics |
| `sensor2/getSensorEventData` (subscribe, ~100 Hz via `setSensorInterval` within reported bounds), legacy `sensor/getSensorData` (`callbackInterval` 1 = 100/s) fallback | LG documentation; sensor2 is documented for webOS TV 24+ and legacy is discouraged | Documentation only; field conventions to be confirmed with real samples |
| Fonts: Inter (OFL) | Legible at distance, licensed for distribution | n/a |

## Reference research

* **plx-native** (`github.com/GLinnik21/plx-native`, inspected at commit
  `17eb48b95157740cb5c3c3fe9f7269bc6a66635c`, 2026-10-18; GPL-3.0). **Used as a reference only;
  no code copied.** Facts taken from its docs: the LS2 registration outcomes measured on a webOS 4
  set (`rust-modules/platform/src/webos.rs`: anonymous `LSRegister(NULL)` accepted;
  application-service registration refused there), the role-file description, the `dynlib`
  runtime-binding approach, and its build-host restriction (`docs/building.md`). Its remote-key
  notes (`docs/remote-keys.md`) agree with the scancodes used here. Its Plex, media and
  networking subsystems were not used.
* **webos-rustdesk-client** (same owner): platform helpers adapted with permission. This covers
  SDL hints, keyboard-layout detection, the glibc compatibility shim, the build/ELF-audit/qemu
  smoke scripts and the deterministic IPK packager.
* **LG documentation** (fetched 2026-10-18): magic-remote reference, motion-sensor overview,
  sensor data, implementation guide and the Magic Remote guide. These give the body axes
  (X right, Y long axis toward the top, Z out of the face), gyro in rad/s with counter-clockwise
  positive, accelerometer in m/s², `gameRotationVector` as x/y/z/w, `getSensorInterval` in ms
  (10–1000), the legacy interval codes and the error codes 1001–1006 and -1.

## Data and lifecycle

* Data lives in `$HOME/data` (the app's own directory on webOS).
* Files are `settings.json`, `calibration.json`, `thresholds.json`, `records.json`,
  `savegame.json` and `traces/`. Each is a versioned envelope with atomic writes (temp file,
  fsync, rename) on a background thread. A corrupt file is renamed to `*.corrupt` and replaced by
  defaults without touching the others.
* The game is saved every ~1.5 s at lock boundaries, on pause, on focus loss, on quit-to-menu and
  on exit.
* Focus loss pauses the game, releases held buttons, resets gestures, cancels the sensor
  subscription and drops to ~10 fps. Focus gain stays paused.
* The sensor runs only where it is needed: motion play, calibration, onboarding and diagnostics.
