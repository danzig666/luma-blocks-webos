# On-TV verification checklist

First target: LG OLED C3, webOS 10.2.2, O22N, rooted. Repeat the access checks on a genuinely
non-rooted Developer Mode TV before claiming Developer Mode support. Install the **Dev** identity
for testing. Turn on Settings → More → Developer options, then use **Input diagnostics**.

Record results (with the diagnostics screen photographed where useful) in
`docs/evidence/<date>-<tv>.md`. A root-shell `luna-send` success does **not** count: test the
app as launched normally.

## 1. Install and launch

- [ ] Install via Homebrew Channel / `scripts/tv.sh install`; the launcher tile shows the icon.
- [ ] The black splash shows, then the first-run screen, at full resolution. Log line
      `renderer: …` reports `opengles2`.
- [ ] Exit with the Exit button and with Home; relaunch works.
- [ ] Developer Mode TV: the same steps with `ares-install`.

## 2. Buttons (Input diagnostics, left column)

- [ ] D-pad, OK, Back, Red, Green, Yellow, Blue, CH+/- arrive (`sc482` Back, `sc486–489` colours).
- [ ] Holding a D-pad key: do repeats arrive (`(repeat)`), and at what interval? Does key-up
      always arrive?
- [ ] OK with the pointer visible: key, mouse click, or both? (Gameplay de-duplicates both.)
- [ ] Wheel events and their sign.
- [ ] Long Back does not close the app.

## 3. Motion stream (middle column)

- [ ] **State**: streaming? Note the **registration** shape that worked, and every failed shape
      from the Developer screen's backend log.
- [ ] **Backend**: sensor2 or legacy; API version.
- [ ] Interval current/min/max, requested, and the **measured rate** (target 50–100 Hz) and
      freshness.
- [ ] **Fields** delivered (orientation, gyro, accel, linear accel, cursor).
- [ ] Axis conventions: aim right should give positive *aim yaw*; tip up positive *pitch*; a
      clockwise wrist twist positive *twist* after calibration. Check the raw gyro sign for each
      motion.
- [ ] Let the remote sleep: the state becomes "remote asleep" and recovers on its own when moved
      (stream restarts count up).
- [ ] Yellow (hide cursor): does the stream keep going? Does the SDL pointer stop?
- [ ] Record a trace (Red) of each gesture, ten natural attempts each, and copy the traces off
      with `scripts/tv.sh traces`.

## 4. Gameplay

- [ ] Calibration completes; the twist step reports the expected axis.
- [ ] For each profile (twist+point, twist+flick, circle+point, circle+flick), seated, each hand:
      count 20 deliberate attempts per gesture and direction. **Target ≥ 19/20.** Also record false
      positives.
- [ ] A 10-minute mixed session: **target zero accidental hard drops**.
- [ ] Rotating does not drag the piece sideways; after a circle the piece is back in its column.
- [ ] A button press during motion play does not trigger a gesture.
- [ ] Pause, resume (Ready waits for stillness), focus change (Home, then back), and motion loss
      → "Remote motion is unavailable" → Try again / Play with buttons.
- [ ] A 20-minute continuous session with pauses: frame timing (diagnostics footer), no stutter,
      no memory growth (`top` over SSH).
- [ ] Quit mid-game, relaunch → "Pick up where you left off?" → Continue restores the board,
      paused.
- [ ] Install an update over the existing app: do settings, records and the saved game survive?
- [ ] Idle: menus dim after the set time, then go black; the first button only wakes the screen.
      Play auto-pauses when idle.
- [ ] Sound: effects play at the set volume; mute works.

## 5. Report

Note the TV model, webOS version, firmware and remote model, the install route, the motion
backend and rate, the accuracy counts, false positives and latency observations. These are
acceptance targets that need evidence; nothing here is to be marked passed without it.
