//! Playfield rendering and short-lived visual effects. The simulation stays grid-accurate:
//! effects only decorate what the engine already decided (instant clears, immediate locks),
//! so no rule timing ever waits on an animation.

use crate::gfx::{alpha, scale, theme, Face, Gfx, H, W};
use crate::strings as S;
use luma_core::game::{Event, Game, Mode, COLS, HIDDEN_ROWS, ROWS, SPRINT_LINES};
use luma_core::scoring::clear_label;
use luma_core::{Kind, Piece};
use sdl2::pixels::Color;
use sdl2::rect::Rect;

pub const CELL: i32 = 46;
pub const BOARD_W: i32 = CELL * COLS;
pub const BOARD_H: i32 = CELL * (ROWS - HIDDEN_ROWS);
pub const BOARD_X: i32 = (W - BOARD_W) / 2;
pub const BOARD_Y: i32 = (H - BOARD_H) / 2;

pub fn colour(k: Kind) -> Color {
    match k {
        Kind::I => Color::RGB(64, 214, 222),
        Kind::O => Color::RGB(232, 198, 82),
        Kind::T => Color::RGB(168, 118, 238),
        Kind::S => Color::RGB(98, 210, 128),
        Kind::Z => Color::RGB(240, 112, 96),
        Kind::J => Color::RGB(82, 136, 240),
        Kind::L => Color::RGB(240, 152, 70),
    }
}

/// Visual options taken from settings.
#[derive(Debug, Clone, Copy)]
pub struct Look {
    pub glow: f32,
    pub ghost: bool,
    pub patterns: bool,
    pub reduced_motion: bool,
}

fn cell_rect(x: i32, y: i32) -> Rect {
    Rect::new(
        BOARD_X + x * CELL,
        BOARD_Y + (y - HIDDEN_ROWS) * CELL,
        CELL as u32,
        CELL as u32,
    )
}

struct Trail {
    cols: Vec<(i32, i32, i32)>,
    c: Color,
    t0: u64,
}
struct ClearFx {
    rows: Vec<i32>,
    t0: u64,
    big: bool,
}
struct Particle {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    c: Color,
    t0: u64,
    life: f32,
}
/// Cells of a just-locked piece, its colour and the lock time.
type Flash = (Vec<(i32, i32)>, Color, u64);

struct Label {
    text: String,
    t0: u64,
    c: Color,
}

#[derive(Default)]
pub struct Fx {
    trails: Vec<Trail>,
    clears: Vec<ClearFx>,
    particles: Vec<Particle>,
    labels: Vec<Label>,
    flashes: Vec<Flash>,
    rot_pulse: Option<u64>,
    rng: u64,
}

fn age_ms(now: u64, t0: u64) -> f32 {
    now.saturating_sub(t0) as f32 / 1000.0
}

