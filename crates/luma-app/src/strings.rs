//! Every user-facing string, in one place for later translation. English only for now.
#![allow(dead_code)]

pub const APP_TITLE: &str = "Luma Blocks";

// Main menu
pub const CONTINUE: &str = "Continue";
pub const MARATHON: &str = "Marathon";
pub const SPRINT: &str = "Sprint · 40 lines";
pub const ZEN: &str = "Zen";
pub const PRACTICE: &str = "Gesture practice";
pub const SETTINGS: &str = "Settings";
pub const RECORDS: &str = "Records";
pub const DIAGNOSTICS: &str = "Input diagnostics";
pub const DEVELOPER: &str = "Developer";
pub const EXIT: &str = "Exit";

pub const MARATHON_HINT: &str = "Endless. Speed rises every ten lines.";
pub const SPRINT_HINT: &str = "Clear forty lines as fast as you can.";
pub const ZEN_HINT: &str = "Gentle and unranked. Play at your own pace.";
pub const PRACTICE_HINT: &str = "Try the gestures. No gravity, no game over.";

// Resume
pub const RESUME_TITLE: &str = "Pick up where you left off?";
pub const NEW_GAME: &str = "New game";
pub const BACK_TO_MENU: &str = "Main menu";
pub const REPLACE_SAVE_TITLE: &str = "Start a new game?";
pub const REPLACE_SAVE_BODY: &str = "Your saved game will be replaced.";
pub const START_NEW: &str = "Start new game";
pub const KEEP_SAVED: &str = "Keep saved game";

// Pause
pub const PAUSED: &str = "Paused";
pub const RESUME: &str = "Resume";
pub const ROTATE_CCW: &str = "Rotate anticlockwise";
pub const HOLD: &str = "Hold piece";
pub const RECENTER: &str = "Recenter";
pub const CONTROLS_SETTINGS: &str = "Controls and settings";
pub const RESTART: &str = "Restart";
pub const QUIT_TO_MENU: &str = "Save and quit to menu";
pub const PAUSED_IDLE: &str = "Paused while you were away";
pub const PAUSED_FOCUS: &str = "Paused";
pub const GET_READY: &str = "Ready";

// Motion loss
pub const MOTION_LOST_TITLE: &str = "Remote motion is unavailable";
pub const MOTION_LOST_BODY: &str =
    "Move the remote to wake it, then try again. You can also keep playing with the buttons.";
pub const TRY_AGAIN: &str = "Try again";
pub const PLAY_WITH_BUTTONS: &str = "Play with buttons";

// Game over
pub const GAME_OVER: &str = "Game over";
pub const SPRINT_DONE: &str = "Forty lines";
pub const BOARD_FULL: &str = "Board full";
pub const START_FRESH: &str = "Start fresh";
pub const PLAY_AGAIN: &str = "Play again";
pub const NEW_BEST: &str = "New best";

// HUD
pub const HUD_HOLD: &str = "HOLD";
pub const HUD_NEXT: &str = "NEXT";
pub const HUD_SCORE: &str = "SCORE";
pub const HUD_LEVEL: &str = "LEVEL";
pub const HUD_LINES: &str = "LINES";
pub const HUD_TIME: &str = "TIME";
pub const HUD_LEFT: &str = "TO GO";
pub const HUD_BEST: &str = "BEST";

// Settings
pub const CONTROL: &str = "Controls";
pub const CONTROL_MOTION: &str = "Motion and buttons";
pub const CONTROL_BUTTONS: &str = "Buttons only";
pub const ROTATION: &str = "Rotate by";
pub const ROT_TWIST: &str = "Twisting the wrist";
pub const ROT_CIRCLE: &str = "Drawing a circle";
pub const SIDEWAYS: &str = "Move sideways by";
pub const SIDE_SWING: &str = "Swinging";
pub const SIDE_POINT: &str = "Pointing";
pub const SIDE_FLICK: &str = "Flicking";
pub const SIDE_SWING_HINT: &str = "Pulling back is ignored";
pub const SIDE_POINT_HINT: &str = "The piece follows where you point";
pub const SIDE_FLICK_HINT: &str = "One column per flick";
pub const DROP_GESTURE: &str = "Drop with a downward flick";
pub const DROP_SLIDE: &str = "Move after a drop";
pub const DROP_SLIDE_OFF: &str = "Off (locks at once)";
pub const DROP_SLIDE_SHORT: &str = "Short";
pub const DROP_SLIDE_LONG: &str = "Long";
pub const DROP_SLIDE_HELP: &str = "After a drop the piece rests on the stack for a moment before it locks. A late swing, twist or button press still slides or turns it; drop again to lock at once.";
pub const SENS_SIDE: &str = "Sideways sensitivity";
pub const SENS_ROT: &str = "Rotation sensitivity";
pub const SENS_DROP: &str = "Drop sensitivity";
pub const CALIBRATE: &str = "Calibrate the remote";
pub const VOLUME: &str = "Effects volume";
pub const MUTE: &str = "Mute";
pub const MORE: &str = "More settings";
pub const GHOST: &str = "Landing preview";
pub const GLOW: &str = "Glow";
pub const REDUCED_MOTION: &str = "Reduced motion";
pub const PATTERNS: &str = "Colour patterns";
pub const DAS: &str = "Button repeat delay";
pub const ARR: &str = "Button repeat speed";
pub const SHORTCUTS: &str = "Colour buttons";
pub const WHEEL_ROTATES: &str = "Wheel rotates";
pub const ZEN_PACE: &str = "Zen pace";
pub const START_LEVEL: &str = "Marathon start level";
pub const DIM_AFTER: &str = "Dim menus after";
pub const AUTO_PAUSE: &str = "Pause when idle after";
pub const HIDE_CURSOR: &str = "Hide pointer while playing";
pub const DEVELOPER_TOGGLE: &str = "Developer options";
pub const RUN_SETUP: &str = "Run first-time setup";
pub const ON: &str = "On";
pub const OFF: &str = "Off";
pub const NEVER: &str = "Never";
pub const UNAVAILABLE_TWIST: &str = "Needs the remote's motion sensor";

