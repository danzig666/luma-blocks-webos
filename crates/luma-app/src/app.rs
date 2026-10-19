//! Application state: screens, the game session, lifecycle (focus, background, exit),
//! motion policy, idle dimming and persistence timing. Screen-specific input and drawing
//! live in `screens.rs`, `setup.rs` and `diag.rs`.

use crate::audio::{Audio, Sfx};
use crate::gfx::Gfx;
use crate::keys::In;
use crate::motion::{MotionHub, State as MState};
use crate::platform::now_us;
use crate::store::{ControlMode, Store};
use crate::ui::{List, UiIn};
use crate::view::{self, Fx, Look, BOARD_W, BOARD_X};
use luma_core::game::{Event, Status, COLS};
use luma_core::{Action, Game, Mode, Rules};
use luma_input::buttons::{Button, ButtonMapper, Cmd};
use luma_input::calib::PointRange;
use luma_input::config::{HorizontalStyle, RotationStyle};
use luma_input::engine::target_left_column;
use luma_input::math::Vec2;
use luma_input::trace::{Recorder, TraceLine, TRACE_VERSION, UNITS};
use luma_input::{Gesture, GestureEngine};
use std::collections::VecDeque;
use std::path::PathBuf;

/// Fixed simulation step (120 Hz) and bounded catch-up.
pub const STEP_US: u64 = 8_333;
pub const MAX_STEPS: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Menu,
    Resume,
    ConfirmNew(Mode),
    Game,
    Pause,
    MotionLost,
    Over,
    Settings,
    MoreSettings,
    Shortcuts,
    Records,
    Calibrate,
    Onboard,
    Diagnostics,
    Developer,
}

/// Play stops for lost motion only after it has been missing this long...
const MOTION_LOST_PAUSE_US: u64 = 5_000_000;
/// ...and no button was used for this long.
const MOTION_LOST_IDLE_US: u64 = 3_000_000;
/// The "motion paused" note waits this long after the last button press.
const MOTION_NOTE_AFTER_BUTTONS_US: u64 = 2_500_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseReason {
    User,
    Idle,
    Focus,
    Motion,
}

pub struct Session {
    pub game: Game,
    pub fx: Fx,
    pub buttons: ButtonMapper,
    acc_us: u64,
    last_us: u64,
    pub target: Option<i32>,
    piece_key: (u32, u8, i32),
    move_budget_us: i64,
    last_save_us: u64,
    pub ready_until: Option<u64>,
    pub practice_log: VecDeque<(u64, String)>,
    /// Extra time a sliding piece has waited for a swing in progress, µs.
    slide_hold_us: u64,
    pub sensor_seen: bool,
    missing_since: Option<u64>,
    pub buttons_only: bool,
    motion_warned: bool,
}

impl Session {
    pub fn new(game: Game, s: &Store) -> Session {
        Session {
            game,
            fx: Fx::default(),
            buttons: ButtonMapper::new(s.settings.repeat, s.settings.bindings.clone()),
            acc_us: 0,
            last_us: now_us(),
            target: None,
            piece_key: (u32::MAX, 0, 0),
            move_budget_us: 0,
            last_save_us: 0,
            ready_until: None,
            practice_log: VecDeque::new(),
            slide_hold_us: 0,
            sensor_seen: false,
            missing_since: None,
            buttons_only: false,
            motion_warned: false,
        }
    }
}

pub struct OverInfo {
    pub mode: Mode,
    pub finished: bool,
    pub score: u64,
    pub lines: u32,
    pub level: u32,
    pub time_ms: u64,
    pub rank: Option<usize>,
}

#[derive(Default)]
pub struct FrameStats {
    pub times: VecDeque<f32>,
    /// Recent (gesture, recognition ms, recognized -> applied ms).
    pub latency: VecDeque<(String, f32, f32)>,
}

impl FrameStats {
    pub fn push(&mut self, ms: f32) {
        if self.times.len() >= 120 {
            self.times.pop_front();
        }
        self.times.push_back(ms);
    }
    pub fn summary(&self) -> (f32, f32) {
        if self.times.is_empty() {
            return (0.0, 0.0);
        }
        let avg = self.times.iter().sum::<f32>() / self.times.len() as f32;
        let max = self.times.iter().cloned().fold(0.0, f32::max);
        (avg, max)
    }
}

