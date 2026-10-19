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
| Rotate by **drawing a circle** | a compact circle with the remote's tip, clockwise or anticlockwise; about one turn. The piece does not drift while you draw. |
| Move sideways by **swinging** (default) | a quick sideways swing moves the piece; a bigger swing moves it further (about 2.4° per column). **Pulling the remote back afterwards is ignored**, so you can swing, return to your comfortable position and swing again. Slow movements never move the piece. |
| Move sideways by **pointing** | the piece follows the column you aim at (a faint marker above the board). Returning the remote moves the piece back, so this suits players who keep the remote aimed at the board. |
| Move sideways by **flicking** | a short left or right flick moves exactly one column; the pull back is ignored. |
| **Drop** | a quick downward flick of the wrist. Can be turned off (OK still drops). |

* Rotation and sideways styles combine freely.
* Each gesture type has its own sensitivity (0.5–2.0). For Swing, higher sensitivity means
  fewer degrees per column.
* **Gesture practice** shows each recognized gesture and how long it took to decide.
* The wrist twist needs the remote's motion sensor. Circles, swings, flicks and pointing also work
  from the pointer alone.

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
| "Remote motion is unavailable" | Move the remote to wake it, then **Try again**. Or choose **Play with buttons**; the game never resumes on its own. |
| "The remote's motion sensor is not answering" | Move the remote. The grey "Motion: …" line under the message says which step failed; please send a photo of it. The app keeps retrying every few seconds. |
| Menu says "Motion not available" / "not permitted" | Your TV or remote does not offer the sensor to apps. Circles, flicks and pointing still work from the pointer; twist needs the sensor. Please report it with **Input diagnostics** (Settings → More → Developer options). |
| The piece follows the remote back | Choose **Swinging** instead of Pointing (Settings → Move sideways by). |
| Pointing aim is off-centre | Recenter (Blue), or run the motion setup again. |
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
