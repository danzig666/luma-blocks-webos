//! First-run onboarding and the guided motion setup.
//!
//! Setup steps (sensor available):
//! 1. Hold: the comfortable neutral pose (auto, once the remote is still).
//! 2. Twist right and back: measures the remote's long axis and which way is clockwise.
//! 3. Swing right and back: measures the sideways axis and its direction.
//! 4. (Pointing style only) left and right board edges, with a visible aim dot.
//! 5. Try it: a live board where every recognized gesture acts on a piece; colour buttons
//!    flip rotation, sideways or drop direction if one feels reversed.
//!
//! Without a sensor the measuring steps are skipped. Every step shows an animated
//! illustration and a live gauge, so it is always clear what to do and what is being seen.

use crate::app::{App, Screen, Session};
use crate::audio::Sfx;
use crate::gfx::{alpha, theme, Face, Gfx, H, W};
use crate::illus::{self, Illus};
use crate::keys::In;
use crate::platform::now_us;
use crate::store::ControlMode;
use crate::strings as S;
use crate::ui::{Ev, Item, UiIn};
use crate::view::{self, BOARD_H, BOARD_W, BOARD_X, BOARD_Y};
use luma_core::game::COLS;
use luma_core::{Action, Game, Mode, Rules};
use luma_input::buttons::Button;
use luma_input::calib::{apply_swing_probe, apply_twist_probe, AimSource, Calibration, PointRange};
use luma_input::config::{HorizontalStyle, RotationStyle};
use luma_input::engine::target_left_column;
use luma_input::Gesture;
use sdl2::rect::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalStep {
    Hold,
    Twist,
    Swing,
    Left,
    Right,
    Try,
}