pub struct App {
    pub quit: bool,
    pub sdl: sdl2::Sdl,
    pub store: Store,
    pub audio: Audio,
    pub motion: MotionHub,
    pub engine: GestureEngine,
    pub screen: Screen,
    pub return_to: Screen,
    pub list: List,
    pub menu_focus: usize,
    pub session: Option<Session>,
    pub pause_reason: PauseReason,
    pub focused: bool,
    pub last_activity: u64,
    activity_ptr: (i32, i32),
    pub pointer: Option<(i32, i32)>,
    cursor_hidden: Option<bool>,
    /// Last system pointer event, and the app-drawn cursor fed by the sensor's own cursor
    /// coordinates when the system pointer goes quiet (e.g. a hidden system cursor).
    sdl_ptr_us: u64,
    pub own_cursor: Option<(i32, i32)>,
    own_cursor_us: u64,
    pub recorder: Option<Recorder>,
    pub last_trace: Option<PathBuf>,
    pub dev_msg: String,
    pub over: Option<OverInfo>,
    pub cal: Option<crate::setup::Calib>,
    pub onb: Option<crate::setup::Onboard>,
    pub stats: FrameStats,
    pub log_path: PathBuf,
    pub sdl_desc: String,
    pub records_tab: u8,
    pub settings_dirty: bool,
    /// When the current screen was entered (transitions and input guards).
    pub screen_since: u64,
    pub cursor_test_hidden: bool,
    /// A short message over the game (text, shown since).
    pub notice: Option<(&'static str, u64)>,
    /// While the motion-lost screen is up: since when the stream has been back.
    motion_back_since: Option<u64>,
    pub diag_counts: (u64, u64),
}

impl App {
    pub fn new(sdl: sdl2::Sdl, store: Store, motion: MotionHub, log_path: PathBuf, sdl_desc: String) -> App {
        let audio = Audio::open(&sdl);
        audio.set_volume(store.settings.volume, store.settings.mute);
        let mut engine = GestureEngine::new(
            store.thresholds.clone(),
            store.settings.profile,
            store.calibration.clone(),
        );
        engine.enabled = store.settings.control == ControlMode::Motion;
        let screen = if !store.settings.first_run_done {
            Screen::Onboard
        } else if store.save.is_some() {
            Screen::Resume
        } else {
            Screen::Menu
        };
        let mut app = App {
            quit: false,
            sdl,
            store,
            audio,
            motion,
            engine,
            screen: Screen::Menu,
            return_to: Screen::Menu,
            list: List::new(0),
            menu_focus: 0,
            session: None,
            pause_reason: PauseReason::User,
            focused: true,
            last_activity: now_us(),
            activity_ptr: (0, 0),
            pointer: None,
            cursor_hidden: None,
            sdl_ptr_us: 0,
            own_cursor: None,
            own_cursor_us: 0,
            recorder: None,
            last_trace: None,
            dev_msg: String::new(),
            over: None,
            cal: None,
            onb: None,
            stats: FrameStats::default(),
            log_path,
            sdl_desc,
            records_tab: 0,
            settings_dirty: false,
            screen_since: 0,
            cursor_test_hidden: false,
            notice: None,
            motion_back_since: None,
            diag_counts: (0, 0),
        };
        if screen == Screen::Onboard {
            app.start_onboarding();
        } else {
            app.go(screen);
        }
        app
    }

    pub fn go(&mut self, s: Screen) {
        self.screen_since = now_us();
        self.list = List::new(0);
        if s == Screen::Menu {
            self.list = List::new(self.menu_focus);
        }
        self.screen = s;
    }

    pub fn sfx(&self, s: Sfx) {
        self.audio.play(s);
    }

    pub fn look(&self) -> Look {
        Look {
            glow: self.store.settings.glow,
            ghost: self.store.settings.ghost,
            patterns: self.store.settings.patterns,
            reduced_motion: self.store.settings.reduced_motion,
        }
    }

    /// Apply changed settings to the engine, audio and session.
    pub fn settings_changed(&mut self) {
        self.store.settings.sanitize();
        let s = &self.store.settings;
        self.engine.set_profile(s.profile, self.store.thresholds.clone());
        self.engine.enabled = s.control == ControlMode::Motion;
        self.audio.set_volume(s.volume, s.mute);
        if let Some(sess) = self.session.as_mut() {
            sess.buttons.timing = s.repeat;
            sess.buttons.bindings = s.bindings.clone();
            sess.game.rules.drop_slide_us = s.drop_slide_ms * 1000;
        }
        self.settings_dirty = true;
    }

    pub fn motion_control_active(&self) -> bool {
        self.store.settings.control == ControlMode::Motion
            && !self.session.as_ref().is_some_and(|s| s.buttons_only)
    }

    // ---------------------------------------------------------------- game lifecycle

    pub fn new_game(&mut self, mode: Mode) {
        let s = &self.store.settings;
        let rules = Rules {
            start_level: s.marathon_start_level,
            zen_level: s.zen_level,
            drop_slide_us: s.drop_slide_ms * 1000,
            ..Rules::default()
        };
        let seed = if mode == Mode::Practice {
            0x5EED_1234 // repeatable sample placements
        } else {
            now_us() ^ 0x9E37_79B9_7F4A_7C15 ^ self.store.records.seq.wrapping_mul(7919)
        };
        let game = Game::new(mode, rules, seed);
        if mode != Mode::Practice {
            self.store.records.games_played += 1;
            self.store.save_records();
        }
        self.session = Some(Session::new(game, &self.store));
        if mode != Mode::Practice {
            self.save_now();
        }
        self.engine.reset();
        self.go(Screen::Game);
        self.begin_ready();
    }

