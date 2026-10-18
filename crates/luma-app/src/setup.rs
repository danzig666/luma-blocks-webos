//! First-run onboarding and the guided calibration flow.
//!
//! Calibration (sensor available): neutral pose -> clockwise twist (finds the long axis and
//! the clockwise sign) -> tip up (finds the pitch axis) -> left and right board edges.
//! Pointer only: the two edge steps. Every step can be skipped with Back.

use crate::app::{App, Screen, Session};
use crate::audio::Sfx;
use crate::gfx::{alpha, theme, Face, Gfx, W};
use crate::keys::In;
use crate::platform::now_us;
use crate::store::ControlMode;
use crate::strings as S;
use crate::ui::{Ev, Item, UiIn};
use crate::view::{self, BOARD_W, BOARD_X, BOARD_Y};
use luma_core::game::COLS;
use luma_core::{Action, Game, Mode, Rules};
use luma_input::buttons::Button;
use luma_input::calib::{
    apply_pitch_probe, apply_twist_probe, AimSource, AxisProbe, Calibration, PointRange,
};
use luma_input::config::{HorizontalStyle, RotationStyle};
use luma_input::engine::target_left_column;
use luma_input::Gesture;
use sdl2::rect::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalStep {
    Neutral,
    Twist,
    Tilt,
    Left,
    Right,
    Done,
}

pub struct Calib {
    pub step: CalStep,
    pub return_to: Screen,
    since: u64,
    still_since: Option<u64>,
    probe: AxisProbe,
    crossed_at: Option<u64>,
    pub msg: Option<&'static str>,
    left: Option<f32>,
    working: Calibration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnbStep {
    Welcome,
    Style,
    Try,
}

pub struct Onboard {
    pub step: OnbStep,
    since: u64,
    row: usize,
    tried: [bool; 3],
    session: Option<Session>,
    target: Option<i32>,
    last_col: Option<f32>,
    moved: f32,
}

impl App {
    pub fn start_calibration(&mut self, return_to: Screen) {
        self.cal = Some(Calib {
            step: CalStep::Neutral,
            return_to,
            since: now_us(),
            still_since: None,
            probe: AxisProbe::default(),
            crossed_at: None,
            msg: None,
            left: None,
            working: self.store.calibration.clone(),
        });
        self.engine.reset();
        self.go(Screen::Calibrate);
    }

    fn cal_advance(&mut self, next: CalStep) {
        if let Some(c) = self.cal.as_mut() {
            c.step = next;
            c.since = now_us();
            c.still_since = None;
            c.probe = AxisProbe::default();
            c.crossed_at = None;
            c.msg = None;
        }
        if next == CalStep::Done {
            if let Some(c) = self.cal.as_ref() {
                self.store.calibration = c.working.clone();
                self.store.save_calibration();
                self.engine.set_calibration(c.working.clone());
            }
            self.sfx(Sfx::MenuSelect);
        } else {
            self.sfx(Sfx::Recognized);
        }
    }

    fn cal_finish(&mut self) {
        let back = self.cal.as_ref().map(|c| c.return_to).unwrap_or(Screen::Menu);
        self.cal = None;
        if back == Screen::Onboard {
            self.go(Screen::Onboard);
            self.onboard_try_begin();
        } else {
            self.go(back);
        }
    }