impl Fx {
    fn rand(&mut self) -> f32 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.rng >> 33) as u32) as f32 / u32::MAX as f32
    }

    pub fn on_event(&mut self, e: &Event, now: u64, look: &Look) {
        match e {
            Event::Rotated { .. } => self.rot_pulse = Some(now),
            Event::HardDrop { from, to } => {
                if to.y > from.y && !look.reduced_motion {
                    let mut cols: Vec<(i32, i32, i32)> = Vec::new();
                    for (x, y) in from.cells() {
                        let top = y;
                        let bottom = y + (to.y - from.y);
                        match cols.iter_mut().find(|c| c.0 == x) {
                            Some(c) => {
                                c.1 = c.1.min(top);
                                c.2 = c.2.max(bottom);
                            }
                            None => cols.push((x, top, bottom)),
                        }
                    }
                    self.trails.push(Trail {
                        cols,
                        c: colour(from.kind),
                        t0: now,
                    });
                }
            }
            Event::Locked { cells, kind } => {
                self.flashes.push((cells.clone(), colour(*kind), now));
            }
            Event::Cleared { rows, score } => {
                let big = score.lines >= 4 || score.spin != luma_core::scoring::Spin::None;
                self.clears.push(ClearFx {
                    rows: rows.clone(),
                    t0: now,
                    big,
                });
                if !look.reduced_motion {
                    let per_row = if big { 12 } else { 7 };
                    for &r in rows {
                        if r < HIDDEN_ROWS {
                            continue;
                        }
                        let cy = (BOARD_Y + (r - HIDDEN_ROWS) * CELL + CELL / 2) as f32;
                        for _ in 0..per_row {
                            let x = BOARD_X as f32 + self.rand() * BOARD_W as f32;
                            let vx = (self.rand() - 0.5) * 220.0;
                            let vy = -60.0 - self.rand() * 180.0;
                            let hue = [Kind::I, Kind::T, Kind::Z, Kind::O][(self.rand() * 3.99) as usize];
                            let life = 380.0 + self.rand() * 260.0;
                            self.particles.push(Particle {
                                x,
                                y: cy,
                                vx,
                                vy,
                                c: colour(hue),
                                t0: now,
                                life,
                            });
                        }
                    }
                    self.particles.truncate(80);
                }
                if let Some(text) = clear_label(score) {
                    self.labels.push(Label {
                        text,
                        t0: now,
                        c: theme::CYAN,
                    });
                }
                if score.perfect {
                    self.labels.push(Label {
                        text: "Perfect clear".into(),
                        t0: now + 250_000,
                        c: theme::CORAL,
                    });
                }
            }
            Event::SpinNoLines { score } => {
                if let Some(text) = clear_label(score) {
                    self.labels.push(Label {
                        text,
                        t0: now,
                        c: theme::TEXT_DIM,
                    });
                }
            }
            Event::LevelUp(l) => self.labels.push(Label {
                text: format!("Level {l}"),
                t0: now,
                c: theme::TEXT,
            }),
            Event::PracticeReset => self.labels.push(Label {
                text: "Fresh board".into(),
                t0: now,
                c: theme::TEXT_DIM,
            }),
            _ => {}
        }
    }

    fn prune(&mut self, now: u64) {
        self.trails.retain(|t| age_ms(now, t.t0) < 240.0);
        self.clears.retain(|c| age_ms(now, c.t0) < 420.0);
        self.particles.retain(|p| age_ms(now, p.t0) < p.life);
        self.labels.retain(|l| now < l.t0 || age_ms(now, l.t0) < 1400.0);
        self.flashes.retain(|f| age_ms(now, f.2) < 160.0);
    }
}

pub fn draw_block(g: &mut Gfx, r: Rect, c: Color, bright: f32, look: &Look, kind: Option<Kind>) {
    let canvas = &mut g.canvas;
    Gfx::tinted(&mut g.tex.block, canvas, r, scale(c, bright), false);
    if look.patterns {
        if let Some(k) = kind {
            let t = &mut g.tex.patterns[k as usize];
            t.set_alpha_mod(255);
            let _ = canvas.copy(t, None, r);
        }
    }
}

fn glow_at(g: &mut Gfx, r: Rect, c: Color, size: f32, a: f32) {
    if a <= 0.004 {
        return;
    }
    let s = (r.width() as f32 * size) as i32;
    let dst = Rect::new(r.center().x() - s / 2, r.center().y() - s / 2, s as u32, s as u32);
    let canvas = &mut g.canvas;
    Gfx::tinted(&mut g.tex.glow, canvas, dst, alpha(c, a), true);
}

/// A small piece preview centred in `area` (hold / next queue).
pub fn draw_mini(g: &mut Gfx, kind: Kind, cx: i32, cy: i32, cell: i32, dim: bool, look: &Look) {
    let cells = kind.cells(0);
    let (mut minx, mut maxx, mut miny, mut maxy) = (9, -9, 9, -9);
    for (x, y) in cells {
        minx = minx.min(x);
        maxx = maxx.max(x);
        miny = miny.min(y);
        maxy = maxy.max(y);
    }
    let w = (maxx - minx + 1) * cell;
    let h = (maxy - miny + 1) * cell;
    let c = colour(kind);
    let bright = if dim { 0.35 } else { 0.85 * look.glow.min(1.1) };
    for (x, y) in cells {
        let r = Rect::new(
            cx - w / 2 + (x - minx) * cell,
            cy - h / 2 + (y - miny) * cell,
            cell as u32,
            cell as u32,
        );
        if !dim {
            glow_at(g, r, c, 2.6, 0.12 * look.glow);
        }
        draw_block(g, r, c, bright, look, Some(kind));
    }
}