    /// Start a mode, asking first if it would replace a saved game.
    pub fn request_mode(&mut self, mode: Mode) {
        if mode != Mode::Practice && self.store.save.is_some() {
            self.go(Screen::ConfirmNew(mode));
        } else {
            self.new_game(mode);
        }
    }

    /// Restore the saved game, paused, with input state cleared.
    pub fn continue_saved(&mut self) {
        if let Some(sg) = self.store.save.clone() {
            let mut g = sg.game;
            g.clear_transient();
            g.rules.drop_slide_us = self.store.settings.drop_slide_ms * 1000;
            self.session = Some(Session::new(g, &self.store));
            self.engine.reset();
            self.pause_reason = PauseReason::User;
            self.go(Screen::Pause);
        }
    }

    pub fn save_now(&mut self) {
        if let Some(s) = self.session.as_mut() {
            if s.game.mode == Mode::Practice {
                return;
            }
            s.last_save_us = now_us();
            let g = s.game.clone();
            self.store.save_game(&g);
        }
    }

    pub fn pause(&mut self, reason: PauseReason) {
        if self.screen != Screen::Game {
            return;
        }
        if let Some(s) = self.session.as_mut() {
            for c in s.buttons.release_all() {
                if c == Cmd::SoftDrop(false) {
                    s.game.apply(Action::SoftDropStop);
                }
            }
            s.game.soft_drop = false;
            s.ready_until = None;
        }
        self.engine.reset();
        self.save_now();
        self.pause_reason = reason;
        self.go(if reason == PauseReason::Motion {
            Screen::MotionLost
        } else {
            Screen::Pause
        });
    }

    fn begin_ready(&mut self) {
        if let Some(s) = self.session.as_mut() {
            s.ready_until = Some(now_us() + 450_000);
            s.last_us = now_us();
            s.acc_us = 0;
            s.target = None;
            s.missing_since = None;
        }
        self.engine.reset();
    }

    /// Explicit, safe resume: wait for button release and a fresh neutral motion state.
    pub fn resume(&mut self) {
        if self.session.is_none() {
            return self.go(Screen::Menu);
        }
        self.go(Screen::Game);
        self.begin_ready();
    }

    pub fn quit_to_menu(&mut self) {
        self.save_now();
        self.session = None;
        self.menu_focus = 0;
        self.go(Screen::Menu);
    }

    pub fn recenter(&mut self) {
        self.motion.send(crate::motion::Cmd::Recenter);
        if self.engine.recenter() {
            self.store.calibration.neutral = self.engine.calib.neutral;
            self.store.save_calibration();
        }
    }

    fn finish_game(&mut self) {
        let Some(s) = self.session.as_ref() else { return };
        let g = &s.game;
        let finished = g.status == Status::Finished;
        let rank = match (g.mode, finished) {
            (Mode::Marathon, _) => self.store.records.add_marathon(g),
            (Mode::Sprint, true) => self.store.records.add_sprint(g),
            _ => None,
        };
        self.over = Some(OverInfo {
            mode: g.mode,
            finished,
            score: g.score,
            lines: g.lines,
            level: g.level,
            time_ms: g.elapsed_us / 1000,
            rank,
        });
        self.store.save_records();
        self.store.clear_game();
        self.sfx(if finished { Sfx::Special } else { Sfx::GameOver });
        self.go(Screen::Over);
    }

    // ---------------------------------------------------------------- input

    pub fn on_focus(&mut self, gained: bool) {
        if gained == self.focused {
            return;
        }
        self.focused = gained;
        log::info!("focus {}", if gained { "gained" } else { "lost" });
        if !gained {
            if self.screen == Screen::Game {
                self.pause(PauseReason::Focus);
            } else {
                self.save_now();
            }
            self.engine.reset();
            self.motion.stop();
            if self.settings_dirty {
                self.store.save_settings();
                self.settings_dirty = false;
            }
        } else {
            self.last_activity = now_us();
        }
    }

    pub fn dim_level(&self, now: u64) -> f32 {
        let active_play = self.screen == Screen::Game;
        if active_play {
            return 0.0;
        }
        let idle_s = now.saturating_sub(self.last_activity) as f32 / 1e6;
        let d = self.store.settings.idle_dim_s as f32;
        if idle_s > d * 2.0 {
            1.0
        } else if idle_s > d {
            ((idle_s - d) / 1.5).min(1.0) * 0.7
        } else {
            0.0
        }
    }

    pub fn wants_low_power(&self, now: u64) -> bool {
        !self.focused || self.dim_level(now) >= 1.0
    }

