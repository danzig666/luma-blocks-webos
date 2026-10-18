# Compatibility

**No TV has run this app yet.** Everything below says what it rests on.

## Verified (this environment and CI)

| Item | How |
|---|---|
| Engine rules, scoring, seven-bag, lock delay, save/restore | 32 unit tests (`luma-core`) |
| Gesture recognition for all four profiles, arbitration, button mapping, mrcu parsing of LG's documented examples, traces | 55 tests (`luma-input`), synthetic motion at 50/60/100 Hz with tremor |
| Persistence (atomic, corrupt-file isolation), input decoding, menus | `luma-app` unit tests and the headless `--flow-test` |
| Cross-build for armv7 softfp, ELF/ABI rules for webOS ≥ 5 | `scripts/elf-audit.sh` (local and CI) |
| ARM binary starts and self-test passes | qemu-arm on Debian armel glibc 2.36 (local and CI) |
| IPK structure, both identities | `scripts/package-ipk.py`; CI also runs `webosbrew-ipk-verify` against firmware symbol dumps (result in the workflow summary) |
| Rendering of every screen | desktop simulator (OpenGL / software), screenshots in `docs/screenshots` |

## Not yet verified (needs hardware)

| Item | Target |
|---|---|
| Launch, render, 60 fps, audio | LG C3, webOS 10.2.2, O22N (owner's rooted TV) |
| Remote button codes in this app (D-pad, OK, Back, colours, wheel, OK as key and/or click) | C3 |
| Luna registration shape accepted for this app id; `com.webos.service.mrcu` access from an ordinarily launched app | C3 rooted **and** a genuinely non-rooted Developer Mode TV |
| sensor2 vs legacy availability, delivered rate, field names, quaternion order, axis signs | C3 (webOS 10 ≈ webOS TV 25: sensor2 expected per LG docs) |
| Effect of hiding the system cursor on sensor/pointer delivery | C3 |
| Gesture accuracy (target ≥ 19/20 per gesture and direction) and zero accidental drops in a 10-minute mixed session | C3, both hands, seated |
| 20-minute session with pauses, persistence across relaunch and app update | C3 |
| webOS 5–9 TVs (legacy sensor API, older SDL) | testers wanted |

## Known limitations

* Wrist twist needs the remote's motion sensor; pointer-only fallback offers circles, flicks and
  pointing.
* The sensor's heading drifts over time (no magnetometer, per LG). Recenter with Blue or from the
  pause menu.
* Render resolution is a 1920×1080 logical layout scaled to the panel; no 4K rendering is
  claimed.
* The app does not claim to prevent burn-in. It dims and fades idle screens and leaves the TV's
  own panel protection untouched.