    pub fn calib_input(&mut self, inp: &In) {
        let Some(step) = self.cal.as_ref().map(|c| c.step) else {
            return;
        };
        match inp {
            In::Button {
                b: Button::Back,
                down: true,
                ..
            } => {
                let next = match step {
                    CalStep::Neutral => CalStep::Twist,
                    CalStep::Twist => CalStep::Tilt,
                    CalStep::Tilt => CalStep::Left,
                    CalStep::Left | CalStep::Right => CalStep::Done,
                    CalStep::Done => return self.cal_finish(),
                };
                let sensor = self.motion.is_streaming(now_us());
                let next = if !sensor && matches!(next, CalStep::Twist | CalStep::Tilt) {
                    CalStep::Left
                } else {
                    next
                };
                self.cal_advance(next);
            }
            In::Button {
                b: Button::Ok,
                down: true,
                ..
            }
            | In::Click {
                left: true,
                down: true,
                ..
            } => match step {
                CalStep::Left | CalStep::Right => {
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
                        self.cal_advance(CalStep::Right);
                    } else {
                        let src = self.engine.active_source();
                        let ok = if let Some(c) = self.cal.as_mut() {
                            let l = c.left.unwrap_or(x - 0.5);
                            let mid = (l + x) / 2.0;
                            let center = if src == Some(AimSource::Sensor) && l < 0.0 && x > 0.0 {
                                0.0
                            } else {
                                mid
                            };
                            let r = PointRange {
                                left: l,
                                center,
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
                        } else {
                            false
                        };
                        if ok {
                            self.cal_advance(CalStep::Done);
                        }
                    }
                }
                CalStep::Done => self.cal_finish(),
                _ => {}
            },
            _ => {}
        }
    }

    pub fn update_calibration(&mut self, now: u64) {
        let streaming = self.motion.is_streaming(now);
        let q = self.engine.last_quat();
        let f = self.engine.debug.last_frame;
        let Some(c) = self.cal.as_mut() else { return };
        match c.step {
            CalStep::Neutral => {
                if !streaming {
                    if now - c.since > 4_000_000 {
                        // No sensor: only the pointer range can be calibrated.
                        self.cal_advance(CalStep::Left);
                    }
                    return;
                }
                let still = f.vel.len() < 0.12 && f.twist_rate_dps.is_none_or(|r| r.abs() < 15.0);
                if still {
                    let s0 = *c.still_since.get_or_insert(now);
                    if now - s0 > 900_000 {
                        c.working.neutral = q;
                        let w = c.working.clone();
                        self.engine.set_calibration(w);
                        self.cal_advance(CalStep::Twist);
                    }
                } else {
                    c.still_since = None;
                }
            }
            CalStep::Twist | CalStep::Tilt => {
                let Some(q) = q else { return };
                c.probe.push(q);
                if c.probe.angle_deg() >= 35.0 {
                    let t0 = *c.crossed_at.get_or_insert(now);
                    if now - t0 > 250_000 {
                        let axis = c.probe.axis();
                        let ok = match (c.step, axis) {
                            (CalStep::Twist, Some(a)) => {
                                apply_twist_probe(&mut c.working, a);
                                true
                            }
                            (CalStep::Tilt, Some(a)) => apply_pitch_probe(&mut c.working, a),
                            _ => false,
                        };
                        if ok {
                            let step = c.step;
                            let w = c.working.clone();
                            self.engine.set_calibration(w);
                            self.cal_advance(if step == CalStep::Twist {
                                CalStep::Tilt
                            } else {
                                CalStep::Left
                            });
                        } else {
                            c.msg = Some(S::CAL_RETRY);
                            c.probe = AxisProbe::default();
                            c.crossed_at = None;
                        }
                    }
                } else if now - c.since > 12_000_000 && c.msg.is_none() {
                    c.msg = Some(S::CAL_RETRY);
                }
            }
            CalStep::Done if now - c.since > 1_400_000 => self.cal_finish(),
            _ => {}
        }
    }