    pub fn handle(&mut self, inputs: Vec<In>) {
        let now = now_us();
        for inp in inputs {
            match &inp {
                In::Quit => {
                    self.exit();
                    continue;
                }
                In::Focus(f) => {
                    self.on_focus(*f);
                    continue;
                }
                _ => {}
            }
            // Intentional activity (sensor samples never count).
            let activity = match &inp {
                In::Button { down: true, .. } | In::Click { down: true, .. } | In::Wheel { .. } => true,
                In::Key { down: true, .. } => true,
                In::PointerMove { x, y } => {
                    (x - self.activity_ptr.0).abs() + (y - self.activity_ptr.1).abs() > 40
                }
                _ => false,
            };
            if activity {
                let was_dim = self.dim_level(now) > 0.05;
                self.last_activity = now;
                if let In::PointerMove { x, y } = &inp {
                    self.activity_ptr = (*x, *y);
                }
                // The first input that wakes a dimmed screen only restores it.
                if was_dim && !matches!(inp, In::PointerMove { .. }) {
                    continue;
                }
            }
            if let In::Button { b, down: true, .. } = &inp {
                if !self.store.settings.seen_buttons.contains(b) {
                    self.store.settings.seen_buttons.push(*b);
                    self.settings_dirty = true;
                }
            }
            if let Some(r) = self.recorder.as_mut() {
                match &inp {
                    In::Button {
                        b,
                        down,
                        repeat: false,
                    } => r.push(TraceLine::Button {
                        t: now,
                        b: *b,
                        down: *down,
                    }),
                    In::PointerMove { x, y } => r.push(TraceLine::Pointer {
                        t: now,
                        x: *x as f32 / 1080.0,
                        y: *y as f32 / 1080.0,
                    }),
                    _ => {}
                }
            }
            match &inp {
                In::PointerMove { x, y } => {
                    self.pointer = Some((*x, *y));
                    self.sdl_ptr_us = now;
                    self.engine
                        .pointer(now, Vec2::new(*x as f32 / 1080.0, *y as f32 / 1080.0));
                }
                In::Button {
                    down: true,
                    repeat: false,
                    ..
                }
                | In::Click { down: true, .. } => self.engine.button(now),
                _ => {}
            }
            self.sim_keys(&inp);
            match self.screen {
                Screen::Game => self.game_input(&inp, now),
                Screen::Calibrate => self.calib_input(&inp),
                Screen::Onboard => self.onboard_input(&inp),
                Screen::Diagnostics => self.diag_input(&inp),
                _ => {
                    if let Some(u) = to_ui(&inp) {
                        self.screen_input(u);
                    }
                }
            }
        }
    }

    /// Desktop simulator keys (motion stand-ins); inert on the TV.
    fn sim_keys(&mut self, inp: &In) {
        use crate::motion::Script;
        use sdl2::keyboard::Keycode as K;
        let Some(sim) = self.motion.sim() else { return };
        match inp {
            In::Key {
                key,
                down,
                repeat: false,
            } => {
                let script = match *key {
                    K::Num1 => Some(Script::TwistCw),
                    K::Num2 => Some(Script::TwistCcw),
                    K::Num3 => Some(Script::CircleCw),
                    K::Num4 => Some(Script::CircleCcw),
                    K::Num5 => Some(Script::FlickLeft),
                    K::Num6 => Some(Script::FlickRight),
                    K::Num7 => Some(Script::Drop),
                    K::Num8 => Some(Script::SwingLeft),
                    K::Num9 => Some(Script::SwingRight),
                    _ => None,
                };
                if let (Some(s), true) = (script, *down) {
                    sim.play(s);
                }
                match *key {
                    K::Q => {
                        if *down {
                            sim.twist_key(-1.0)
                        } else {
                            sim.twist_release()
                        }
                    }
                    K::E => {
                        if *down {
                            sim.twist_key(1.0)
                        } else {
                            sim.twist_release()
                        }
                    }
                    _ => {}
                }
            }
            In::Click {
                left: false, down, ..
            } => sim.aiming = *down,
            In::PointerMove { x, y } => {
                if let Some((px, py)) = self.pointer {
                    sim.mouse_delta((x - px) as f32, (y - py) as f32);
                }
            }
            _ => {}
        }
    }

    fn game_input(&mut self, inp: &In, now: u64) {
        let Some(s) = self.session.as_mut() else { return };
        if s.ready_until.is_some() {
            // Ignore gameplay input until resume completes; Back still pauses.
            if let In::Button {
                b: Button::Back,
                down: true,
                ..
            } = inp
            {
                self.pause(PauseReason::User);
            }
            return;
        }
        let mut cmds = Vec::new();
        match inp {
            In::Button { b, down, repeat } => {
                if *down && matches!(b, Button::Left | Button::Right) {
                    self.engine.suspend_pointing();
                    s.target = None;
                }
                cmds = s.buttons.event(now, *b, *down, *repeat);
            }
            In::Click {
                left: true,
                down: true,
                ..
            } => cmds = s.buttons.click(now),
            In::Wheel { dy } if s.buttons.bindings.wheel_rotates && *dy != 0 => {
                cmds.push(if *dy > 0 { Cmd::RotateCw } else { Cmd::RotateCcw });
            }
            _ => {}
        }
        for c in cmds {
            self.apply_cmd(c);
        }
    }

