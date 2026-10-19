# Controls, calibration and troubleshooting

## Buttons (always available)

| Button | In play | In menus |
|---|---|---|
| Left / Right | move (hold: auto-repeat after the *repeat delay*, then at the *repeat speed*) | change a value |
| Down | soft drop while held | next item |
| Up | rotate clockwise | previous item |
| OK (centre click) | hard drop | select |
| Back | pause menu (never quits a game) | back |
| Red | rotate anticlockwise (remappable) | |
| Green | hold piece (remappable) | |
| Blue | recenter; **long press** opens calibration | |
| Yellow | unassigned (remappable) | |
| Wheel | rotate (optional) | scroll |
| Pointer | aim (Pointing style) | hover and click (a glowing cursor appears if the TV hides its own) |

* The pause menu offers **Resume, Rotate anticlockwise, Hold, Recenter, Controls and settings,
  Restart, Save and quit**. That means every action is reachable on a remote without colour
  buttons.
* After any pause the game shows **Ready** and waits until buttons are released and the remote
  is still.
* Colour hints only appear after that button has actually been seen on your TV.
* Home, volume, input and streaming buttons are left to the TV.

## Motion (Settings → Controls → Motion and buttons)

| Setting | Gesture |
|---|---|
| Rotate by **twisting the wrist** (default) | twist about the remote's length, like turning a key; to the right rotates clockwise. Come back towards your resting angle before the next twist. Holding the twist does not repeat. |
| Rotate by **drawing a circle** | a compact, roundish circle with the remote's tip, clockwise or anticlockwise; about one turn. The piece does not drift while you draw. A swing and its pull back never count as a circle, even when they draw a flat loop. |
| Move sideways by **swinging** (default) | a quick sideways swing moves the piece; a bigger swing moves it further. The first column needs about 1.2°, the second 3.6°, then 6.4°, 9.5°, 13°…: each further column needs a little more, so small swings stay precise. **Pulling the remote back afterwards is ignored**, so you can swing, return to your comfortable position and swing again. Slow movements never move the piece. The piece moves when the swing ends (a few hundredths of a second after the remote stops or turns back), because only then is it clear that the motion was a swing and not the side of a circle or the start of a downward flick. |
| Move sideways by **pointing** | the piece follows the column you aim at (a faint marker above the board). Returning the remote moves the piece back, so this suits players who keep the remote aimed at the board. |
| Move sideways by **flicking** | a short left or right flick moves exactly one column; the pull back is ignored. |
| **Drop** | a quick downward flick of the wrist. Can be turned off (OK still drops). A small sideways movement just before the flick, and the bounce after it, never move the piece. |
| **Move after a drop** | after a drop (gesture or OK) the piece rests on the stack for a moment before it locks; a late swing, twist or button press still slides or turns it, and it drops straight down again if it slides off a ledge. A swing that is under way when the time runs out holds the piece up to 0.4 s longer, so its move still lands. A shrinking line under the piece shows the time left. Drop again to lock at once. Off / Short (0.3 s, default) / Long (0.6 s). |

