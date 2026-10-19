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
    /// Cell values of each cleared row just before it was removed (for the dissolve).
    cells: Vec<Vec<u8>>,
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
    // Active piece animation (visual only; the simulation stays on the grid).
    vis: Option<(f32, f32)>,
    rot_anim: Option<(u64, f32)>,
    spawn_t0: u64,
    last_now: u64,
    // Board snapshot from the previous frame plus the cells of the piece that just locked,
    // so cleared rows can dissolve even though the engine removed them instantly.
    last_board: Vec<u8>,
    pending_lock: Vec<(i32, i32, u8)>,
    queue_t0: u64,
    hold_t0: u64,
    level_t0: u64,
    over_t0: Option<u64>,
    impacts: Vec<(f32, f32, Color, u64)>,
    score_shown: f64,
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
            Event::Rotated { cw, .. } => {
                self.rot_pulse = Some(now);
                if !look.reduced_motion {
                    self.rot_anim = Some((now, if *cw { -90.0 } else { 90.0 }));
                }
            }
            Event::Spawned => {
                self.vis = None;
                self.rot_anim = None;
                self.spawn_t0 = now;
                self.queue_t0 = now;
            }
            Event::Held => {
                self.vis = None;
                self.hold_t0 = now;
            }
            Event::ToppedOut => self.over_t0 = Some(now),
            Event::HardDrop { from, to } => {
                self.vis = None;
                self.rot_anim = None;
                if !look.reduced_motion {
                    // A little dust where the piece lands.
                    let mut bottoms: Vec<(i32, i32)> = Vec::new();
                    for (x, y) in to.cells() {
                        match bottoms.iter_mut().find(|b| b.0 == x) {
                            Some(b) => b.1 = b.1.max(y),
                            None => bottoms.push((x, y)),
                        }
                    }
                    for (x, y) in bottoms {
                        if y >= HIDDEN_ROWS {
                            let r = cell_rect(x, y);
                            self.impacts.push((
                                r.center().x() as f32,
                                r.bottom() as f32,
                                colour(to.kind),
                                now,
                            ));
                        }
                    }
                }
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
                self.pending_lock = cells.iter().map(|&(x, y)| (x, y, kind.cell())).collect();
            }
            Event::Cleared { rows, score } => {
                let big = score.lines >= 4 || score.spin != luma_core::scoring::Spin::None;
                let cells = rows
                    .iter()
                    .map(|&r| {
                        let mut row: Vec<u8> = (0..COLS)
                            .map(|x| self.last_board.get((r * COLS + x) as usize).copied().unwrap_or(0))
                            .collect();
                        for &(x, y, c) in &self.pending_lock {
                            if y == r && (0..COLS).contains(&x) {
                                row[x as usize] = c;
                            }
                        }
                        row
                    })
                    .collect();
                self.clears.push(ClearFx {
                    rows: rows.clone(),
                    cells,
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
            Event::LevelUp(l) => {
                self.level_t0 = now;
                self.labels.push(Label {
                    text: format!("Level {l}"),
                    t0: now,
                    c: theme::TEXT,
                })
            }
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
        self.impacts.retain(|i| age_ms(now, i.3) < 320.0);
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
    /// Hide hold, stats and queue (setup's try-it board).
    pub hide_hud: bool,
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Rows each settled cell has to fall, in the latest clear, as a function of its row now.
fn fall_rows(rows: &[i32], y: i32) -> i32 {
    // A cell now at row y came from row o with y = o + (cleared rows below o).
    for k in 0..=rows.len() as i32 {
        let o = y - k;
        if !rows.contains(&o) && rows.iter().filter(|r| **r > o).count() as i32 == k {
            return k;
        }
    }
    0
}

const CLEAR_DISSOLVE_MS: f32 = 170.0;
const CLEAR_FALL_DELAY_MS: f32 = 150.0;
const CLEAR_FALL_MS: f32 = 170.0;

pub fn draw_game(g: &mut Gfx, fx: &mut Fx, b: &BoardInput) {
    let game = b.game;
    let look = &b.look;
    let now = b.now;
    let dt_ms = if fx.last_now == 0 {
        16.0
    } else {
        (now.saturating_sub(fx.last_now) as f32 / 1000.0).min(50.0)
    };
    fx.last_now = now;
    fx.prune(now);
    let rm = look.reduced_motion;

    // Board frame, with a soft pulse on level-up.
    draw_board_frame(g);
    let lv = age_ms(now, fx.level_t0);
    if fx.level_t0 > 0 && lv < 700.0 && !rm {
        let r = Rect::new(
            BOARD_X - 6,
            BOARD_Y - 6,
            (BOARD_W + 12) as u32,
            (BOARD_H + 12) as u32,
        );
        g.round_outline(r, 14, alpha(theme::CYAN, 0.5 * (1.0 - lv / 700.0)), 2);
    }

    // Settled blocks fall into place after a clear; game over greys them from the top.
    let latest = fx.clears.last().map(|c| (c.rows.clone(), age_ms(now, c.t0)));
    let fall = |y: i32| -> f32 {
        match &latest {
            Some((rows, age)) if !rm => {
                let k = fall_rows(rows, y) as f32;
                let t = ((age - CLEAR_FALL_DELAY_MS) / CLEAR_FALL_MS).clamp(0.0, 1.0);
                k * (1.0 - ease(t))
            }
            _ => 0.0,
        }
    };
    let grey = |y: i32| -> f32 {
        match fx.over_t0 {
            Some(t0) => ((age_ms(now, t0) - (y - HIDDEN_ROWS) as f32 * 40.0) / 220.0).clamp(0.0, 1.0),
            None => 0.0,
        }
    };
    let settled_colour = |k: Kind, y: i32| -> Color {
        let c = colour(k);
        let gr = grey(y);
        if gr <= 0.0 {
            return c;
        }
        let l = (c.r as f32 * 0.3 + c.g as f32 * 0.59 + c.b as f32 * 0.11) * 0.55;
        let mix = |v: u8| (v as f32 * (1.0 - gr) + l * gr) as u8;
        Color::RGB(mix(c.r), mix(c.g), mix(c.b))
    };
    let shifted = |x: i32, y: i32, f: f32| -> Rect {
        let r = cell_rect(x, y);
        Rect::new(r.x(), r.y() - (f * CELL as f32) as i32, r.width(), r.height())
    };
    for y in HIDDEN_ROWS..ROWS {
        let f = fall(y);
        for x in 0..COLS {
            if let Some(k) = Kind::from_cell(game.cell(x, y)) {
                glow_at(
                    g,
                    shifted(x, y, f),
                    settled_colour(k, y),
                    2.2,
                    0.07 * look.glow * (1.0 - grey(y) * 0.8),
                );
            }
        }
    }
    for y in HIDDEN_ROWS..ROWS {
        let f = fall(y);
        for x in 0..COLS {
            if let Some(k) = Kind::from_cell(game.cell(x, y)) {
                draw_block(
                    g,
                    shifted(x, y, f),
                    settled_colour(k, y),
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
    // Cleared rows: a bright flash, then each block shrinks away from the centre outward.
    for c in &fx.clears {
        let age = age_ms(now, c.t0);
        for (ri, &r) in c.rows.iter().enumerate() {
            if r < HIDDEN_ROWS {
                continue;
            }
            let Some(row) = c.cells.get(ri) else { continue };
            for x in 0..COLS {
                let Some(k) = Kind::from_cell(row[x as usize]) else {
                    continue;
                };
                // Flash at full size first, then dissolve from the centre outward.
                let delay = if rm {
                    0.0
                } else {
                    70.0 + (x as f32 - 4.5).abs() * 12.0
                };
                let t = ((age - delay) / CLEAR_DISSOLVE_MS).clamp(0.0, 1.0);
                if t >= 1.0 {
                    continue;
                }
                let s = if rm { 1.0 } else { 1.0 - ease(t) };
                let base = cell_rect(x, r);
                let w = (CELL as f32 * s) as i32;
                if w < 2 {
                    continue;
                }
                let dst = Rect::new(
                    base.center().x() - w / 2,
                    base.center().y() - w / 2,
                    w as u32,
                    w as u32,
                );
                let a = if rm { 1.0 - t } else { 1.0 };
                draw_block(g, dst, alpha(colour(k), a), 1.0, look, None);
                let flash = (1.0 - age / 120.0).max(0.0);
                let canvas = &mut g.canvas;
                Gfx::tinted(
                    &mut g.tex.block,
                    canvas,
                    dst,
                    alpha(Color::RGB(255, 255, 255), 0.65 * flash),
                    true,
                );
            }
        }
        // A soft band sweeping out from the centre.
        let t = age / 420.0;
        let reach = if rm { 1.0 } else { (t * 2.2).min(1.0) };
        let a = (1.0 - t).max(0.0) * if c.big { 0.8 } else { 0.45 };
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
        if c.big && !rm {
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
                    alpha(theme::CYAN, 0.18 * (1.0 - t).max(0.0)),
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
    // Landing dust.
    for &(x, y, c, t0) in &fx.impacts {
        let t = age_ms(now, t0) / 320.0;
        for side in [-1.0f32, 1.0] {
            let px = x + side * (6.0 + 26.0 * ease(t));
            let py = y - 4.0 - 10.0 * ease(t);
            let s = (8.0 * (1.0 - t)) as i32 + 2;
            let canvas = &mut g.canvas;
            Gfx::tinted(
                &mut g.tex.dot,
                canvas,
                Rect::new(px as i32 - s / 2, py as i32 - s / 2, s as u32, s as u32),
                alpha(c, 0.7 * (1.0 - t)),
                true,
            );
        }
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
        // Visual position glides toward the grid position (fast, never far behind).
        let target = (p.x as f32, p.y as f32);
        let (vx, vy) = match fx.vis {
            Some((vx, vy)) if !rm && (vx - target.0).abs() < 4.0 && (vy - target.1).abs() < 4.0 => {
                let kx = 1.0 - (-dt_ms / 28.0).exp();
                let ky = 1.0 - (-dt_ms / 20.0).exp();
                (vx + (target.0 - vx) * kx, vy + (target.1 - vy) * ky)
            }
            _ => target,
        };
        fx.vis = Some((vx, vy));
        let ang = match fx.rot_anim {
            Some((t0, a0)) => {
                let t = age_ms(now, t0) / 110.0;
                if t >= 1.0 {
                    fx.rot_anim = None;
                    0.0
                } else {
                    a0 * (1.0 - ease(t))
                }
            }
            None => 0.0,
        };
        let spawn = if rm {
            1.0
        } else {
            ease(age_ms(now, fx.spawn_t0) / 110.0)
        };
        let n = p.kind.box_size() as f32;
        let (sa, ca) = ang.to_radians().sin_cos();
        let mut rects = Vec::with_capacity(4);
        for (cx, cy) in p.kind.cells(p.rot) {
            if p.y + cy < HIDDEN_ROWS - 1 {
                continue;
            }
            let rx = cx as f32 + 0.5 - n / 2.0;
            let ry = cy as f32 + 0.5 - n / 2.0;
            let (qx, qy) = (rx * ca - ry * sa, rx * sa + ry * ca);
            let px = BOARD_X as f32 + (vx + n / 2.0 + qx) * CELL as f32 - CELL as f32 / 2.0;
            let py = BOARD_Y as f32
                + (vy + n / 2.0 + qy - HIDDEN_ROWS as f32 - (1.0 - spawn) * 0.5) * CELL as f32
                - CELL as f32 / 2.0;
            rects.push((
                Rect::new(px.round() as i32, py.round() as i32, CELL as u32, CELL as u32),
                p.y + cy < HIDDEN_ROWS,
            ));
        }
        let pulse = fx
            .rot_pulse
            .map(|t0| (1.0 - age_ms(now, t0) / 130.0).max(0.0))
            .unwrap_or(0.0);
        let pulse = if rm { pulse * 0.4 } else { pulse };
        for (r, _) in &rects {
            glow_at(g, *r, c, 3.4, (0.30 + 0.20 * pulse) * look.glow * spawn);
        }
        for (r, above) in &rects {
            let bright = (0.92 + 0.12 * pulse) * (0.9 + 0.1 * look.glow);
            let col = if *above { scale(c, 0.6) } else { c };
            draw_block(g, *r, alpha(col, spawn), bright, look, Some(p.kind));
        }
        // Resting after a drop: a shrinking line under the piece shows the time left to
        // slide it.
        if game.is_sliding() && game.rules.drop_slide_us > 0 && !rects.is_empty() {
            let frac = game.slide_left_us as f32 / game.rules.drop_slide_us as f32;
            let x0 = rects.iter().map(|(r, _)| r.x()).min().unwrap_or(0);
            let x1 = rects.iter().map(|(r, _)| r.right()).max().unwrap_or(0);
            let y = rects.iter().map(|(r, _)| r.bottom()).max().unwrap_or(0);
            let w = ((x1 - x0) as f32 * frac).round() as i32;
            if w > 2 {
                let cx = (x0 + x1) / 2;
                g.round(Rect::new(cx - w / 2, y + 3, w as u32, 4), 2, alpha(c, 0.85));
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

    if !b.hide_hud {
        draw_hud(g, fx, b, dt_ms);
    }

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
        let rise = if rm { 0 } else { (t * 30.0) as i32 };
        // A small pop on entry.
        let px = if rm {
            40.0
        } else {
            40.0 * (0.85 + 0.15 * ease(t / 0.12))
        };
        g.text_centered(
            &text,
            BOARD_X + BOARD_W / 2,
            ly - rise,
            px,
            Face::Semi,
            alpha(c, a.max(0.0)),
        );
        ly += 52;
    }
    fx.last_board.clone_from(&game.board);
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

fn draw_hud(g: &mut Gfx, fx: &mut Fx, b: &BoardInput, dt_ms: f32) {
    let game = b.game;
    let look = &b.look;
    let now = b.now;
    let rm = look.reduced_motion;
    let lx = BOARD_X - 70; // right edge of the left column
    let rx = BOARD_X + BOARD_W + 70; // left edge of the right column
    let label = |g: &mut Gfx, s: &str, x: i32, y: i32, right: bool| {
        if right {
            g.text_right(s, x, y, 22.0, Face::Semi, theme::TEXT_FAINT);
        } else {
            g.text(s, x, y, 22.0, Face::Semi, theme::TEXT_FAINT);
        }
    };
    // Hold, with a small pop when it changes.
    label(g, S::HUD_HOLD, lx, BOARD_Y + 4, true);
    if let Some(k) = game.hold {
        let pop = if rm || fx.hold_t0 == 0 {
            1.0
        } else {
            0.6 + 0.4 * ease(age_ms(now, fx.hold_t0) / 160.0)
        };
        draw_mini(
            g,
            k,
            lx - 70,
            BOARD_Y + 90,
            (30.0 * pop) as i32,
            game.hold_used,
            look,
        );
    }
    // The score counts up instead of jumping.
    let actual = game.score as f64;
    if rm || fx.score_shown > actual || (actual - fx.score_shown) < 1.0 {
        fx.score_shown = actual;
    } else {
        let k = 1.0 - (-(dt_ms as f64) / 140.0).exp();
        fx.score_shown += (actual - fx.score_shown) * k + 0.5;
        fx.score_shown = fx.score_shown.min(actual);
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
            stat(g, S::HUD_SCORE, fmt_score(fx.score_shown.round() as u64), true);
            stat(g, S::HUD_LEVEL, game.level.to_string(), false);
            stat(g, S::HUD_LINES, game.lines.to_string(), false);
        }
    }
    if let Some(best) = &b.best {
        label(g, S::HUD_BEST, lx, y, true);
        g.text_right(best, lx, y + 30, 30.0, Face::Light, theme::TEXT_DIM);
    }
    // Next queue scrolls up by one slot when a piece spawns.
    label(g, S::HUD_NEXT, rx, BOARD_Y + 4, false);
    let shift = if rm || fx.queue_t0 == 0 {
        0.0
    } else {
        1.0 - ease(age_ms(now, fx.queue_t0) / 180.0)
    };
    let mut ny = BOARD_Y + 90;
    for (i, k) in game.queue.iter().enumerate() {
        let cell = if i == 0 { 30 } else { 22 };
        let slot = if i == 0 { 120.0 } else { 92.0 };
        let off = (shift * slot) as i32;
        let last = i == game.queue.len() - 1;
        if !(last && shift > 0.5) {
            draw_mini(g, *k, rx + 70, ny + off, cell, false, look);
        }
        ny += slot as i32;
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

/// Column span of a piece (for the pointing target width).
pub fn piece_width(p: &Piece) -> i32 {
    let (lo, hi) = p.column_span();
    hi - lo + 1
}