    pub fn apply_cmd(&mut self, c: Cmd) {
        let Some(s) = self.session.as_mut() else { return };
        let g = &mut s.game;
        match c {
            Cmd::Move(d) => {
                g.apply(if d < 0 {
                    Action::MoveLeft
                } else {
                    Action::MoveRight
                });
            }
            Cmd::SoftDrop(on) => {
                g.apply(if on {
                    Action::SoftDropStart
                } else {
                    Action::SoftDropStop
                });
            }
            Cmd::RotateCw => {
                g.apply(Action::RotateCw);
            }
            Cmd::RotateCcw => {
                g.apply(Action::RotateCcw);
            }
            Cmd::HardDrop => {
                g.apply(Action::HardDrop);
            }
            Cmd::Hold => {
                g.apply(Action::Hold);
            }
            Cmd::Pause => self.pause(PauseReason::User),
            Cmd::Recenter => self.recenter(),
            Cmd::Calibrate => {
                self.pause(PauseReason::User);
                self.start_calibration(Screen::Pause);
            }
        }
    }

    pub fn exit(&mut self) {
        if self.screen == Screen::Game || self.screen == Screen::Pause {
            self.save_now();
        }
        self.quit = true;
    }

    // ---------------------------------------------------------------- per-frame update

    fn motion_wanted(&self) -> bool {
        if !self.focused {
            return false;
        }
        match self.screen {
            Screen::Calibrate
            | Screen::Onboard
            | Screen::Diagnostics
            | Screen::Developer
            | Screen::MotionLost => true,
            Screen::Game => self.motion_control_active(),
            _ => false,
        }
    }

    pub fn update(&mut self) {
        let now = now_us();
        if self.motion_wanted() {
            self.motion.start();
        } else {
            self.motion.stop();
        }
        let (samples, restarted) = self.motion.drain(now);
        if restarted {
            // Remote wake / reconnection: never act on stale state.
            self.engine.reset();
        }
        for smp in &samples {
            self.engine.motion(smp);
            if let Some(r) = self.recorder.as_mut() {
                r.push(TraceLine::motion(smp));
            }
        }
        self.engine.tick(now);
        let commits = self.engine.take();
        if let Some(r) = self.recorder.as_mut() {
            for c in &commits {
                r.push(TraceLine::Recognized {
                    t: c.t_us,
                    g: c.gesture,
                });
            }
        }
        match self.screen {
            Screen::Game => self.update_game(now, commits),
            Screen::Calibrate => self.update_calibration(now, &commits),
            Screen::MotionLost => {
                // Motion is back: carry on by itself (through Ready), so the message is not
                // swapped for a menu the moment the stream returns.
                if self.motion.is_streaming(now) {
                    let t0 = *self.motion_back_since.get_or_insert(now);
                    if now - t0 > 600_000 {
                        self.motion_back_since = None;
                        self.notice = Some((crate::strings::MOTION_BACK, now));
                        self.resume();
                    }
                } else {
                    self.motion_back_since = None;
                }
            }
            Screen::Diagnostics | Screen::Developer => self.update_diag(&commits),
            _ => {}
        }
        self.update_own_cursor(now);
        // Cursor: hidden during play only when the sensor (not the pointer) drives motion,
        // or in button mode; the diagnostics toggle tests the effect on sensor delivery.
        let streaming_sensor = self.motion.is_real() && self.motion.is_streaming(now);
        let hide = (self.screen == Screen::Game
            && self.store.settings.hide_cursor
            && (!self.motion_control_active() || streaming_sensor || !self.motion.is_real()))
            || (self.screen == Screen::Diagnostics && self.cursor_test_hidden);
        if self.cursor_hidden != Some(hide) {
            self.sdl.mouse().show_cursor(!hide);
            self.cursor_hidden = Some(hide);
        }
        if self.settings_dirty && self.screen != Screen::Game {
            self.store.save_settings();
            self.settings_dirty = false;
        }
    }

    pub fn default_range(&self) -> PointRange {
        match self.engine.active_source() {
            Some(luma_input::calib::AimSource::Pointer) => {
                let l = BOARD_X as f32 / 1080.0;
                let r = (BOARD_X + BOARD_W) as f32 / 1080.0;
                PointRange {
                    left: l,
                    center: (l + r) / 2.0,
                    right: r,
                }
            }
            _ => PointRange {
                left: -0.32,
                center: 0.0,
                right: 0.32,
            },
        }
    }

