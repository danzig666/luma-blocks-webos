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
  legacy)               (rad/s, m/s²,  │   calibrated       Swing   (aim vx)          ownership,        (engine)
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
* **Frames**: motion comes from the **gyroscope** whenever it is delivered (a direct
  body-frame rate with documented axes); its units are cross-checked against the orientation at
  runtime (rad/s per LG, deg/s detected automatically). The yaw and pitch rates are turned by
  the remote's roll about its length, so a remote held rolled still moves in room directions.
  The roll comes from gravity (accelerometer, smoothed, only while it reads about 1 g). A
  decaying complex fit of LG's pointer steps (`coordinate`) against our gyro steps gives the
  same rotation as seen by the firmware; it is shown in diagnostics for comparison only,
  because using it on the TV (0.2.11) broke sideways movement. The orientation's yaw and pitch are
  compared with the gyro's while the remote moves; if the firmware reports the rotation vector
  in the opposite convention (which mirrors pointing), the absolute aim is flipped to match. Short-term aim is integrated from the gyro
  with a slow leak. For the Pointing style, absolute aim comes from the orientation relative to
  the calibrated neutral pose, projected to yaw and
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
  * *Circle*: signed turning around the window centroid, with radius, radius variation,
    path-length, duration and continuity checks, plus **roundness** (minor over major spread of
    the points, `circle_min_axis_ratio`; a partial arc needs a little under half of it to become
    a candidate). Lines, L-shapes, scribbles, partial arcs and the flat loop of a swing with its
    pull back are rejected.
  * *Swing / Flick* (`Swing`): a quick, horizontally dominant stroke measured from where the
    movement began. Swing moves by `swing_columns` of the travel (the first column early, each
    further one `swing_col_growth` wider); Flick moves one column per stroke. The columns are
    held until the stroke ends and dropped if it curves away at speed (the side of a circle or
    the wind-up of a downward flick keeps its speed, a swing slows down before it stops or
    turns back). A drop candidate owns the motion and the post-commit freeze covers the start
    of its recoil, so neither moves the piece; a deliberate swing after that may still slide
    the dropped piece, and the app holds the lock while that swing is under way. A rejected stroke still opens a return window, so its pull back
    never moves the piece the other way. When the stroke ends, opposite motion within a window and
    up to 1.6× the stroke's travel is the **return pull** and moves nothing; the window closes
    early when the remote rests or the player continues in the same direction. Slow motion never
    starts a stroke. This replaces absolute pointing as the default because players naturally pull
    the remote back after moving it.
  * *Drop*: a high peak speed, a straight-down heading throughout, minimum travel and a short
    duration. After a commit it stays latched across piece spawns until a minimum time has
    passed *and* the remote has stopped moving down (resting, recoiling or moving sideways).
* **Arbitration** (`engine.rs`):
  * A twist candidate blocks drops only while the wrist is actually turning; a twist that
    stops halfway is adopted as a new posture (change of grip). A circle candidate does not (a drop needs a nearly
    straight, fast downward stroke, which a circle's side is not, and a small sideways wind-up
    before a flick often looks like the start of a circle). A circle candidate does not block a held swing
    stroke (a curved swing looks like the start of a circle); a stroke that moved the piece
    restarts the circle window, so a swing's out-and-back loop never adds up to a turn.
  * A twist candidate blocks drops. Any candidate or recent commit freezes pointing, and pointing
    re-anchors without a jump.
  * Only one discrete action is accepted per trajectory.
  * A button press cancels candidates and adds a short guard against press jiggle.
  * Once the sensor has delivered a sample, the system pointer never drives gestures again
    (it is clamped at the screen edges and, as a second source, used to take over whenever
    the sensor paused, resetting the gestures each time). It remains the fallback only for
    remotes whose sensor never reports.
  * A pause in the sensor data (the Magic Remote stops reporting after a button press until
    it is moved) only resets the recognizers; gestures stay armed, so the movement that
    resumes the data counts. If the data resumes in the middle of a fast movement (a radio
    gap), that movement waits for calm instead. The worker keeps the subscription through
    such pauses while the remote reports itself awake and renews it only after 4 s, then
    8 s, 16 s… without data.
  * After `reset()` (pause, resume, focus change, remote wake or reconnection) nothing commits
    until the remote has been calm. "Calm" starts as stillness and relaxes over about two
    seconds; after 2.5 s gestures re-arm regardless, so a shaky hand never leaves them off.
* **Calibration** probes measure the rotation axis of each guided motion from the integrated
  gyro at its largest excursion (so out-and-back motions read correctly), and the try-it step
  can flip the twist, sideways and drop directions individually.
* **Pointer image**: on webOS the app installs its own arrow as the SDL cursor. SDL-webOS
  builds its default arrow from LG's cursor PNGs; when those cannot be loaded, the pointer
  vanishes for good after the first time the game hides it. With our own image, hiding during
  play and showing again in menus is reliable.
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
| **Luna via `dlopen`**, first through the TV's own `libhelpers.so.2` (`HLunaServiceCall`, the path SDL-webOS uses for its own system calls), then `libluna-service2.so.3` + glib on our own thread and glib context, then the `luna-send-pub` / `luna-send` commands | No link-time webOS dependency; the app degrades to "motion unavailable" | Headers checked against the SDK sysroot (LS2 3.21, webos-helpers); tested on the host against a stand-in service; on-TV behaviour unverified |
| Connections tried in order: helper (public, private), app service (app id), named (app id), anonymous, anonymous with app-id-tagged calls, `luna-send-pub`, `luna-send`. A connection counts once `sensor2/getSensorState` answers; no reply within 2 s or a denial moves on, and after all fail the app retries every 8 s | Which path the hub accepts and whether `mrcu` answers depend on the generated role file and jail | Unverified; every attempt is logged, shown in diagnostics and printed by `--motion-probe` |
| `sensor2/getSensorEventData` (subscribe; `sensorType` falls back to `"all"` if rejected), then ~100 Hz via `setSensorInterval` **after** the subscription is confirmed (LG's sample does the same; the service starts at 1000 ms) and again whenever the measured rate is too low; legacy `sensor/getSensorData` (`callbackInterval` 1 = 100/s) fallback | LG documentation; sensor2 is documented for webOS TV 24+ and legacy is discouraged | Documentation only; field conventions to be confirmed with real samples |
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