    pub fn draw_calibration(&mut self, g: &mut Gfx, now: u64) {
        let Some(c) = self.cal.as_ref() else { return };
        g.text(S::CAL_TITLE, 220, 90, 54.0, Face::Light, theme::TEXT);
        let steps = [
            CalStep::Neutral,
            CalStep::Twist,
            CalStep::Tilt,
            CalStep::Left,
            CalStep::Right,
        ];
        for (i, s) in steps.iter().enumerate() {
            let on = *s == c.step;
            let done = steps.iter().position(|x| *x == c.step).unwrap_or(5) > i || c.step == CalStep::Done;
            let col = if on {
                theme::CYAN
            } else if done {
                theme::TEXT_DIM
            } else {
                sdl2::pixels::Color::RGB(40, 44, 52)
            };
            g.round(Rect::new(220 + i as i32 * 70, 180, 50, 6), 3, col);
        }
        let text = match c.step {
            CalStep::Neutral => S::CAL_NEUTRAL,
            CalStep::Twist => S::CAL_TWIST,
            CalStep::Tilt => S::CAL_TILT,
            CalStep::Left => S::CAL_LEFT,
            CalStep::Right => S::CAL_RIGHT,
            CalStep::Done => S::CAL_DONE,
        };
        g.paragraph(text, 220, 240, 600, 34.0, theme::TEXT);
        if let Some(m) = c.msg {
            g.paragraph(m, 220, 420, 600, 28.0, theme::WARN);
        }
        g.text(S::CAL_SKIP_HINT, 220, 980, 24.0, Face::Regular, theme::TEXT_FAINT);
        // Live feedback on the right.
        match c.step {
            CalStep::Neutral => {
                let p = c
                    .still_since
                    .map(|s| ((now - s) as f32 / 900_000.0).min(1.0))
                    .unwrap_or(0.0);
                draw_ring(g, 1300, 500, 120, p);
                if !self.motion.is_streaming(now) {
                    g.text_centered(S::DETECTING, 1300, 700, 28.0, Face::Regular, theme::TEXT_DIM);
                }
            }
            CalStep::Twist | CalStep::Tilt => {
                let p = (c.probe.angle_deg() / 35.0).min(1.0);
                draw_ring(g, 1300, 500, 120, p);
                let kind = if c.step == CalStep::Twist { 0 } else { 2 };
                if kind == 0 {
                    view::draw_illustration(g, 0, 1300, 500, now);
                }
            }
            CalStep::Left | CalStep::Right => {
                // Board outline with the edge to point at.
                let r = Rect::new(BOARD_X + 300, BOARD_Y, BOARD_W as u32, view::BOARD_H as u32);
                g.round_outline(r, 14, theme::LINE, 2);
                let ex = if c.step == CalStep::Left { r.x() } else { r.right() };
                g.round(
                    Rect::new(ex - 3, r.y(), 6, r.height()),
                    3,
                    alpha(theme::CYAN, 0.8),
                );
                if let Some(x) = self.engine.aim_x() {
                    let u = x; // live aim (screen heights)
                    let px = if self.engine.active_source() == Some(AimSource::Pointer) {
                        (u * 1080.0) as i32
                    } else {
                        r.center().x() + (u * 1080.0 * 0.8) as i32
                    };
                    let dst = Rect::new(px - 10, r.y() + r.height() as i32 / 2 - 10, 20, 20);
                    let canvas = &mut g.canvas;
                    Gfx::tinted(&mut g.tex.dot, canvas, dst, theme::TEXT, false);
                }
            }
            CalStep::Done => draw_ring(g, 1300, 500, 120, 1.0),
        }
    }

    // ------------------------------------------------------------------ onboarding