    fn update_game(&mut self, now: u64, commits: Vec<luma_input::Commit>) {
        let last_activity = self.last_activity;
        let motion_on = self.motion_control_active();
        let swing_in_progress = motion_on && self.engine.swing.is_stroke();
        let streaming = self.motion.is_streaming(now);
        let needs_sensor = self.store.settings.profile.rotation == RotationStyle::Twist;
        let unavailable = self.motion.state().is_unavailable();
        let range = self.default_range();
        let pointing = motion_on && self.store.settings.profile.horizontal == HorizontalStyle::Pointing;
        let u = if pointing {
            self.engine.aim_column(COLS as f32, range)
        } else {
            None
        };
        let armed = self.engine.is_armed();
        let auto_pause = self.store.settings.auto_pause_s;
        let hyst = self.store.thresholds.point_hysteresis_cols;
        let look = self.look();
        let Some(s) = self.session.as_mut() else { return };

        // Motion availability.
        if motion_on && streaming {
            s.sensor_seen = true;
            s.missing_since = None;
        }
        if motion_on && needs_sensor && s.ready_until.is_none() {
            if s.sensor_seen && !streaming {
                // Short gaps (the stream reconnecting) only show a note; play stops only when
                // motion stays away and the buttons are not being used either.
                let since = *s.missing_since.get_or_insert(now);
                let using_buttons = now.saturating_sub(last_activity) < MOTION_LOST_IDLE_US;
                if now - since > MOTION_LOST_PAUSE_US && !using_buttons {
                    self.pause(PauseReason::Motion);
                    return;
                }
            } else if !s.sensor_seen && unavailable && !s.motion_warned {
                s.motion_warned = true;
                self.pause(PauseReason::Motion);
                return;
            }
        }

        // Resume guard: wait for release and a neutral, still remote.
        if let Some(until) = s.ready_until {
            let neutral_ok = !motion_on || !streaming || armed;
            if (now >= until && !s.buttons.any_held() && neutral_ok) || now > until + 2_500_000 {
                s.ready_until = None;
                s.last_us = now;
                s.acc_us = 0;
            } else {
                s.last_us = now;
                return;
            }
        }

        // Idle auto-pause (intentional input only).
        if auto_pause > 0
            && now.saturating_sub(self.last_activity) > auto_pause as u64 * 1_000_000
            && commits.is_empty()
        {
            self.pause(PauseReason::Idle);
            return;
        }
        if !commits.is_empty() {
            self.last_activity = now;
        }

        // Buttons auto-repeat.
        let cmds = s.buttons.tick(now);
        // Gestures.
        let mut gesture_actions = Vec::new();
        if motion_on {
            for c in &commits {
                let a = match c.gesture {
                    Gesture::RotateCw => Action::RotateCw,
                    Gesture::RotateCcw => Action::RotateCcw,
                    Gesture::MoveLeft => Action::MoveLeft,
                    Gesture::MoveRight => Action::MoveRight,
                    Gesture::HardDrop => Action::HardDrop,
                };
                gesture_actions.push((a, *c));
            }
        }
        for c in cmds {
            self.apply_cmd(c);
        }
        let Some(s) = self.session.as_mut() else { return };
        for (a, c) in gesture_actions {
            s.game.apply(a);
            let applied_ms = now.saturating_sub(c.t_us) as f32 / 1000.0;
            if self.stats.latency.len() >= 10 {
                self.stats.latency.pop_front();
            }
            self.stats
                .latency
                .push_back((c.gesture.label().to_owned(), c.recognition_ms(), applied_ms));
            if s.game.mode == Mode::Practice {
                if s.practice_log.len() >= 8 {
                    s.practice_log.pop_front();
                }
                s.practice_log.push_back((
                    now,
                    format!("{}  ·  {:.0} ms", c.gesture.label(), c.recognition_ms()),
                ));
            }
        }

        // Pointing: move toward the target column through legal moves only.
        let p = s.game.active;
        let key = (s.game.pieces_placed, p.rot, view::piece_width(&p));
        if key != s.piece_key || s.game.hold_used && s.target.is_none() {
            s.piece_key = key;
            s.target = None;
        }
        if let (Some(u), true) = (u, s.game.is_playing()) {
            let w = view::piece_width(&p);
            let t = target_left_column(u, w, COLS, s.target, hyst);
            s.target = Some(t);
            let dt = now.saturating_sub(s.last_us) as i64;
            s.move_budget_us = (s.move_budget_us + dt).min(40_000);
            let per_move = 12_000;
            while s.move_budget_us >= per_move {
                let lo = s.game.active.column_span().0;
                if lo == t {
                    break;
                }
                let ok = s.game.apply(if lo < t {
                    Action::MoveRight
                } else {
                    Action::MoveLeft
                });
                s.move_budget_us -= per_move;
                if !ok {
                    break;
                }
                // A piece following the player's aim is intentional activity.
                self.last_activity = now;
            }
        } else {
            s.move_budget_us = 0;
        }

        // A sliding piece waits (a little) for a swing that is under way: the swing's move
        // comes when it ends, which is often just after the slide time.
        let frame_us = now.saturating_sub(s.last_us);
        if !s.game.is_sliding() {
            s.slide_hold_us = 0;
        } else if swing_in_progress && s.slide_hold_us < crate::store::DROP_SLIDE_SWING_HOLD_US {
            s.game.hold_slide(frame_us);
            s.slide_hold_us += frame_us;
        }

        // Fixed-step simulation with bounded catch-up.
        s.acc_us += frame_us;
        s.last_us = now;
        let mut n = 0;
        while s.acc_us >= STEP_US && n < MAX_STEPS {
            s.game.tick(STEP_US);
            s.acc_us -= STEP_US;
            n += 1;
        }
        if n == MAX_STEPS {
            s.acc_us = 0;
        }

        // Events: sound, effects, saves.
        let events = s.game.drain_events();
        let mut locked = false;
        let mut sounds = Vec::new();
        for e in &events {
            s.fx.on_event(e, now, &look);
            match e {
                Event::Moved => sounds.push(Sfx::Move),
                Event::Rotated { .. } => sounds.push(Sfx::Rotate),
                Event::Blocked => sounds.push(Sfx::Invalid),
                Event::HardDrop { .. } => sounds.push(Sfx::HardDrop),
                Event::Locked { .. } => {
                    locked = true;
                    sounds.push(Sfx::Lock)
                }
                Event::Cleared { score, .. } => {
                    let special = score.lines >= 4 || score.spin != luma_core::scoring::Spin::None;
                    sounds.push(if special {
                        Sfx::Special
                    } else {
                        Sfx::Clear(score.lines as u8)
                    });
                }
                Event::Held => sounds.push(Sfx::Hold),
                Event::LevelUp(_) => sounds.push(Sfx::LevelUp),
                _ => {}
            }
        }
        // Blocked moves from pointing glide are not errors worth a sound.
        if pointing && sounds.contains(&Sfx::Invalid) && !sounds.contains(&Sfx::Rotate) {
            sounds.retain(|x| *x != Sfx::Invalid);
        }
        sounds.dedup();
        let ended = !s.game.is_playing();
        let save_due = locked && now.saturating_sub(s.last_save_us) > 1_500_000;
        for snd in sounds {
            self.audio.play(snd);
        }
        if ended {
            self.finish_game();
        } else if save_due {
            self.save_now();
        }
    }

