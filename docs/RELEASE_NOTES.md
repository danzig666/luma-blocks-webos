# Luma Blocks 0.2.1 (test build)

A minimalist falling-block puzzle for LG webOS TVs with Magic Remote motion controls.

**Pre-release.** Built, ABI-audited and smoke-tested under ARM emulation; game and gesture logic
covered by automated tests. On-TV feedback drove this release; motion behaviour on real remotes
still needs confirming
([TV checklist](https://github.com/danzig666/luma-blocks-webos/blob/v0.2.1/docs/TV_CHECKLIST.md)).

## What changed since 0.2.0

Feedback on 0.2.0: the remote's motion sensor was always reported as not answering, and the
pointer did not show.

* **Motion sensor connection reworked.**
  * The app now reaches the motion service through the TV's own helper library first (the one
    SDL already uses for its system calls), then its own Luna registrations, then the
    `luna-send` commands on rooted TVs. Each path is checked with LG's documented
    `getSensorState` call and abandoned after 2 s without an answer.
  * The sample rate is set **after** the subscription is confirmed, as LG's own sample does.
    The service starts at one sample per second, and 0.2.0 asked for 100 per second too early.
    The app now also re-requests the rate whenever the measured stream is too slow.
  * If the service rejects the requested sensor types, the app asks for `"all"`.
  * After every path fails, it retries every 8 seconds on its own.
* **Pointer**: the app now supplies its own arrow image. Before, when the TV's arrow image
  could not be loaded, the pointer vanished for good after the first game hid it.
* **Diagnosable on the spot**: the "not answering" and "motion unavailable" screens show a grey
  "Motion: …" line naming the failing step. `scripts/tv.sh motionprobe` (or
  `luma-blocks --motion-probe` over SSH) prints every connection step and the measured rate.

## Earlier: 0.2.0 (since 0.1.0)

* **Sideways movement redesigned: Swing (new default).**
  * A quick sideways swing moves the piece as far as you swing, and **pulling the remote back is
    ignored**. Before, absolute pointing moved the piece back with the remote.
  * Slow movements never move a piece.
  * Pointing and one-column Flicking remain as options; Flicking now also ignores the pull back.
  * Existing installs switch to Swing automatically.
* **Motion from the gyroscope.** Gestures now use the remote's gyroscope directly; its units are
  checked against the orientation automatically.
* **New motion setup**:
  * Hold still → twist right → swing right → (board edges for Pointing) → try it.
  * Every step has an animated example and a live gauge, and moves on by itself.
  * The last step is a live board where a reversed rotation, sideways or drop direction is fixed
    with one colour button.
  * A clearly visible aim dot replaces the invisible pointer in the edge steps.
* **App-drawn cursor** when the TV's own pointer is hidden or silent.
* **Much more animation**:
  * Pieces glide between cells and visibly turn when rotated.
  * Cleared rows flash and dissolve from the centre while the stack falls into place.
  * Landing dust, a scrolling next queue, a hold pop, a counting score and a level-up pulse.
  * Game over greys the board from the top.
  * Menus fade in, the focus glides and items slide in; dim blocks drift behind the main menu;
    Settings shows the selected gesture animated.
  * Reduced motion turns most of this off.
* **Fixes**: an OK press at game over no longer starts a new game instantly. The tail of a
  circle no longer nudges the piece sideways. Flicks commit before their recoil.

## Downloads

* `io.github.danzig666.lumablocks_0.2.1_arm.ipk`: **Luma Blocks**
* `io.github.danzig666.lumablocks.debug_0.2.1_arm.ipk`: **Luma Blocks (Dev)**, a separate
  identity with separate saves
* `SHA256SUMS.txt`, plus the Homebrew Channel manifest

Install: Developer Mode (`ares-install`) or Homebrew Channel / Dev Manager. See
[INSTALL](https://github.com/danzig666/luma-blocks-webos/blob/v0.2.1/docs/INSTALL.md).

**For testers:** please run Settings → Calibrate the remote first. If gestures still feel wrong,
turn on Settings → More → Developer options, open **Input diagnostics**, record a trace with Red
while doing the gesture a few times, and send the trace file (`scripts/tv.sh traces`) and a photo
of the screen.