pub struct Calib {
    pub step: CalStep,
    pub return_to: Screen,
    since: u64,
    still_since: Option<u64>,
    crossed_at: Option<u64>,
    pub msg: Option<&'static str>,
    left: Option<f32>,
    working: Calibration,
    /// Last "got it" moment, for the success flourish.
    ok_at: Option<u64>,
    try_session: Option<Session>,
    try_target: Option<i32>,
    tried: [bool; 3],
    flash: Option<(u64, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnbStep {
    Welcome,
    Style,
}

pub struct Onboard {
    pub step: OnbStep,
    since: u64,
    row: usize,
}

const TWIST_TARGET_DEG: f32 = 35.0;
const SWING_TARGET_DEG: f32 = 8.0;
const HOLD_MS: u64 = 1_000_000;

impl App {
    pub fn start_calibration(&mut self, return_to: Screen) {
        self.cal = Some(Calib {
            step: CalStep::Hold,
            return_to,
            since: now_us(),
            still_since: None,
            crossed_at: None,
            msg: None,
            left: None,
            working: self.store.calibration.clone(),
            ok_at: None,
            try_session: None,
            try_target: None,
            tried: [false; 3],
            flash: None,
        });
        self.engine.stop_probe();
        self.engine.reset();
        self.go(Screen::Calibrate);
    }

    fn pointing_style(&self) -> bool {
        self.store.settings.profile.horizontal == HorizontalStyle::Pointing
    }

    fn cal_advance(&mut self, next: CalStep) {
        let now = now_us();
        if let Some(c) = self.cal.as_mut() {
            c.step = next;
            c.since = now;
            c.still_since = None;
            c.crossed_at = None;
            c.msg = None;
        }
        // The next measurement starts once the remote has settled (see update_calibration),
        // so the end of the previous motion is never measured as the new one.
        self.engine.stop_probe();
        if next == CalStep::Try {
            self.cal_save();
            let game = Game::new(Mode::Practice, Rules::default(), 0x7E57);
            let sess = Session::new(game, &self.store);
            if let Some(c) = self.cal.as_mut() {
                c.try_session = Some(sess);
                c.try_target = None;
                c.tried = [false; 3];
            }
            self.engine.reset();
        }
    }

    /// Mark the current step as recognized: a short flourish, then the next step.
    fn cal_success(&mut self, next: CalStep) {
        if let Some(c) = self.cal.as_mut() {
            c.ok_at = Some(now_us());
        }
        self.sfx(Sfx::MenuSelect);
        self.cal_advance(next);
    }

    fn cal_save(&mut self) {
        if let Some(c) = self.cal.as_ref() {
            self.store.calibration = c.working.clone();
            self.store.save_calibration();
            self.engine.set_calibration(c.working.clone());
        }
    }

    fn after_swing(&self) -> CalStep {
        if self.pointing_style() {
            CalStep::Left
        } else {
            CalStep::Try
        }
    }

    fn cal_finish(&mut self) {
        self.cal_save();
        self.engine.stop_probe();
        let back = self.cal.as_ref().map(|c| c.return_to).unwrap_or(Screen::Menu);
        self.cal = None;
        if back == Screen::Onboard {
            self.onboard_finish(true);
        } else {
            self.go(back);
        }
    }

    pub fn calib_input(&mut self, inp: &In) {
        let Some(step) = self.cal.as_ref().map(|c| c.step) else {
            return;
        };
        let press = match inp {
            In::Button { b, down: true, .. } => Some(*b),
            In::Click {
                left: true,
                down: true,
                ..
            } => Some(Button::Ok),
            _ => None,
        };
        let Some(b) = press else { return };
        match (step, b) {
            (CalStep::Try, Button::Back) => {
                self.start_calibration(self.cal.as_ref().map(|c| c.return_to).unwrap_or(Screen::Menu))
            }
            (_, Button::Back) => {
                // Cancel: keep the previous calibration.
                let back = self.cal.as_ref().map(|c| c.return_to).unwrap_or(Screen::Menu);
                self.engine.stop_probe();
                self.engine.set_calibration(self.store.calibration.clone());
                self.cal = None;
                if back == Screen::Onboard {
                    self.onboard_finish(false);
                } else {
                    self.go(back);
                }
            }
            (CalStep::Hold, Button::Ok) => self.cal_advance(CalStep::Twist),
            (CalStep::Twist, Button::Ok) => self.cal_advance(CalStep::Swing),
            (CalStep::Swing, Button::Ok) => {
                let n = self.after_swing();
                self.cal_advance(n)
            }
            (CalStep::Left | CalStep::Right, Button::Ok) => self.capture_edge(step),
            (CalStep::Try, Button::Ok) => self.cal_finish(),
            (CalStep::Try, Button::Red | Button::Green | Button::Yellow) => {
                if let Some(c) = self.cal.as_mut() {
                    let (sign, what) = match b {
                        Button::Red => (&mut c.working.twist_sign, S::TRY_ROT),
                        Button::Green => (&mut c.working.yaw_sign, S::TRY_MOVE),
                        _ => (&mut c.working.pitch_sign, S::TRY_DROP_SHORT),
                    };
                    *sign = -*sign;
                    c.flash = Some((now_us(), format!("{}: {}", what, S::CAL_FLIPPED)));
                }
                self.cal_save();
                self.engine.reset();
                self.sfx(Sfx::MenuSelect);
            }
            _ => {}
        }
    }

    fn capture_edge(&mut self, step: CalStep) {
        let Some(x) = self.engine.aim_x() else {
            if let Some(c) = self.cal.as_mut() {
                c.msg = Some(S::CAL_RETRY);
            }
            return;
        };
        if step == CalStep::Left {
            if let Some(c) = self.cal.as_mut() {
                c.left = Some(x);
            }
            return self.cal_success(CalStep::Right);
        }
        let src = self.engine.active_source();
        let ok = match self.cal.as_mut() {
            Some(c) => {
                let l = c.left.unwrap_or(x - 0.5);
                let r = PointRange {
                    left: l,
                    center: (l + x) / 2.0,
                    right: x,
                };
                if r.is_valid() {
                    match src {
                        Some(AimSource::Pointer) => c.working.pointer_range = Some(r),
                        _ => c.working.sensor_range = Some(r),
                    }
                    true
                } else {
                    c.msg = Some(S::CAL_RETRY);
                    c.step = CalStep::Left;
                    false
                }
            }
            None => false,
        };
        if ok {
            self.cal_success(CalStep::Try);
        }
    }

    pub fn update_calibration(&mut self, now: u64, commits: &[luma_input::Commit]) {
        let streaming = self.motion.is_streaming(now);
        let f = self.engine.debug.last_frame;
        let quat = self.engine.last_quat();
        let min_deg = if self.cal.as_ref().is_some_and(|c| c.step == CalStep::Swing) {
            6.0
        } else {
            25.0
        };
        let probe = self
            .engine
            .probe()
            .map(|p| (p.angle_deg(), p.current_deg(), p.axis(min_deg)));
        let pointing = self.pointing_style();
        let Some(step) = self.cal.as_ref().map(|c| c.step) else {
            return;
        };
        match step {
            CalStep::Hold => {
                let Some(c) = self.cal.as_mut() else { return };
                if !streaming {
                    if now - c.since > 4_000_000 {
                        c.msg = Some(S::CAL_NO_SENSOR);
                    }
                    // Pointer-only remotes can still set up pointing or try gestures.
                    if now - c.since > 6_000_000 && self.pointer.is_some() {
                        let next = if pointing { CalStep::Left } else { CalStep::Try };
                        self.cal_advance(next);
                    }
                    return;
                }
                c.msg = None;
                let still = f.vel.len() < 0.18 && f.twist_rate_dps.is_none_or(|r| r.abs() < 20.0);
                if still {
                    let s0 = *c.still_since.get_or_insert(now);
                    if now - s0 > HOLD_MS {
                        c.working.neutral = quat;
                        let w = c.working.clone();
                        self.engine.set_calibration(w);
                        self.cal_success(CalStep::Twist);
                    }
                } else {
                    c.still_since = None;
                }
            }
            CalStep::Twist | CalStep::Swing => {
                let Some((best, current, axis)) = probe else {
                    // Arm the measurement after a calm moment (or 1.5 s at the latest).
                    let still = f.vel.len() < 0.25 && f.twist_rate_dps.is_none_or(|r| r.abs() < 30.0);
                    let Some(c) = self.cal.as_mut() else { return };
                    if still {
                        let s0 = *c.still_since.get_or_insert(now);
                        if now - s0 > 200_000 {
                            c.still_since = None;
                            self.engine.start_probe();
                        }
                    } else {
                        c.still_since = None;
                    }
                    if now - c.since > 1_500_000 {
                        self.engine.start_probe();
                    }
                    return;
                };
                let target = if step == CalStep::Twist {
                    TWIST_TARGET_DEG
                } else {
                    SWING_TARGET_DEG
                };
                let Some(c) = self.cal.as_mut() else { return };
                if best < target {
                    if now - c.since > 12_000_000 && c.msg.is_none() {
                        c.msg = Some(S::CAL_RETRY);
                    }
                    return;
                }
                // Decide once the motion turns back (or after a short moment at the peak).
                let t0 = *c.crossed_at.get_or_insert(now);
                if current > best * 0.7 && now - t0 < 400_000 {
                    return;
                }
                let applied = match (step, axis) {
                    (CalStep::Twist, Some(a)) => {
                        apply_twist_probe(&mut c.working, a);
                        true
                    }
                    (CalStep::Swing, Some(a)) => {
                        let ok = apply_swing_probe(&mut c.working, a);
                        if !ok {
                            c.msg = Some(S::CAL_TWISTED);
                        }
                        ok
                    }
                    _ => {
                        c.msg = Some(S::CAL_RETRY);
                        false
                    }
                };
                if applied {
                    let w = c.working.clone();
                    self.engine.set_calibration(w);
                    let next = if step == CalStep::Twist {
                        CalStep::Swing
                    } else {
                        self.after_swing()
                    };
                    self.cal_success(next);
                } else {
                    c.crossed_at = None;
                    c.since = now;
                    self.engine.stop_probe();
                }
            }
            CalStep::Try => self.update_try(now, commits),
            _ => {}
        }
    }

    fn update_try(&mut self, now: u64, commits: &[luma_input::Commit]) {
        let look = self.look();
        let hyst = self.store.thresholds.point_hysteresis_cols;
        let u = if self.pointing_style() {
            let range = self.default_range();
            self.engine.aim_column(COLS as f32, range)
        } else {
            None
        };
        let Some(c) = self.cal.as_mut() else { return };
        let Some(s) = c.try_session.as_mut() else { return };
        let mut sounds = Vec::new();
        for cm in commits {
            let (a, idx) = match cm.gesture {
                Gesture::RotateCw => (Action::RotateCw, 1),
                Gesture::RotateCcw => (Action::RotateCcw, 1),
                Gesture::MoveLeft => (Action::MoveLeft, 0),
                Gesture::MoveRight => (Action::MoveRight, 0),
                Gesture::HardDrop => (Action::HardDrop, 2),
            };
            s.game.apply(a);
            c.tried[idx] = true;
            c.flash = Some((now, cm.gesture.label().to_owned()));
            sounds.push(Sfx::Recognized);
        }
        if let Some(u) = u {
            let p = s.game.active;
            let t = target_left_column(u, view::piece_width(&p), COLS, c.try_target, hyst);
            if c.try_target.is_some_and(|old| old != t) {
                c.tried[0] = true;
            }
            c.try_target = Some(t);
            for _ in 0..2 {
                let lo = s.game.active.column_span().0;
                if lo == t
                    || !s.game.apply(if lo < t {
                        Action::MoveRight
                    } else {
                        Action::MoveLeft
                    })
                {
                    break;
                }
            }
        }
        for e in s.game.drain_events() {
            s.fx.on_event(&e, now, &look);
            if matches!(e, luma_core::Event::Spawned) {
                c.try_target = None;
            }
        }
        for snd in sounds {
            self.audio.play(snd);
        }
    }

    /// Screen position of the live aim dot (pointing steps).
    fn aim_dot(&self) -> Option<(i32, i32)> {
        if self.engine.active_source() == Some(AimSource::Pointer) {
            return self.pointer;
        }
        let x = self.engine.aim_x()?;
        let y = self.engine.debug.last_frame.abs.map(|a| a.y).unwrap_or(0.0);
        Some((
            (W / 2 + (x * 1080.0) as i32).clamp(0, W),
            (H / 2 + (y * 1080.0) as i32).clamp(0, H),
        ))
    }

    pub fn draw_calibration(&mut self, g: &mut Gfx, now: u64) {
        let Some(step) = self.cal.as_ref().map(|c| c.step) else {
            return;
        };
        if step == CalStep::Try {
            return self.draw_try(g, now);
        }
        let pointing = self.pointing_style();
        let steps: Vec<CalStep> = if pointing {
            vec![
                CalStep::Hold,
                CalStep::Twist,
                CalStep::Swing,
                CalStep::Left,
                CalStep::Try,
            ]
        } else {
            vec![CalStep::Hold, CalStep::Twist, CalStep::Swing, CalStep::Try]
        };
        let idx = steps
            .iter()
            .position(|s| *s == step || (step == CalStep::Right && *s == CalStep::Left))
            .unwrap_or(0);
        let c = self.cal.as_ref().unwrap();
        let since_ms = now.saturating_sub(c.since) as f32 / 1000.0;
        let enter = (since_ms / 280.0).min(1.0);
        // Header: title and step progress.
        g.text(S::CAL_TITLE, 160, 90, 30.0, Face::Semi, theme::TEXT_FAINT);
        for (i, _) in steps.iter().enumerate() {
            let col = if i < idx {
                alpha(theme::CYAN, 0.55)
            } else if i == idx {
                theme::CYAN
            } else {
                sdl2::pixels::Color::RGB(40, 44, 52)
            };
            let w = if i == idx { 90 } else { 46 };
            let x = 160
                + steps[..i]
                    .iter()
                    .enumerate()
                    .map(|(j, _)| if j == idx { 104 } else { 60 })
                    .sum::<i32>();
            g.round(Rect::new(x, 146, w, 8), 4, col);
        }
        let (title, body) = match step {
            CalStep::Hold => (S::CAL_HOLD_T, S::CAL_HOLD),
            CalStep::Twist => (S::CAL_TWIST_T, S::CAL_TWIST),
            CalStep::Swing => (S::CAL_SWING_T, S::CAL_SWING),
            CalStep::Left => (S::CAL_LEFT_T, S::CAL_LEFT),
            _ => (S::CAL_RIGHT_T, S::CAL_RIGHT),
        };
        // Text slides in with each new step.
        let slide = ((1.0 - enter) * 40.0) as i32;
        g.text(
            &format!("{} {} / {}", S::CAL_STEP, idx + 1, steps.len()),
            160 + slide,
            200,
            26.0,
            Face::Regular,
            alpha(theme::TEXT_DIM, enter),
        );
        g.text(
            title,
            160 + slide,
            250,
            64.0,
            Face::Light,
            alpha(theme::TEXT, enter),
        );
        g.paragraph(body, 160 + slide, 360, 640, 34.0, alpha(theme::TEXT_DIM, enter));
        if let Some(m) = c.msg {
            let pulse = 0.75 + 0.25 * (now as f32 / 1e6 * 4.0).sin();
            let hgt = g.paragraph(m, 160, 620, 640, 28.0, alpha(theme::WARN, pulse));
            if m == S::CAL_NO_SENSOR {
                if let Some(d) = self.motion_detail() {
                    g.paragraph(&d, 160, 640 + hgt, 640, 22.0, theme::TEXT_FAINT);
                }
            }
        }
        let keys = if matches!(step, CalStep::Left | CalStep::Right) {
            S::CAL_KEYS_EDGE
        } else {
            S::CAL_KEYS_MEASURE
        };
        g.text(keys, 160, H - 110, 26.0, Face::Regular, theme::TEXT_FAINT);
        // Success flourish from the previous step.
        if let Some(t0) = c.ok_at {
            let a = 1.0 - now.saturating_sub(t0) as f32 / 900_000.0;
            if a > 0.0 {
                g.text(S::CAL_GOT_IT, 160, 720, 40.0, Face::Semi, alpha(theme::OK, a));
            }
        }
        // Right side: illustration and live gauge.
        let (cx, cy) = (1340, 440);
        match step {
            CalStep::Hold => {
                illus::draw(g, Illus::Still, cx, cy, 1.6, now);
                let p = c
                    .still_since
                    .map(|s| (now.saturating_sub(s) as f32 / HOLD_MS as f32).min(1.0))
                    .unwrap_or(0.0);
                draw_ring(g, cx, cy, 230, p, now);
                let wob = self.engine.debug.last_frame.vel.len();
                let label = if self.motion.is_streaming(now) {
                    format!("{}  {:.0}%", S::CAL_HOLD_LIVE, p * 100.0)
                } else {
                    S::DETECTING.to_owned()
                };
                g.text_centered(&label, cx, cy + 270, 28.0, Face::Regular, theme::TEXT_DIM);
                // Live steadiness bar: how much the remote is moving right now.
                let w = ((wob / 0.5).min(1.0) * 300.0) as u32;
                g.round(
                    Rect::new(cx - 150, cy + 320, 300, 6),
                    3,
                    sdl2::pixels::Color::RGB(30, 33, 40),
                );
                g.round(
                    Rect::new(cx - 150, cy + 320, w.max(6), 6),
                    3,
                    alpha(theme::CORAL, 0.8),
                );
            }
            CalStep::Twist => {
                illus::draw(g, Illus::Twist, cx, cy - 20, 1.7, now);
                let (best, cur) = self
                    .engine
                    .probe()
                    .map(|p| (p.angle_deg(), p.current_deg()))
                    .unwrap_or((0.0, 0.0));
                draw_gauge(g, cx, cy + 290, cur.max(0.0), best, TWIST_TARGET_DEG, now);
            }
            CalStep::Swing => {
                illus::draw(g, Illus::Swing, cx, cy - 20, 1.6, now);
                let (best, cur) = self
                    .engine
                    .probe()
                    .map(|p| (p.angle_deg(), p.current_deg()))
                    .unwrap_or((0.0, 0.0));
                draw_gauge(g, cx, cy + 290, cur.max(0.0), best, SWING_TARGET_DEG, now);
            }
            CalStep::Left | CalStep::Right => {
                let r = Rect::new(BOARD_X + 380, BOARD_Y, BOARD_W as u32, BOARD_H as u32);
                g.round_outline(r, 14, theme::LINE, 2);
                let ex = if step == CalStep::Left { r.x() } else { r.right() };
                let pulse = 0.6 + 0.4 * (now as f32 / 1e6 * 5.0).sin();
                g.round(
                    Rect::new(ex - 4, r.y(), 8, r.height()),
                    4,
                    alpha(theme::CYAN, pulse),
                );
                if let Some((px, py)) = self.aim_dot() {
                    draw_cursor(g, px, py.clamp(r.y() + 20, r.bottom() - 20), 1.4, now);
                }
            }
            CalStep::Try => {}
        }
    }

    fn draw_try(&mut self, g: &mut Gfx, now: u64) {
        let look = self.look();
        let Some(c) = self.cal.as_mut() else { return };
        if let Some(s) = c.try_session.as_mut() {
            let input = view::BoardInput {
                game: &s.game,
                look,
                now,
                target: c.try_target.map(|t| (t, view::piece_width(&s.game.active), true)),
                best: None,
                hide_hud: true,
            };
            view::draw_game(g, &mut s.fx, &input);
        }
        let tried = c.tried;
        let flash = c.flash.clone();
        g.text(S::CAL_TRY_T, 120, 120, 56.0, Face::Light, theme::TEXT);
        g.paragraph(S::CAL_TRY, 120, 210, 520, 28.0, theme::TEXT_DIM);
        let p = self.store.settings.profile;
        let rows: [(Illus, &str); 3] = [
            (
                match p.horizontal {
                    HorizontalStyle::Swing => Illus::Swing,
                    HorizontalStyle::Pointing => Illus::Point,
                    HorizontalStyle::Flick => Illus::Flick,
                },
                S::TRY_MOVE,
            ),
            (
                match p.rotation {
                    RotationStyle::Twist => Illus::Twist,
                    RotationStyle::Circle => Illus::Circle,
                },
                S::TRY_ROT,
            ),
            (Illus::Drop, S::TRY_DROP_SHORT),
        ];
        for (i, (il, label)) in rows.iter().enumerate() {
            let y = 360 + i as i32 * 190;
            let card = Rect::new(110, y, 540, 170);
            g.round(card, 22, if tried[i] { theme::PANEL_HI } else { theme::PANEL });
            if tried[i] {
                g.round_outline(card, 22, alpha(theme::OK, 0.7), 2);
            }
            illus::draw(g, *il, 230, y + 90, 0.62, now);
            g.text(
                label,
                360,
                y + 58,
                32.0,
                Face::Regular,
                if tried[i] { theme::TEXT } else { theme::TEXT_DIM },
            );
            if tried[i] {
                g.text("✓", 600, y + 52, 40.0, Face::Semi, theme::OK);
            }
        }
        // Recognized-gesture flash next to the board.
        if let Some((t0, text)) = flash {
            let a = 1.0 - now.saturating_sub(t0) as f32 / 1_200_000.0;
            if a > 0.0 {
                let rise = ((1.0 - a) * 30.0) as i32;
                g.text_centered(
                    &text,
                    BOARD_X + BOARD_W / 2,
                    BOARD_Y + BOARD_H / 3 - rise,
                    40.0,
                    Face::Semi,
                    alpha(theme::CYAN, a),
                );
            }
        }
        let x = BOARD_X + BOARD_W + 80;
        let mut y = 380;
        for (line, c) in [
            (S::CAL_DONE_BTN, theme::CYAN),
            (S::CAL_FLIP_TWIST, theme::TEXT_DIM),
            (S::CAL_FLIP_SIDE, theme::TEXT_DIM),
            (S::CAL_FLIP_DROP, theme::TEXT_DIM),
            (S::CAL_REDO, theme::TEXT_FAINT),
        ] {
            g.text(line, x, y, 28.0, Face::Regular, c);
            y += 56;
        }
    }

    // ------------------------------------------------------------------ onboarding

    pub fn start_onboarding(&mut self) {
        self.onb = Some(Onboard {
            step: OnbStep::Welcome,
            since: now_us(),
            row: 0,
        });
        self.go(Screen::Onboard);
    }

    fn onboard_finish(&mut self, play: bool) {
        self.store.settings.first_run_done = true;
        self.settings_changed();
        self.store.save_settings();
        self.onb = None;
        if play {
            self.request_mode(Mode::Marathon);
        } else {
            self.go(Screen::Menu);
        }
    }

    fn welcome_items(&self) -> Vec<Item> {
        let detected = self.onb.as_ref().is_some_and(|o| now_us() - o.since > 2_500_000)
            || self.motion.is_streaming(now_us());
        vec![
            Item::action(S::USE_MOTION).disabled(!detected),
            Item::action(S::USE_BUTTONS),
            Item::action(S::SKIP),
        ]
    }

    pub fn onboard_input(&mut self, inp: &In) {
        let Some(step) = self.onb.as_ref().map(|o| o.step) else {
            return;
        };
        let Some(u) = crate::app::to_ui(inp) else {
            return;
        };
        match step {
            OnbStep::Welcome => {
                let items = self.welcome_items();
                match self.list.handle(&items, u) {
                    Ev::Activate(0) => {
                        self.store.settings.control = ControlMode::Motion;
                        // Wrist twist needs the sensor: preselect circles on pointer-only remotes.
                        if self.motion.is_real() && !self.motion.is_streaming(now_us()) {
                            self.store.settings.profile.rotation = RotationStyle::Circle;
                        }
                        self.settings_changed();
                        if let Some(o) = self.onb.as_mut() {
                            o.step = OnbStep::Style;
                            o.since = now_us();
                            o.row = 0;
                        }
                        self.sfx(Sfx::MenuSelect);
                    }
                    Ev::Activate(1) => {
                        self.store.settings.control = ControlMode::Buttons;
                        self.onboard_finish(false);
                    }
                    Ev::Activate(_) | Ev::Back => self.onboard_finish(false),
                    Ev::Moved => self.sfx(Sfx::MenuMove),
                    _ => {}
                }
            }
            OnbStep::Style => {
                let twist_ok = !self.motion.is_real() || self.motion.is_streaming(now_us());
                let row = self.onb.as_ref().map(|o| o.row).unwrap_or(0);
                match u {
                    UiIn::Press(Button::Up) => {
                        if let Some(o) = self.onb.as_mut() {
                            o.row = o.row.saturating_sub(1);
                        }
                        self.sfx(Sfx::MenuMove);
                    }
                    UiIn::Press(Button::Down) => {
                        if let Some(o) = self.onb.as_mut() {
                            o.row = (o.row + 1).min(2);
                        }
                        self.sfx(Sfx::MenuMove);
                    }
                    UiIn::Press(Button::Left) | UiIn::Press(Button::Right) => {
                        let d = if u == UiIn::Press(Button::Left) { -1 } else { 1 };
                        self.step_style(row, d, twist_ok);
                        self.sfx(Sfx::MenuMove);
                    }
                    UiIn::Pointer(x, y) => {
                        for (r, _, rect) in style_cards() {
                            if rect.contains_point((x, y)) {
                                if let Some(o) = self.onb.as_mut() {
                                    o.row = r;
                                }
                            }
                        }
                        if continue_rect().contains_point((x, y)) {
                            if let Some(o) = self.onb.as_mut() {
                                o.row = 2;
                            }
                        }
                    }
                    UiIn::Click(x, y) => {
                        for (r, col, rect) in style_cards() {
                            if rect.contains_point((x, y)) {
                                self.choose_style(r, col, twist_ok);
                            }
                        }
                        if continue_rect().contains_point((x, y)) {
                            self.start_calibration(Screen::Onboard);
                        }
                    }
                    UiIn::Press(Button::Ok) => {
                        if row == 2 {
                            self.start_calibration(Screen::Onboard);
                        } else if let Some(o) = self.onb.as_mut() {
                            o.row += 1;
                        }
                    }
                    UiIn::Press(Button::Back) => {
                        if let Some(o) = self.onb.as_mut() {
                            o.step = OnbStep::Welcome;
                            o.since = now_us();
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn step_style(&mut self, row: usize, d: i32, twist_ok: bool) {
        let p = self.store.settings.profile;
        let cur = if row == 0 {
            (p.rotation == RotationStyle::Circle) as i32
        } else {
            match p.horizontal {
                HorizontalStyle::Swing => 0,
                HorizontalStyle::Pointing => 1,
                HorizontalStyle::Flick => 2,
            }
        };
        let n = if row == 0 { 2 } else { 3 };
        self.choose_style(row, (cur + d).clamp(0, n - 1) as usize, twist_ok);
    }

    fn choose_style(&mut self, row: usize, col: usize, twist_ok: bool) {
        let p = &mut self.store.settings.profile;
        if row == 0 {
            p.rotation = if col == 0 && twist_ok {
                RotationStyle::Twist
            } else {
                RotationStyle::Circle
            };
        } else if row == 1 {
            p.horizontal = match col {
                0 => HorizontalStyle::Swing,
                1 => HorizontalStyle::Pointing,
                _ => HorizontalStyle::Flick,
            };
        }
        self.settings_changed();
    }

    pub fn draw_onboarding(&mut self, g: &mut Gfx, now: u64) {
        let Some((step, since)) = self.onb.as_ref().map(|o| (o.step, o.since)) else {
            return;
        };
        let enter = (now.saturating_sub(since) as f32 / 350_000.0).min(1.0);
        match step {
            OnbStep::Welcome => {
                crate::screens::draw_brand(g, now, enter);
                g.text(S::WELCOME, 220, 330, 50.0, Face::Light, alpha(theme::TEXT, enter));
                let streaming = self.motion.is_streaming(now);
                let status = if streaming {
                    S::MOTION_FOUND
                } else if now - since < 2_500_000 {
                    S::DETECTING
                } else if self.pointer.is_some() {
                    S::MOTION_POINTER_ONLY
                } else {
                    S::MOTION_NONE
                };
                g.paragraph(status, 220, 420, 900, 30.0, theme::TEXT_DIM);
                let items = self.welcome_items();
                self.list.draw(g, &items, 220, 560, 640, 4);
                // Ambient preview of the twist on the right.
                illus::draw(g, Illus::Twist, 1450, 560, 1.8, now);
            }
            OnbStep::Style => {
                g.text(
                    S::CHOOSE_STYLE,
                    160,
                    70,
                    50.0,
                    Face::Light,
                    alpha(theme::TEXT, enter),
                );
                let p = self.store.settings.profile;
                let row = self.onb.as_ref().map(|o| o.row).unwrap_or(0);
                let twist_ok = !self.motion.is_real() || self.motion.is_streaming(now);
                g.text(S::ROTATION, 160, 170, 26.0, Face::Semi, theme::TEXT_FAINT);
                g.text(S::SIDEWAYS, 160, 545, 26.0, Face::Semi, theme::TEXT_FAINT);
                for (r, col, rect) in style_cards() {
                    let selected = if r == 0 {
                        (p.rotation == RotationStyle::Twist) == (col == 0)
                    } else {
                        matches!(
                            (p.horizontal, col),
                            (HorizontalStyle::Swing, 0)
                                | (HorizontalStyle::Pointing, 1)
                                | (HorizontalStyle::Flick, 2)
                        )
                    };
                    g.round(rect, 24, if selected { theme::PANEL_HI } else { theme::PANEL });
                    if selected {
                        g.round_outline(
                            rect,
                            24,
                            alpha(theme::CYAN, if row == r { 0.95 } else { 0.45 }),
                            3,
                        );
                    }
                    let (il, label, hint) = match (r, col) {
                        (0, 0) => (Illus::Twist, S::ROT_TWIST, ""),
                        (0, _) => (Illus::Circle, S::ROT_CIRCLE, ""),
                        (_, 0) => (Illus::Swing, S::SIDE_SWING, S::SIDE_SWING_HINT),
                        (_, 1) => (Illus::Point, S::SIDE_POINT, S::SIDE_POINT_HINT),
                        _ => (Illus::Flick, S::SIDE_FLICK, S::SIDE_FLICK_HINT),
                    };
                    // Only the selected card animates; the others show a still frame.
                    let t = if selected { now } else { 300_000 };
                    illus::draw(g, il, rect.center().x(), rect.y() + 145, 0.85, t);
                    let disabled = r == 0 && col == 0 && !twist_ok;
                    g.text_centered(
                        label,
                        rect.center().x(),
                        rect.bottom() - 74,
                        30.0,
                        Face::Regular,
                        if disabled { theme::TEXT_FAINT } else { theme::TEXT },
                    );
                    let hint = if disabled { S::UNAVAILABLE_TWIST } else { hint };
                    g.text_centered(
                        hint,
                        rect.center().x(),
                        rect.bottom() - 38,
                        20.0,
                        Face::Regular,
                        theme::TEXT_FAINT,
                    );
                }
                let cr = continue_rect();
                if row == 2 {
                    g.round(cr, 30, theme::PANEL_HI);
                    g.round_outline(cr, 30, theme::CYAN, 3);
                }
                g.text_centered(
                    S::NEXT,
                    cr.center().x(),
                    cr.y() + 13,
                    32.0,
                    Face::Regular,
                    theme::TEXT,
                );
            }
        }
    }
}

fn style_cards() -> Vec<(usize, usize, Rect)> {
    let mut v = Vec::new();
    for c in 0..2 {
        v.push((0, c, Rect::new(160 + c as i32 * 420, 205, 390, 320)));
    }
    for c in 0..3 {
        v.push((1, c, Rect::new(160 + c as i32 * 420, 580, 390, 320)));
    }
    v
}

fn continue_rect() -> Rect {
    Rect::new(W - 160 - 420, 960, 420, 60)
}

/// Dotted progress ring that breathes slightly while waiting.
fn draw_ring(g: &mut Gfx, cx: i32, cy: i32, r: i32, p: f32, now: u64) {
    let n = 60;
    let breathe = 1.0 + 0.02 * (now as f32 / 1e6 * 2.0).sin();
    for i in 0..n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        let on = (i as f32) < p * n as f32;
        let rr = r as f32 * breathe;
        let x = cx + (a.cos() * rr) as i32;
        let y = cy + (a.sin() * rr) as i32;
        let s = if on { 14 } else { 7 };
        let col = if on {
            theme::CYAN
        } else {
            sdl2::pixels::Color::RGB(40, 44, 52)
        };
        let dst = Rect::new(x - s / 2, y - s / 2, s as u32, s as u32);
        let canvas = &mut g.canvas;
        Gfx::tinted(&mut g.tex.dot, canvas, dst, col, on);
    }
}

/// Horizontal gauge: current excursion (bright), best so far (dim) and the target tick.
fn draw_gauge(g: &mut Gfx, cx: i32, y: i32, current: f32, best: f32, target: f32, now: u64) {
    let w = 520;
    let x0 = cx - w / 2;
    let full = target * 1.6;
    let px = |v: f32| x0 + ((v / full).clamp(0.0, 1.0) * w as f32) as i32;
    g.round(
        Rect::new(x0, y, w as u32, 12),
        6,
        sdl2::pixels::Color::RGB(30, 33, 40),
    );
    g.round(
        Rect::new(x0, y, (px(best) - x0).max(12) as u32, 12),
        6,
        alpha(theme::CYAN, 0.3),
    );
    g.round(
        Rect::new(x0, y, (px(current) - x0).max(12) as u32, 12),
        6,
        theme::CYAN,
    );
    let tx = px(target);
    let reached = best >= target;
    let pulse = if reached {
        1.0
    } else {
        0.6 + 0.4 * (now as f32 / 1e6 * 4.0).sin()
    };
    g.round(
        Rect::new(tx - 2, y - 14, 4, 40),
        2,
        alpha(if reached { theme::OK } else { theme::TEXT }, pulse),
    );
    g.text_centered(
        &format!("{:.0}°", best.max(current)),
        cx,
        y + 30,
        26.0,
        Face::Regular,
        theme::TEXT_DIM,
    );
}

/// The app's own pointer: a soft ring with a bright centre.
pub fn draw_cursor(g: &mut Gfx, x: i32, y: i32, s: f32, now: u64) {
    let pulse = 1.0 + 0.06 * (now as f32 / 1e6 * 3.0).sin();
    let r = (34.0 * s * pulse) as i32;
    let canvas = &mut g.canvas;
    Gfx::tinted(
        &mut g.tex.glow,
        canvas,
        Rect::new(x - r * 2, y - r * 2, (r * 4) as u32, (r * 4) as u32),
        alpha(theme::CYAN, 0.22),
        true,
    );
    let d = (18.0 * s) as i32;
    Gfx::tinted(
        &mut g.tex.dot,
        canvas,
        Rect::new(x - d, y - d, (d * 2) as u32, (d * 2) as u32),
        theme::CYAN,
        true,
    );
    Gfx::tinted(
        &mut g.tex.dot,
        canvas,
        Rect::new(x - d / 3, y - d / 3, (d * 2 / 3) as u32, (d * 2 / 3) as u32),
        theme::TEXT,
        false,
    );
}