    // ---------------------------------------------------------------- traces

    pub fn toggle_recording(&mut self) {
        if let Some(r) = self.recorder.take() {
            let path = self.store.save_trace(r.to_jsonl());
            self.dev_msg = format!("Saved {} lines to {}", r.len(), path.display());
            self.last_trace = Some(path);
        } else {
            let mut r = Recorder::new(60_000);
            let st = self.motion.status();
            r.header = Some(TraceLine::Header {
                version: TRACE_VERSION,
                origin: self
                    .motion
                    .origin
                    .unwrap_or(luma_input::sample::Origin::Simulated),
                units: UNITS.into(),
                calibration: self.engine.calib.clone(),
                profile: self.engine.profile,
                thresholds: self.engine.cfg.clone(),
                note: format!(
                    "app {} {}; backend state {}; registration {:?}; interval {:?}",
                    env!("CARGO_PKG_VERSION"),
                    crate::platform::app_id(),
                    st.state.label(),
                    st.registration,
                    st.interval
                ),
            });
            self.recorder = Some(r);
            self.dev_msg = "Recording…".into();
        }
    }

    /// How long motion has been missing during play.
    pub fn motion_gap_us(&self, now: u64) -> Option<u64> {
        self.session
            .as_ref()
            .and_then(|s| s.missing_since)
            .map(|t| now.saturating_sub(t))
    }

    /// Whether to show the "motion paused" note: motion is missing and the player is not
    /// busy with the buttons (a button press itself pauses the remote's motion reports).
    pub fn motion_note_due(&self, now: u64) -> bool {
        self.motion_gap_us(now).is_some_and(|g| g > 400_000)
            && now.saturating_sub(self.last_activity) > MOTION_NOTE_AFTER_BUTTONS_US
    }

    /// One line on the motion connection for ordinary screens, so a photo of the screen says
    /// what failed (the full log is in Input diagnostics).
    pub fn motion_detail(&self) -> Option<String> {
        if !self.motion.is_real() {
            return None;
        }
        let st = self.motion.status();
        let mut d = format!("Motion: {}", st.state.label());
        if st.stalls > 0 {
            d += &format!(" · {} reconnects", st.stalls);
        }
        if let Some(r) = &st.registration {
            d += &format!(" · via {r}");
        }
        if let Some(e) = &st.last_error {
            d += &format!(" · {e:?}");
        } else if let Some(l) = st.log.back() {
            d += &format!(" · {}", l.trim());
        }
        Some(d.chars().take(260).collect())
    }

