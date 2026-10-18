# Luma Blocks 0.1.0 (test build)

A minimalist falling-block puzzle for LG webOS TVs with Magic Remote motion controls.

**This is a pre-release.** The app has been built, ABI-audited and smoke-tested under ARM
emulation, and its game and gesture logic is covered by automated tests. It has **not yet been
run on a TV**. Motion sensing on real remotes, button codes and performance still need checking
([TV checklist](https://github.com/danzig666/luma-blocks-webos/blob/v0.1.0/docs/TV_CHECKLIST.md)).

## Downloads

* `io.github.danzig666.lumablocks_0.1.0_arm.ipk`: **Luma Blocks**
* `io.github.danzig666.lumablocks.debug_0.1.0_arm.ipk`: **Luma Blocks (Dev)**, a separate
  identity with separate saves, for testing
* `SHA256SUMS.txt`, plus the Homebrew Channel manifest

Install: Developer Mode (`ares-install`) or Homebrew Channel / Dev Manager. See
[INSTALL](https://github.com/danzig666/luma-blocks-webos/blob/v0.1.0/docs/INSTALL.md).

## What's in it

* **Modes**: Marathon, Sprint (40 lines), Zen, Gesture practice. The game is always resumable:
  leave mid-game and choose **Continue** or **New game** next time.
* **Motion controls**:
  * Wrist twist or circle to rotate; pointing or flicking to move; a downward flick to drop.
  * All four style combinations, per-gesture sensitivity, guided calibration and a recenter
    button.
* **Full button controls**: D-pad, OK hard drop, Back pause, colour shortcuts (remappable), and a
  pause menu that offers every action.
* **Look**: true black, softly glowing pieces, short drop trails and quick line-clear sweeps.
  Idle screens dim to protect the OLED.
* **For testers**: Settings → More → Developer options adds **Input diagnostics** (button codes,
  sensor stream, rate, recognized gestures) and trace recording. Please send traces and
  screenshots.