// Onboarding
pub const WELCOME: &str = "Welcome to Luma Blocks";
pub const DETECTING: &str = "Checking your remote…";
pub const MOTION_FOUND: &str = "Motion sensing is available on your remote.";
pub const MOTION_POINTER_ONLY: &str = "Only the pointer is available. Circles, flicks and pointing will work; wrist twist needs the motion sensor.";
pub const MOTION_NONE: &str = "Motion sensing is not available. You can play with the buttons.";
pub const USE_MOTION: &str = "Play with motion";
pub const USE_BUTTONS: &str = "Play with buttons";
pub const SKIP: &str = "Skip";
pub const NEXT: &str = "Continue";
pub const CHOOSE_STYLE: &str = "How would you like to play?";
pub const TRY_TITLE: &str = "Give it a try";
pub const TRY_SIDEWAYS: &str = "Move a piece sideways";
pub const TRY_ROTATE: &str = "Rotate it";
pub const TRY_DROP: &str = "Drop it with a quick downward flick";
pub const START_PLAYING: &str = "Start playing";

// Calibration
pub const CAL_TITLE: &str = "Set up motion";
pub const CAL_STEP: &str = "Step";
pub const CAL_HOLD_T: &str = "Hold it like you play";
pub const CAL_HOLD: &str = "Sit comfortably and point the remote at the screen. Keep it still for a moment.";
pub const CAL_HOLD_LIVE: &str = "Keep still";
pub const CAL_TWIST_T: &str = "Twist to the right";
pub const CAL_TWIST: &str =
    "Turn your wrist to the right, like turning a key, then back. This is how you rotate pieces.";
pub const CAL_SWING_T: &str = "Swing to the right";
pub const CAL_SWING: &str =
    "Swing the remote quickly to the right, then back. This is how you move pieces sideways.";
pub const CAL_LEFT_T: &str = "Left edge";
pub const CAL_LEFT: &str = "Move the dot onto the left edge of the board and press OK.";
pub const CAL_RIGHT_T: &str = "Right edge";
pub const CAL_RIGHT: &str = "Now move the dot onto the right edge and press OK.";
pub const CAL_TRY_T: &str = "Try it";
pub const CAL_TRY: &str =
    "Play with the piece. If anything goes the wrong way, flip it with a colour button.";
pub const CAL_GOT_IT: &str = "Got it";
pub const CAL_RETRY: &str = "That was hard to read. Please try once more, a little bigger.";
pub const CAL_TWISTED: &str = "That looked like a twist. Swing sideways instead, like turning your head.";
pub const CAL_NO_SENSOR: &str =
    "The remote's motion sensor is not answering. Move the remote to wake it, or press Back to skip.";
pub const CAL_KEYS_MEASURE: &str = "OK  skip this step      Back  cancel";
pub const CAL_KEYS_EDGE: &str = "OK  set      Back  cancel";
pub const CAL_DONE_BTN: &str = "OK  done";
pub const CAL_FLIP_TWIST: &str = "Red  rotation reversed";
pub const CAL_FLIP_SIDE: &str = "Green  sideways reversed";
pub const CAL_FLIP_DROP: &str = "Yellow  drop reversed";
pub const CAL_REDO: &str = "Back  start over";
pub const CAL_FLIPPED: &str = "Flipped";
pub const TRY_MOVE: &str = "Move sideways";
pub const TRY_ROT: &str = "Rotate";
pub const TRY_DROP_SHORT: &str = "Drop";

// Records
pub const NO_RECORDS: &str = "No records yet.";

// Shortcut hints
pub const HINT_PAUSE: &str = "Back  Pause";