pub fn draw_board_frame(g: &mut Gfx) {
    let r = Rect::new(
        BOARD_X - 6,
        BOARD_Y - 6,
        (BOARD_W + 12) as u32,
        (BOARD_H + 12) as u32,
    );
    g.round_outline(r, 14, theme::LINE, 1);
    // Faint corner dots instead of grid lines.
    g.canvas.set_draw_color(Color::RGBA(255, 255, 255, 16));
    for y in 1..(ROWS - HIDDEN_ROWS) {
        for x in 1..COLS {
            let px = BOARD_X + x * CELL;
            let py = BOARD_Y + y * CELL;
            let _ = g.canvas.fill_rect(Rect::new(px - 1, py - 1, 2, 2));
        }
    }
}

/// Everything inside and around the board for one frame.
pub struct BoardInput<'a> {
    pub game: &'a Game,
    pub look: Look,
    pub now: u64,
    /// Pointing target (leftmost column) and whether the ghost reaches it.
    pub target: Option<(i32, i32, bool)>,
    pub best: Option<String>,
}

pub fn draw_game(g: &mut Gfx, fx: &mut Fx, b: &BoardInput) {
    let game = b.game;
    let look = &b.look;
    let now = b.now;
    fx.prune(now);
    draw_board_frame(g);

    // Settled blocks: calmer than the active piece.
    for y in HIDDEN_ROWS..ROWS {
        for x in 0..COLS {
            if let Some(k) = Kind::from_cell(game.cell(x, y)) {
                let r = cell_rect(x, y);
                glow_at(g, r, colour(k), 2.2, 0.07 * look.glow);
            }
        }
    }
    for y in HIDDEN_ROWS..ROWS {
        for x in 0..COLS {
            if let Some(k) = Kind::from_cell(game.cell(x, y)) {
                draw_block(
                    g,
                    cell_rect(x, y),
                    colour(k),
                    0.70 * (0.85 + 0.15 * look.glow),
                    look,
                    Some(k),
                );
            }
        }
    }
    // Lock flash.
    for (cells, c, t0) in &fx.flashes {
        let a = 1.0 - age_ms(now, *t0) / 160.0;
        for &(x, y) in cells {
            if y >= HIDDEN_ROWS {
                let r = cell_rect(x, y);
                let canvas = &mut g.canvas;
                Gfx::tinted(&mut g.tex.block, canvas, r, alpha(*c, 0.45 * a), true);
            }
        }
    }
    // Line clear sweeps.
    for c in &fx.clears {
        let t = age_ms(now, c.t0) / 420.0;
        let reach = if look.reduced_motion {
            1.0
        } else {
            (t * 2.2).min(1.0)
        };
        let a = (1.0 - t).max(0.0) * if c.big { 0.9 } else { 0.6 };
        for &r in &c.rows {
            if r < HIDDEN_ROWS {
                continue;
            }
            let cy = BOARD_Y + (r - HIDDEN_ROWS) * CELL + CELL / 2;
            let half = (BOARD_W as f32 / 2.0 * reach) as i32 + CELL;
            let dst = Rect::new(
                BOARD_X + BOARD_W / 2 - half,
                cy - CELL,
                (half * 2) as u32,
                (CELL * 2) as u32,
            );
            let col = if c.big {
                theme::CYAN
            } else {
                Color::RGB(220, 230, 240)
            };
            let canvas = &mut g.canvas;
            Gfx::tinted(&mut g.tex.hband, canvas, dst, alpha(col, a), true);
        }
        if c.big && !look.reduced_motion {
            // Local highlight around the cleared rows (no full-screen flash).
            let ys: Vec<i32> = c.rows.iter().filter(|r| **r >= HIDDEN_ROWS).copied().collect();
            if let (Some(lo), Some(hi)) = (ys.iter().min(), ys.iter().max()) {
                let top = BOARD_Y + (lo - HIDDEN_ROWS) * CELL;
                let bot = BOARD_Y + (hi - HIDDEN_ROWS + 1) * CELL;
                let s = (bot - top + CELL * 6).max(CELL * 8);
                let dst = Rect::new(
                    BOARD_X + BOARD_W / 2 - s / 2,
                    (top + bot) / 2 - s / 2,
                    s as u32,
                    s as u32,
                );
                let canvas = &mut g.canvas;
                Gfx::tinted(
                    &mut g.tex.glow,
                    canvas,
                    dst,
                    alpha(theme::CYAN, 0.18 * (1.0 - t)),
                    true,
                );
            }
        }
    }
    for p in &fx.particles {
        let t = age_ms(now, p.t0) / 1000.0;
        let x = p.x + p.vx * t;
        let y = p.y + p.vy * t + 140.0 * t * t;
        let a = 1.0 - age_ms(now, p.t0) / p.life;
        let s = 10;
        let dst = Rect::new(x as i32 - s / 2, y as i32 - s / 2, s as u32, s as u32);
        let canvas = &mut g.canvas;
        Gfx::tinted(&mut g.tex.dot, canvas, dst, alpha(p.c, 0.8 * a), true);
    }

    if game.is_playing() {
        let p = game.active;
        let c = colour(p.kind);
        // Ghost: dim, outlined, low glow.
        if look.ghost {
            let gp = game.ghost();
            if gp.y != p.y {
                for (x, y) in gp.cells() {
                    if y >= HIDDEN_ROWS {
                        let r = cell_rect(x, y);
                        let canvas = &mut g.canvas;
                        Gfx::tinted(&mut g.tex.outline, canvas, r, alpha(c, 0.42), false);
                        Gfx::tinted(&mut g.tex.block, canvas, r, alpha(c, 0.06), false);
                    }
                }
            }
        }
        // Drop trails.
        for t in &fx.trails {
            let a = 1.0 - age_ms(now, t.t0) / 240.0;
            for &(x, top, bottom) in &t.cols {
                let top = top.max(HIDDEN_ROWS);
                if bottom < top {
                    continue;
                }
                let r0 = cell_rect(x, top);
                let r1 = cell_rect(x, bottom);
                let dst = Rect::new(
                    r0.x() + CELL / 6,
                    r0.y(),
                    (CELL * 2 / 3) as u32,
                    (r1.bottom() - r0.y()).max(1) as u32,
                );
                let canvas = &mut g.canvas;
                Gfx::tinted(&mut g.tex.vgrad, canvas, dst, alpha(t.c, 0.38 * a), true);
            }
        }
        // Active piece with halo; brief pulse on rotation.
        let pulse = fx
            .rot_pulse
            .map(|t0| (1.0 - age_ms(now, t0) / 130.0).max(0.0))
            .unwrap_or(0.0);
        let pulse = if look.reduced_motion { pulse * 0.4 } else { pulse };
        for (x, y) in p.cells() {
            if y >= HIDDEN_ROWS - 1 {
                let r = cell_rect(x, y);
                glow_at(g, r, c, 3.4, (0.30 + 0.20 * pulse) * look.glow);
            }
        }
        for (x, y) in p.cells() {
            if y >= HIDDEN_ROWS - 1 {
                let r = cell_rect(x, y);
                let above = y < HIDDEN_ROWS;
                let bright = (0.92 + 0.12 * pulse) * (0.9 + 0.1 * look.glow);
                draw_block(
                    g,
                    r,
                    if above { scale(c, 0.6) } else { c },
                    bright,
                    look,
                    Some(p.kind),
                );
            }
        }
        // Pointing target marker (never implies an impossible destination: it dims when the
        // ghost cannot reach it).
        if let Some((lo, w, reachable)) = b.target {
            let x0 = BOARD_X + lo * CELL;
            let col = if reachable {
                alpha(theme::CYAN, 0.55)
            } else {
                alpha(theme::TEXT_FAINT, 0.5)
            };
            g.round(Rect::new(x0 + 6, BOARD_Y - 16, (w * CELL - 12) as u32, 4), 2, col);
        }
    }

    draw_hud(g, b);

    // Labels for difficult clears, near the top of the board.
    let mut ly = BOARD_Y + BOARD_H / 3;
    let labels: Vec<(String, f32, Color)> = fx
        .labels
        .iter()
        .filter(|l| now >= l.t0)
        .map(|l| (l.text.clone(), age_ms(now, l.t0) / 1400.0, l.c))
        .collect();
    for (text, t, c) in labels {
        let a = if t < 0.15 {
            t / 0.15
        } else {
            1.0 - ((t - 0.15) / 0.85).powf(2.0)
        };
        let rise = if look.reduced_motion { 0 } else { (t * 30.0) as i32 };
        g.text_centered(
            &text,
            BOARD_X + BOARD_W / 2,
            ly - rise,
            40.0,
            Face::Semi,
            alpha(c, a.max(0.0)),
        );
        ly += 52;
    }
}

