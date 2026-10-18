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
| Pointer | aim (when pointing is selected) | hover and click |

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
| Rotate by **twisting the wrist** (default) | twist about the remote's length, like turning a key; clockwise rotates clockwise. Come back towards your resting angle before the next twist. Holding the twist does not repeat. |
| Rotate by **drawing a circle** | a compact circle with the pointer, clockwise or anticlockwise; about one turn. The piece does not drift while you draw. |
| Move sideways by **pointing** (default) | point across the board; the piece follows to the column you aim at (the faint marker above the board). |
| Move sideways by **flicking** | a short left or right flick moves one column; repeat for more. |
| **Drop** | a quick downward flick of the wrist. Can be turned off (OK still drops). |

* Rotation and sideways styles combine freely.
* Each gesture type has its own sensitivity (0.5–2.0).
* **Gesture practice** shows each recognized gesture and how long it took to decide.
* The wrist twist needs the remote's motion sensor. Circles, flicks and pointing also work from
  the pointer alone.

## Gesture calibration guide

Run it from first-time setup, **Settings → Calibrate the remote**, or by long-pressing **Blue**.

1. **Neutral**: sit as you will play and point at the middle of the screen; hold still for about
   a second.
2. **Twist clockwise**: turn your wrist clockwise, like a key, and back. This measures which axis
   is the remote's length and which direction is clockwise for you, so either hand works.
3. **Tip up**: raise the end of the remote and come back. This measures the up/down axis.
4. **Board edges**: point at the left edge of the board and press OK, then the right edge. This
   sets how far you need to point to reach the outer columns.

Back skips any step. **Blue** (short press) or **Pause → Recenter** re-takes the neutral pose
instantly; use it after you shift on the sofa.

Tips:

* Small, crisp motions work best: a 30–45° twist, a hand-sized circle, a short flick.
* If rotations trigger by accident, lower *Rotation sensitivity*. If twists are missed, raise it.
* If pointing feels too twitchy, lower *Sideways sensitivity* (you then need to point wider), or
  recalibrate the board edges.
* If drops happen when you don't want them, lower *Drop sensitivity* or turn the drop gesture
  off.

## Troubleshooting

| Symptom | Try |
|---|---|
| "Remote motion is unavailable" | Move the remote to wake it, then **Try again**. Or choose **Play with buttons**; the game never resumes on its own. |
| Menu says "Motion not available" / "not permitted" | Your TV or remote does not offer the sensor to apps. Circles, flicks and pointing still work from the pointer; twist needs the sensor. Please report it with **Input diagnostics** (Settings → More → Developer options). |
| Piece drifts or the aim is off-centre | Recenter (Blue), or recalibrate. |
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