    pub fn start_onboarding(&mut self) {
        self.onb = Some(Onboard {
            step: OnbStep::Welcome,
            since: now_us(),
            row: 0,
            tried: [false; 3],
            session: None,
            target: None,
            last_col: None,
            moved: 0.0,
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

    fn onboard_try_begin(&mut self) {
        let game = Game::new(Mode::Practice, Rules::default(), 0x7E57);
        let sess = Session::new(game, &self.store);
        if let Some(o) = self.onb.as_mut() {
            o.step = OnbStep::Try;
            o.since = now_us();
            o.session = Some(sess);
            o.tried = [false; 3];
        }
        self.engine.reset();
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
        let Some(u) = crate::app::to_ui(inp) else { return };
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
                    }
                    UiIn::Press(Button::Down) => {
                        if let Some(o) = self.onb.as_mut() {
                            o.row = (o.row + 1).min(2);
                        }
                    }
                    UiIn::Press(Button::Left) | UiIn::Press(Button::Right) => {
                        let left = u == UiIn::Press(Button::Left);
                        self.choose_style(row, if left { 0 } else { 1 }, twist_ok);
                        self.sfx(Sfx::MenuMove);
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
                        }
                    }
                    _ => {}
                }
            }
            OnbStep::Try => match u {
                UiIn::Press(Button::Ok) | UiIn::Click(..) => self.onboard_finish(true),
                UiIn::Press(Button::Back) => self.onboard_finish(false),
                _ => {}
            },
        }
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
            p.horizontal = if col == 0 {
                HorizontalStyle::Pointing
            } else {
                HorizontalStyle::Flick
            };
        }
        self.settings_changed();
    }

    pub fn update_onboarding(&mut self, now: u64, commits: &[luma_input::Commit]) {
        let look = self.look();
        let pointing = self.store.settings.profile.horizontal == HorizontalStyle::Pointing;
        let u = if pointing {
            let range = PointRange {
                left: -0.32,
                center: 0.0,
                right: 0.32,
            };
            let range = match self.engine.active_source() {
                Some(AimSource::Pointer) => {
                    let l = BOARD_X as f32 / 1080.0;
                    let r = (BOARD_X + BOARD_W) as f32 / 1080.0;
                    PointRange {
                        left: l,
                        center: (l + r) / 2.0,
                        right: r,
                    }
                }
                _ => range,
            };
            self.engine.aim_column(COLS as f32, range)
        } else {
            None
        };
        let hyst = self.store.thresholds.point_hysteresis_cols;
        let Some(o) = self.onb.as_mut() else { return };
        if o.step != OnbStep::Try {
            return;
        }
        let Some(s) = o.session.as_mut() else { return };
        let mut sounds = Vec::new();
        for c in commits {
            let a = match c.gesture {
                Gesture::RotateCw => Action::RotateCw,
                Gesture::RotateCcw => Action::RotateCcw,
                Gesture::MoveLeft => Action::MoveLeft,
                Gesture::MoveRight => Action::MoveRight,
                Gesture::HardDrop => Action::HardDrop,
            };
            s.game.apply(a);
            match c.gesture {
                Gesture::RotateCw | Gesture::RotateCcw => o.tried[1] = true,
                Gesture::MoveLeft | Gesture::MoveRight => o.tried[0] = true,
                Gesture::HardDrop => o.tried[2] = true,
            }
            sounds.push(Sfx::Recognized);
        }
        if let Some(u) = u {
            if let Some(l) = o.last_col {
                o.moved += (u - l).abs();
                if o.moved > 2.5 {
                    o.tried[0] = true;
                }
            }
            o.last_col = Some(u);
            let p = s.game.active;
            let w = view::piece_width(&p);
            let t = target_left_column(u, w, COLS, o.target, hyst);
            o.target = Some(t);
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
                o.target = None;
            }
        }
        for snd in sounds {
            self.audio.play(snd);
        }
    }

    pub fn draw_onboarding(&mut self, g: &mut Gfx, now: u64) {
        let Some(step) = self.onb.as_ref().map(|o| o.step) else {
            return;
        };
        match step {
            OnbStep::Welcome => {
                crate::screens::draw_brand(g, now, 1.0);
                g.text(S::WELCOME, 220, 330, 50.0, Face::Light, theme::TEXT);
                let since = self.onb.as_ref().map(|o| now - o.since).unwrap_or(0);
                let streaming = self.motion.is_streaming(now);
                let status = if streaming {
                    S::MOTION_FOUND
                } else if since < 2_500_000 {
                    S::DETECTING
                } else if self.pointer.is_some() {
                    S::MOTION_POINTER_ONLY
                } else {
                    S::MOTION_NONE
                };
                g.paragraph(status, 220, 420, 900, 30.0, theme::TEXT_DIM);
                let items = self.welcome_items();
                self.list.draw(g, &items, 220, 560, 640, 4);
            }
            OnbStep::Style => {
                g.text(S::CHOOSE_STYLE, 220, 90, 50.0, Face::Light, theme::TEXT);
                let p = self.store.settings.profile;
                let row = self.onb.as_ref().map(|o| o.row).unwrap_or(0);
                let twist_ok = !self.motion.is_real() || self.motion.is_streaming(now);
                let labels = [[S::ROT_TWIST, S::ROT_CIRCLE], [S::SIDE_POINT, S::SIDE_FLICK]];
                g.text(S::ROTATION, 220, 210, 26.0, Face::Semi, theme::TEXT_FAINT);
                g.text(S::SIDEWAYS, 220, 560, 26.0, Face::Semi, theme::TEXT_FAINT);
                for (r, col, rect) in style_cards() {
                    let selected = if r == 0 {
                        (p.rotation == RotationStyle::Twist) == (col == 0)
                    } else {
                        (p.horizontal == HorizontalStyle::Pointing) == (col == 0)
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
                    let kind = (r * 2 + col) as u8;
                    view::draw_illustration(g, kind, rect.center().x(), rect.y() + 120, now);
                    let disabled = r == 0 && col == 0 && !twist_ok;
                    g.text_centered(
                        labels[r][col],
                        rect.center().x(),
                        rect.bottom() - 70,
                        30.0,
                        Face::Regular,
                        if disabled { theme::TEXT_FAINT } else { theme::TEXT },
                    );
                    if disabled {
                        g.text_centered(
                            S::UNAVAILABLE_TWIST,
                            rect.center().x(),
                            rect.bottom() - 34,
                            20.0,
                            Face::Regular,
                            theme::TEXT_FAINT,
                        );
                    }
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
            OnbStep::Try => {
                let look = self.look();
                if let Some(o) = self.onb.as_mut() {
                    if let Some(s) = o.session.as_mut() {
                        let input = view::BoardInput {
                            game: &s.game,
                            look,
                            now,
                            target: None,
                            best: None,
                        };
                        view::draw_game(g, &mut s.fx, &input);
                    }
                }
                let tried = self.onb.as_ref().map(|o| o.tried).unwrap_or_default();
                g.text(S::TRY_TITLE, 120, 120, 44.0, Face::Light, theme::TEXT);
                let lines = [S::TRY_SIDEWAYS, S::TRY_ROTATE, S::TRY_DROP];
                for (i, l) in lines.iter().enumerate() {
                    let y = 220 + i as i32 * 70;
                    let c = if tried[i] { theme::OK } else { theme::TEXT_FAINT };
                    g.round(Rect::new(120, y + 8, 22, 22), 11, c);
                    g.paragraph(
                        l,
                        160,
                        y,
                        380,
                        26.0,
                        if tried[i] { theme::TEXT } else { theme::TEXT_DIM },
                    );
                }
                g.text(
                    &format!("OK  {}", S::START_PLAYING),
                    120,
                    900,
                    28.0,
                    Face::Regular,
                    theme::CYAN,
                );
                g.text(
                    &format!("Back  {}", S::SKIP),
                    120,
                    950,
                    24.0,
                    Face::Regular,
                    theme::TEXT_FAINT,
                );
            }
        }
        let _ = W;
    }
}

fn style_cards() -> Vec<(usize, usize, Rect)> {
    let mut v = Vec::new();
    for r in 0..2 {
        for c in 0..2 {
            v.push((
                r,
                c,
                Rect::new(220 + c as i32 * 440, 250 + r as i32 * 350, 400, 290),
            ));
        }
    }
    v
}

fn continue_rect() -> Rect {
    Rect::new(1200, 900, 420, 60)
}

fn draw_ring(g: &mut Gfx, cx: i32, cy: i32, r: i32, p: f32) {
    let n = 48;
    for i in 0..n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        let on = (i as f32) < p * n as f32;
        let x = cx + (a.cos() * r as f32) as i32;
        let y = cy + (a.sin() * r as f32) as i32;
        let s = if on { 14 } else { 8 };
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