pub fn fmt_time(ms: u64) -> String {
    format!("{}:{:02}.{:02}", ms / 60_000, (ms / 1000) % 60, (ms % 1000) / 10)
}

pub fn fmt_score(s: u64) -> String {
    let d = s.to_string();
    let mut out = String::new();
    for (i, ch) in d.chars().enumerate() {
        if i > 0 && (d.len() - i).is_multiple_of(3) {
            out.push('\u{2009}');
        }
        out.push(ch);
    }
    out
}

fn draw_hud(g: &mut Gfx, b: &BoardInput) {
    let game = b.game;
    let look = &b.look;
    let lx = BOARD_X - 70; // right edge of the left column
    let rx = BOARD_X + BOARD_W + 70; // left edge of the right column
    let label = |g: &mut Gfx, s: &str, x: i32, y: i32, right: bool| {
        if right {
            g.text_right(s, x, y, 22.0, Face::Semi, theme::TEXT_FAINT);
        } else {
            g.text(s, x, y, 22.0, Face::Semi, theme::TEXT_FAINT);
        }
    };
    // Hold.
    label(g, S::HUD_HOLD, lx, BOARD_Y + 4, true);
    if let Some(k) = game.hold {
        draw_mini(g, k, lx - 70, BOARD_Y + 90, 30, game.hold_used, look);
    }
    // Stats.
    let mut y = BOARD_Y + 220;
    let mut stat = |g: &mut Gfx, name: &str, value: String, big: bool| {
        label(g, name, lx, y, true);
        let px = if big { 58.0 } else { 40.0 };
        g.text_right(&value, lx, y + 30, px, Face::Light, theme::TEXT);
        y += if big { 120 } else { 96 };
    };
    match game.mode {
        Mode::Sprint => {
            stat(g, S::HUD_TIME, fmt_time(game.elapsed_us / 1000), true);
            stat(
                g,
                S::HUD_LEFT,
                SPRINT_LINES.saturating_sub(game.lines).to_string(),
                false,
            );
            stat(g, "PIECES", game.pieces_placed.to_string(), false);
        }
        Mode::Practice => {
            stat(g, S::HUD_LINES, game.lines.to_string(), true);
        }
        _ => {
            stat(g, S::HUD_SCORE, fmt_score(game.score), true);
            stat(g, S::HUD_LEVEL, game.level.to_string(), false);
            stat(g, S::HUD_LINES, game.lines.to_string(), false);
        }
    }
    if let Some(best) = &b.best {
        label(g, S::HUD_BEST, lx, y, true);
        g.text_right(best, lx, y + 30, 30.0, Face::Light, theme::TEXT_DIM);
    }
    // Next queue.
    label(g, S::HUD_NEXT, rx, BOARD_Y + 4, false);
    let mut ny = BOARD_Y + 90;
    for (i, k) in game.queue.iter().enumerate() {
        let cell = if i == 0 { 30 } else { 22 };
        draw_mini(g, *k, rx + 70, ny, cell, false, look);
        ny += if i == 0 { 120 } else { 92 };
    }
    if game.mode == Mode::Zen {
        g.text(
            S::ZEN,
            rx,
            BOARD_Y + BOARD_H - 30,
            26.0,
            Face::Light,
            theme::TEXT_FAINT,
        );
    }
}

