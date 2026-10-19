# Luma Blocks 0.2.11 (test build)

A minimalist falling-block puzzle for LG webOS TVs with Magic Remote motion controls.

**Pre-release.** Built, ABI-audited and smoke-tested under ARM emulation; game and gesture logic
covered by automated tests. On-TV feedback drove this release; motion behaviour on real remotes
still needs confirming
([TV checklist](https://github.com/danzig666/luma-blocks-webos/blob/v0.2.11/docs/TV_CHECKLIST.md)).

## What changed since 0.2.10

Feedback: with the remote held rolled, a flick down still was not a drop, although LG's own
pointer clearly knew which way was down.

* **The roll is now learned from LG's pointer.** The firmware's pointer already moves in room
  directions however the remote is held. The app compares its own motion with the pointer's
  whenever the remote moves a little and turns its motion to match; gravity from the
  accelerometer is only the stand-in until then. Input diagnostics shows both.
* **Changing grip no longer blocks drops.** Rolling the remote slowly into a new grip left the
  twist detector waiting halfway, which blocked drops. A twist that stops halfway now counts as
  a change of grip, and only a twist that is actually turning blocks a drop.

## Earlier: 0.2.10 (since 0.2.9)

* **Directions follow the room, however the remote is held.** Held rolled (turned about its
  length), a flick down used to look diagonal or even sideways to the game: at 90° it moved
  the piece instead of dropping it. The motion is now turned by the remote's roll, measured
  from gravity, so a flick down always drops and a swing always moves left or right. Input
  diagnostics shows the roll.

## Earlier: 0.2.9 (since 0.2.8)

* **Motion is purely relative.** Gestures could switch to the TV's on-screen pointer whenever
  the motion sensor paused for a moment (button mode, a short gap). The pointer stops at the
  screen edges, so swings went missing near an edge, and every switch reset the gestures. Once
  the remote's motion sensor has reported, gestures now use only its relative motion; the
  pointer is still used in the menus, and for gestures only on remotes whose sensor never
  reports.

## Earlier: 0.2.8 (since 0.2.7)

* **Live remote indicator while playing with motion.** A small remote under the stats
  (MOTION) follows your movement and turns with your wrist, like in Input diagnostics. An arc
  fills as a rotation gesture builds up (twist or circle), and each recognized gesture
  flashes green: an arrow for a rotation, dots sliding left, right or down for a move or a
  drop. It dims while the remote is not reporting. Settings → "Remote indicator in play"
  turns it off.

## Earlier: 0.2.7 (since 0.2.6)

Feedback: using a button in motion mode immediately "broke" motion, with the reconnecting
note flashing above the board.

* **Buttons and motion mix again.** A button press puts the Magic Remote in button mode: it
  stops reporting motion until it is moved. The app treated that as a broken connection,
  renewed the subscription and made gestures wait for a calm moment. Now:
  * the subscription is kept while the remote reports itself awake; data resumes on it the
    moment the remote moves (it is renewed only after 4 s, 8 s, 16 s… without data, in case
    it really died);
  * gestures stay armed through the pause, so the very first movement counts (a movement
    already under way when data returns still waits for calm);
  * the note ("Motion paused: move the remote") waits 2.5 s after the last button press, so
    it no longer flashes while you use the buttons.

## Earlier: 0.2.6 (since 0.2.5)

* **Sliding a dropped piece with a swing works.** 0.2.5 kept swings off for the whole
  drop recoil, which was longer than the slide time, so only buttons could still move a
  dropped piece. Now only the drop's wind-up and the start of its recoil are protected.
  * A swing takes about 0.4 s after the drop to land (flick, recoil, swing). The slide time
    is therefore longer: Short is now 0.3 s (default) and Long 0.6 s; saved choices move up
    automatically.
  * A swing that is under way when the time runs out holds the piece up to 0.4 s longer, so
    its move still lands.
  * Sloppy drops still never move the piece sideways (120/120).

## Earlier: 0.2.5 (since 0.2.4)

Feedback: dropping did not work well, and the piece sometimes moved sideways before it fell.

* **A downward flick never moves the piece sideways.**
  * A small sideways movement just before the flick used to count as a swing. Swing moves
    now come when the swing ends (a few hundredths of a second after the remote stops or
    turns back), and a stroke that turns into a downward flick is dropped.
  * The bounce after a flick can no longer move the piece during the short "move after a
    drop" time.
* **Drops in the circle style were sometimes swallowed.** A sideways wind-up followed by the
  flick looks like the start of a circle, and that blocked the drop. A circle in progress no
  longer blocks drops: a drop needs a nearly straight, fast downward stroke, which a circle's
  side is not.
* A swing cut short (for example by a wrist roll in the twist style) can no longer let its
  pull back move the piece the other way.
* **New test**: 120 sloppy drops (slightly diagonal, with a sideways wind-up before and a
  sideways recoil after) all drop and never move the piece, in both rotation styles.

## Earlier: 0.2.4 (since 0.2.3)

Feedback on 0.2.3: after a while a remote error appeared and was immediately covered by a
menu; drop by flicking worked poorly and after a few minutes not at all.

* **Drops (and gestures in general) no longer switch themselves off.**
  * After a drop, the next drop needed the remote to be completely still first. With a busy
    or slightly shaky hand that rarely happens in play, so drops stopped working. Now it is
    enough that the remote has stopped moving down.
  * After a pause or a motion hiccup, all gestures waited for complete stillness before
    working again; the first hiccup could switch them off for good. Now the wait relaxes over
    about two seconds and ends after 2.5 s at the latest.
* **Motion hiccups no longer interrupt play.**
  * A short gap in the motion stream shows "Remote motion reconnecting… buttons still work"
    above the board, and play goes on. The app notices a stalled stream after about a second
    and re-establishes it (about two seconds in all).
  * Play stops only when motion stays away for five seconds while no buttons are used. The
    message then stays readable, and play continues by itself (through Ready) once motion is
    back, with a short "Remote motion is back" note.
  * The grey "Motion: …" line counts how often the stream had to reconnect.
* **New tests**: drops with a never-resting, shaky hand (12/12), gestures returning after
  resets, and a stand-in stream that silently dies and must be re-established.

## Earlier: 0.2.3 (since 0.2.2)

* **Pointing went the wrong way** (piece and aim dot mirrored left/right). The Pointing style
  takes absolute aim from the remote's orientation, while swings, the motion setup and the
  other gestures use the gyroscope. On this TV the orientation evidently arrives in the
  opposite convention. The app now compares the two while the remote moves and flips the
  orientation's left/right and up/down to match the gyro, so pointing and the aim dot follow
  the remote. It learns this from the first ~25° of movement, and Input diagnostics shows it
  ("orientation vs gyro").

## Earlier: 0.2.2 (since 0.2.1)

Feedback on 0.2.1 (motion now works on the TV): circle rotation fired during sideways swings;
swings sometimes did not move and sometimes moved too far; a dropped piece could not be nudged.

* **Circle rotation no longer fires on swings.**
  * A circle must now be roundish. A swing and its pull back draw a flat loop, which no longer
    counts as a circle.
  * A swing that moved the piece starts the circle count afresh.
* **Swings move more reliably.**
  * In the circle style, a swing that curves (as real swings do) is no longer mistaken for the
    start of a circle. The piece now moves when the swing ends, about a tenth of a second
    later, once it is clear that the motion was not the side of a circle.
  * The wrist roll that comes with a forearm swing no longer blocks it in the circle style.
  * If a swing is rejected, its pull back can no longer move the piece the other way.
* **Swing distance is more even.** The first columns come easily (about 1.2° and 3.6°), and each
  further column needs a little more swing, so small swings stay precise and big swings no
  longer overshoot.
* **Move after a drop** (new, Settings → Controls). After a drop the piece rests on the stack for
  0.2 s before it locks. A late swing, twist or button press still slides or turns it, and it
  drops straight down again if it slides off a ledge. A shrinking line under the piece shows
  the time left. Choose Off, Short (0.2 s, default) or Long (0.4 s); drop again to lock at once.
* **New test**: 400 imperfect, natural swings (curved paths, wrist roll, looping pull backs,
  uneven speeds) give 0 rotations and 0 wrong-way moves in both rotation styles.

## Earlier: 0.2.1 (since 0.2.0)

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

* `io.github.danzig666.lumablocks_0.2.11_arm.ipk`: **Luma Blocks**
* `io.github.danzig666.lumablocks.debug_0.2.11_arm.ipk`: **Luma Blocks (Dev)**, a separate
  identity with separate saves
* `SHA256SUMS.txt`, plus the Homebrew Channel manifest

Install: Developer Mode (`ares-install`) or Homebrew Channel / Dev Manager. See
[INSTALL](https://github.com/danzig666/luma-blocks-webos/blob/v0.2.11/docs/INSTALL.md).

**For testers:** please run Settings → Calibrate the remote first. If gestures still feel wrong,
turn on Settings → More → Developer options, open **Input diagnostics**, record a trace with Red
while doing the gesture a few times, and send the trace file (`scripts/tv.sh traces`) and a photo
of the screen.