    /// When the system pointer is silent but the sensor reports its cursor position, draw
    /// our own cursor there and let it hover menus.
    fn update_own_cursor(&mut self, now: u64) {
        if now.saturating_sub(self.sdl_ptr_us) < 1_500_000 || self.screen == Screen::Game {
            return;
        }
        let Some((cx, cy)) = self.motion.last.and_then(|s| s.cursor) else {
            return;
        };
        let p = (cx.round() as i32, cy.round() as i32);
        if self
            .own_cursor
            .is_some_and(|o| (o.0 - p.0).abs() + (o.1 - p.1).abs() <= 2)
        {
            return;
        }
        self.own_cursor = Some(p);
        self.own_cursor_us = now;
        match self.screen {
            Screen::Onboard => self.onboard_input(&In::PointerMove { x: p.0, y: p.1 }),
            Screen::Calibrate | Screen::Diagnostics => {}
            _ => self.screen_input(UiIn::Pointer(p.0, p.1)),
        }
    }

    // ---------------------------------------------------------------- drawing

    pub fn draw(&mut self, g: &mut Gfx) {
        let now = now_us();
        g.clear();
        match self.screen {
            Screen::Game | Screen::Pause | Screen::MotionLost | Screen::Over => self.draw_play(g, now),
            Screen::Calibrate => self.draw_calibration(g, now),
            Screen::Onboard => self.draw_onboarding(g, now),
            Screen::Diagnostics => self.draw_diag(g, now),
            Screen::Developer => self.draw_developer(g),
            _ => self.draw_screen(g, now),
        }
        // Every screen fades in from black (except returning to play, which has "Ready").
        let since = now.saturating_sub(self.screen_since) as f32 / 1000.0;
        if since < 240.0 && self.screen != Screen::Game && !self.store.settings.reduced_motion {
            let a = 1.0 - since / 240.0;
            g.fill(
                sdl2::rect::Rect::new(0, 0, crate::gfx::W as u32, crate::gfx::H as u32),
                sdl2::pixels::Color::RGBA(0, 0, 0, (a * a * 255.0) as u8),
            );
        }
        if self.screen != Screen::Game && now.saturating_sub(self.own_cursor_us) < 4_000_000 {
            if let Some((x, y)) = self.own_cursor {
                crate::setup::draw_cursor(g, x, y, 0.8, now);
            }
        }
        let d = self.dim_level(now);
        if d > 0.0 {
            g.fill(
                sdl2::rect::Rect::new(0, 0, crate::gfx::W as u32, crate::gfx::H as u32),
                sdl2::pixels::Color::RGBA(0, 0, 0, (d * 255.0) as u8),
            );
        }
    }

    fn draw_play(&mut self, g: &mut Gfx, now: u64) {
        let look = self.look();
        let best = self.session.as_ref().and_then(|s| match s.game.mode {
            Mode::Marathon => self.store.records.best_marathon().map(view::fmt_score),
            Mode::Sprint => self.store.records.best_sprint().map(view::fmt_time),
            _ => None,
        });
        let pointing = self.motion_control_active()
            && self.store.settings.profile.horizontal == HorizontalStyle::Pointing
            && self.screen == Screen::Game;
        if let Some(s) = self.session.as_mut() {
            let target = if pointing {
                s.target.map(|t| {
                    let w = view::piece_width(&s.game.active);
                    let reach = s.game.ghost().column_span().0 == t;
                    (t, w, reach)
                })
            } else {
                None
            };
            let input = view::BoardInput {
                game: &s.game,
                look,
                now,
                target,
                best,
                hide_hud: false,
            };
            view::draw_game(g, &mut s.fx, &input);
        }
        self.draw_play_overlay(g, now);
    }
}

pub fn to_ui(inp: &In) -> Option<UiIn> {
    match inp {
        In::Button { b, down: true, .. } => Some(UiIn::Press(*b)),
        In::PointerMove { x, y } => Some(UiIn::Pointer(*x, *y)),
        In::Click {
            left: true,
            down: true,
            x,
            y,
        } => Some(UiIn::Click(*x, *y)),
        In::Wheel { dy } => Some(UiIn::Wheel(*dy)),
        _ => None,
    }
}

/// Human label for the motion state, without raw platform text.
pub fn motion_summary(st: &MState) -> &'static str {
    match st {
        MState::Streaming | MState::Simulated | MState::Trace => "Motion active",
        MState::Sleeping => "Remote asleep",
        MState::Unsupported(_) | MState::NoBus(_) => "Motion not available",
        MState::Denied(_) => "Motion not permitted",
        MState::Stopped | MState::NotStarted => "Motion idle",
        MState::Failed(_) => "Motion interrupted",
        _ => "Connecting to the remote",
    }
}