/// Small animated illustrations for the onboarding style choices.
pub fn draw_illustration(g: &mut Gfx, kind: u8, cx: i32, cy: i32, now: u64) {
    let t = now as f32 / 1e6;
    match kind {
        // Wrist twist: the remote seen from behind, rocking about its long axis.
        0 => {
            let ang = (t * 2.2).sin() * 40.0;
            let dst = Rect::new(cx - 22, cy - 60, 44, 120);
            g.tex.block.set_color_mod(150, 155, 170);
            g.tex.block.set_alpha_mod(255);
            g.tex.block.set_blend_mode(sdl2::render::BlendMode::Blend);
            let _ = g
                .canvas
                .copy_ex(&g.tex.block, None, dst, ang as f64, None, false, false);
            let dot = Rect::new(cx - 6, cy - 50, 12, 12);
            let _ = g.canvas.copy_ex(
                &g.tex.dot,
                None,
                dot,
                ang as f64,
                sdl2::rect::Point::new(6, 50),
                false,
                false,
            );
        }
        // Circle: a dot tracing a circle with a short fading tail.
        1 => {
            for i in 0..14 {
                let a = t * 5.0 - i as f32 * 0.16;
                let x = cx as f32 + 48.0 * a.cos();
                let y = cy as f32 + 48.0 * a.sin();
                let s = 18 - i;
                let dst = Rect::new(x as i32 - s / 2, y as i32 - s / 2, s as u32, s as u32);
                let canvas = &mut g.canvas;
                Gfx::tinted(
                    &mut g.tex.dot,
                    canvas,
                    dst,
                    alpha(theme::CYAN, 1.0 - i as f32 / 14.0),
                    true,
                );
            }
        }
        // Pointing: a pointer gliding over a strip of columns, the column under it lit.
        2 => {
            let u = ((t * 1.3).sin() * 0.5 + 0.5) * 9.99;
            for i in 0..10 {
                let r = Rect::new(cx - 100 + i * 20, cy + 10, 18, 18);
                let lit = i == u as i32;
                g.round(r, 4, if lit { theme::CYAN } else { Color::RGB(40, 44, 52) });
            }
            let px = cx - 100 + (u * 20.0) as i32;
            let dst = Rect::new(px - 8, cy - 30, 16, 16);
            let canvas = &mut g.canvas;
            Gfx::tinted(&mut g.tex.dot, canvas, dst, theme::TEXT, false);
        }
        // Flick: a quick left/right jab of an arrow.
        _ => {
            let phase = (t * 1.2).fract();
            let dir = if (t * 1.2) as i32 % 2 == 0 { 1.0 } else { -1.0 };
            let off = if phase < 0.25 {
                (phase / 0.25) * 60.0
            } else {
                60.0 * (1.0 - (phase - 0.25) / 0.75)
            };
            let x = cx + (dir * off) as i32;
            g.round(Rect::new(x - 30, cy - 5, 60, 10), 5, theme::CYAN);
            let tip = x + (dir * 34.0) as i32;
            for k in 0..3 {
                let s = 22 - k * 6;
                let dst = Rect::new(tip - s / 2, cy - s / 2, s as u32, s as u32);
                let canvas = &mut g.canvas;
                Gfx::tinted(&mut g.tex.dot, canvas, dst, theme::CYAN, true);
            }
        }
    }
}

/// Column span of a piece (for the pointing target width).
pub fn piece_width(p: &Piece) -> i32 {
    let (lo, hi) = p.column_span();
    hi - lo + 1
}
