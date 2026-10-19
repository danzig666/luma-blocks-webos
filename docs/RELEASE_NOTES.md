# Luma Blocks 0.2.0 (test build)

A minimalist falling-block puzzle for LG webOS TVs with Magic Remote motion controls.

**Pre-release.** Built, ABI-audited and smoke-tested under ARM emulation; game and gesture logic
covered by automated tests. On-TV feedback from 0.1.0 drove this release; motion behaviour on
real remotes still needs confirming
([TV checklist](https://github.com/danzig666/luma-blocks-webos/blob/v0.2.0/docs/TV_CHECKLIST.md)).

## What changed since 0.1.0

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

* `io.github.danzig666.lumablocks_0.2.0_arm.ipk`: **Luma Blocks**
* `io.github.danzig666.lumablocks.debug_0.2.0_arm.ipk`: **Luma Blocks (Dev)**, a separate
  identity with separate saves
* `SHA256SUMS.txt`, plus the Homebrew Channel manifest

Install: Developer Mode (`ares-install`) or Homebrew Channel / Dev Manager. See
[INSTALL](https://github.com/danzig666/luma-blocks-webos/blob/v0.2.0/docs/INSTALL.md).

**For testers:** please run Settings → Calibrate the remote first. If gestures still feel wrong,
turn on Settings → More → Developer options, open **Input diagnostics**, record a trace with Red
while doing the gesture a few times, and send the trace file (`scripts/tv.sh traces`) and a photo
of the screen.