* Rotation and sideways styles combine freely.
* Directions are measured along the remote's own axes, as calibrated. **Tilt compensation
  (experimental, off by default)** measures them against the room instead (the vertical comes
  from the TV's gravity data): a remote held pointing down at the floor, rolled, or both still
  swings left/right, flicks down and twists. Input diagnostics shows the measured roll and how
  far the remote points up or down ("roll: gravity … (points …)").
* A twist that stops halfway (a change of grip) is taken as the new resting posture, not as
  a gesture in progress.
* While you play with motion, a small live remote under the stats (MOTION) follows your
  movement and turns with your wrist. An arc fills as a rotation gesture builds up, and every
  recognized gesture flashes green (an arrow for a rotation, dots sliding left, right or down
  for a move or a drop). It dims while the remote is not reporting. Turn it off with
  Settings → **Remote indicator in play**.
* Each gesture type has its own sensitivity (0.5–2.0). For Swing, higher sensitivity means
  fewer degrees per column.
* **Gesture practice** shows each recognized gesture and how long it took to decide.
* The wrist twist needs the remote's motion sensor. Circles, swings, flicks and pointing also work
  from the pointer alone, but only on remotes whose motion sensor never reports: once the
  sensor has reported, gestures use only its relative motion and never the on-screen
  pointer (which stops at the screen edges).

## Motion setup

Run it from first-time setup, **Settings → Calibrate the remote**, or by long-pressing **Blue**.
Each step shows an animated example on the right and a live gauge of what the remote is doing.
The step moves on by itself as soon as it has what it needs.

1. **Hold it like you play**: point at the screen and keep still; the ring fills up.
2. **Twist to the right** and back: this measures which way is "turning right" for you, in
   either hand.
3. **Swing to the right** and back: this measures the sideways direction.
4. **Board edges** (only with the Pointing style): move the glowing dot onto the left edge of the
   board and press OK, then the right edge.
5. **Try it**: a live board. Swing, twist and flick down. Every recognized gesture is named on
   the board and ticked on the left. If something goes the wrong way, press **Red** (rotation),
   **Green** (sideways) or **Yellow** (drop) to flip it. **OK** finishes; **Back** starts over.

OK skips a measuring step and Back cancels without changing anything. **Blue** (short press) or
**Pause → Recenter** re-takes the resting pose instantly.

Tips:

* Crisp, wrist-sized motions work best: a 30–45° twist, a 5–15° swing, a short downward flick.
* If the piece moves too far per swing, lower *Sideways sensitivity*; if a small swing does
  nothing, raise it.
* If rotations trigger by accident, lower *Rotation sensitivity*. If twists are missed, raise it.
* If drops happen when you don't want them, lower *Drop sensitivity* or turn the drop gesture
  off.

## Troubleshooting

| Symptom | Try |
|---|---|
| "Motion paused: move the remote (buttons still work)" above the board | The remote stopped reporting motion. A button press does this on the Magic Remote (it switches to button mode until it is moved), so the note waits 2.5 s after your last button press. Move the remote and motion is back at once; the first gesture counts. |
| "Remote motion is unavailable" | Motion stayed away for five seconds while no buttons were used. Move the remote to wake it: the game carries on by itself (through Ready) as soon as motion is back. Or choose **Try again** or **Play with buttons**. The grey "Motion: …" line says what failed and how often the stream had to reconnect. |
| Drops (or all gestures) stop working after a while | Fixed in 0.2.4: gestures no longer wait for the remote to be perfectly still before they work again. If it still happens, open Input diagnostics and note the drop state. |
| "The remote's motion sensor is not answering" | Move the remote. The grey "Motion: …" line under the message says which step failed; please send a photo of it. The app keeps retrying every few seconds. |
| Menu says "Motion not available" / "not permitted" | Your TV or remote does not offer the sensor to apps. Circles, flicks and pointing still work from the pointer; twist needs the sensor. Please report it with **Input diagnostics** (Settings → More → Developer options). |
| The piece follows the remote back | Choose **Swinging** instead of Pointing (Settings → Move sideways by). |
| Pointing aim is off-centre | Recenter (Blue), or run the motion setup again. |
| Pointing (or the aim dot) moves the wrong way for the first moment | The app learns how your TV reports the remote's orientation from the first few movements; move the remote around a little. Input diagnostics shows "orientation vs gyro". |
| Buttons do nothing in play | The game waits for "Ready" after a pause: release all buttons and hold the remote still. |
| Screen went dark | Menus dim after the set time and fade to black; press any button (that press only wakes the screen). |
| Saved game missing after an update | Please report it; see the TV checklist. |

## Input diagnostics (developer options)

Turn on **Settings → More settings → Developer options**. Two extra menu entries appear.

**Input diagnostics** shows:

* every delivered key event (code, repeat, timing), pointer and wheel events;
* the motion backend: state, registration, interval, measured rate, freshness, fields and
  errors;
* the raw sample and calibrated aim/twist, a live remote indicator, recognizer states,
  recognized gestures and decision times;
* where the data comes from (hardware, simulated or trace).

In this screen: Red records a trace, Green restarts motion, Blue recenters, and Yellow hides the
system cursor to test whether the sensor keeps streaming.

**Developer** edits every threshold (saved to `thresholds.json`), toggles the legacy quaternion
order, and replays the last trace with the current settings.
