//! Small animated illustrations of each gesture, used by onboarding, calibration and the
//! controls help. Each one is a looping, self-explaining mini scene: a stylized remote, the
//! motion, and what it does to a piece. Everything is procedural (no image assets).

use crate::gfx::{alpha, scale, theme, Face, Gfx};
use sdl2::pixels::Color;
use sdl2::rect::{Point, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Illus {
    Twist,
    Circle,
    Swing,
    Point,
    Flick,
    Drop,
    Still,
}

const REMOTE: Color = Color::RGB(150, 156, 172);

fn ease(p: f32) -> f32 {
    let p = p.clamp(0.0, 1.0);
    0.5 - 0.5 * (p * std::f32::consts::PI).cos()
}

/// Piecewise keyframes over a looping period: returns the eased value of the segment
/// containing `t` (seconds). `keys` are (duration, target value) from a start of 0.
fn keyframes(t: f32, keys: &[(f32, f32)]) -> (f32, usize) {
    let total: f32 = keys.iter().map(|k| k.0).sum();
    let mut u = t.rem_euclid(total);
    let mut from = keys.last().map(|k| k.1).unwrap_or(0.0);
    for (i, (d, to)) in keys.iter().enumerate() {
        if u <= *d {
            return (from + (to - from) * ease(u / d), i);
        }
        u -= d;
        from = *to;
    }
    (from, keys.len() - 1)
}

/// The remote seen from above, pivoting about its bottom end (yaw/pitch views).
fn remote_top(g: &mut Gfx, cx: i32, cy: i32, s: f32, angle: f32) {
    let w = (34.0 * s) as u32;
    let h = (120.0 * s) as u32;
    let dst = Rect::new(cx - w as i32 / 2, cy - h as i32, w, h);
    let pivot = Point::new(w as i32 / 2, h as i32);
    g.tex.block.set_color_mod(REMOTE.r, REMOTE.g, REMOTE.b);
    g.tex.block.set_alpha_mod(255);
    g.tex.block.set_blend_mode(sdl2::render::BlendMode::Blend);
    let _ = g
        .canvas
        .copy_ex(&g.tex.block, None, dst, angle as f64, pivot, false, false);
    // Emitter dot at the tip.
    let r = 110.0 * s;
    let a = angle.to_radians();
    let tip = (cx as f32 + r * a.sin(), cy as f32 - r * a.cos());
    let d = (14.0 * s) as i32;
    let canvas = &mut g.canvas;
    Gfx::tinted(
        &mut g.tex.dot,
        canvas,
        Rect::new(tip.0 as i32 - d / 2, tip.1 as i32 - d / 2, d as u32, d as u32),
        theme::CYAN,
        true,
    );
}

/// The remote's end face as the player sees it from behind (for the twist).
fn remote_end(g: &mut Gfx, cx: i32, cy: i32, s: f32, angle: f32) {
    let w = (120.0 * s) as u32;
    let h = (40.0 * s) as u32;
    let dst = Rect::new(cx - w as i32 / 2, cy - h as i32 / 2, w, h);
    g.tex.block.set_color_mod(REMOTE.r, REMOTE.g, REMOTE.b);
    g.tex.block.set_alpha_mod(255);
    g.tex.block.set_blend_mode(sdl2::render::BlendMode::Blend);
    let _ = g
        .canvas
        .copy_ex(&g.tex.block, None, dst, angle as f64, None, false, false);
    // A marker on the top face shows the rotation clearly.
    let a = (angle - 90.0).to_radians();
    let r = 13.0 * s;
    let d = (12.0 * s) as i32;
    let p = (cx as f32 + r * a.cos(), cy as f32 + r * a.sin());
    let canvas = &mut g.canvas;
    Gfx::tinted(
        &mut g.tex.dot,
        canvas,
        Rect::new(p.0 as i32 - d / 2, p.1 as i32 - d / 2, d as u32, d as u32),
        theme::CYAN,
        true,
    );
}

/// Dotted arc (degrees, 0 = up, clockwise positive) with an arrow head at the end.
pub fn arc(g: &mut Gfx, cx: i32, cy: i32, r: f32, from: f32, to: f32, c: Color) {
    let n = ((to - from).abs() / 6.0).max(2.0) as i32;
    for i in 0..=n {
        let a = (from + (to - from) * i as f32 / n as f32 - 90.0).to_radians();
        let x = cx as f32 + r * a.cos();
        let y = cy as f32 + r * a.sin();
        let d = if i == n { 14 } else { 7 };
        let canvas = &mut g.canvas;
        Gfx::tinted(
            &mut g.tex.dot,
            canvas,
            Rect::new(x as i32 - d / 2, y as i32 - d / 2, d as u32, d as u32),
            alpha(c, 0.4 + 0.6 * i as f32 / n as f32),
            true,
        );
    }
}

/// A ten-column strip with one lit piece at continuous column `col`.
fn strip(g: &mut Gfx, cx: i32, y: i32, s: f32, col: f32, ghost: Option<f32>) {
    let cw = (22.0 * s) as i32;
    let x0 = cx - cw * 5;
    for i in 0..10 {
        g.round(
            Rect::new(x0 + i * cw + 2, y, (cw - 4) as u32, (cw - 4) as u32),
            (4.0 * s) as i32,
            Color::RGB(28, 31, 38),
        );
    }
    if let Some(gc) = ghost {
        let r = Rect::new(
            x0 + (gc * cw as f32) as i32 + 2,
            y,
            (cw - 4) as u32,
            (cw - 4) as u32,
        );
        g.round_outline(r, (4.0 * s) as i32, alpha(theme::CYAN, 0.35), 1);
    }
    let r = Rect::new(
        x0 + (col * cw as f32) as i32 + 2,
        y,
        (cw - 4) as u32,
        (cw - 4) as u32,
    );
    let canvas = &mut g.canvas;
    let gs = cw * 3;
    Gfx::tinted(
        &mut g.tex.glow,
        canvas,
        Rect::new(
            r.center().x() - gs / 2,
            r.center().y() - gs / 2,
            gs as u32,
            gs as u32,
        ),
        alpha(theme::CYAN, 0.25),
        true,
    );
    Gfx::tinted(&mut g.tex.block, canvas, r, scale(theme::CYAN, 0.9), false);
}

fn caption(g: &mut Gfx, s: &str, cx: i32, y: i32, sc: f32, a: f32) {
    if a > 0.02 {
        g.text_centered(
            s,
            cx,
            y,
            20.0 * sc.max(0.8),
            Face::Regular,
            alpha(theme::TEXT_DIM, a),
        );
    }
}

/// Draw `which` centred at (cx, cy); `s` scales it (1.0 fits a ~300 px card).
pub fn draw(g: &mut Gfx, which: Illus, cx: i32, cy: i32, s: f32, now: u64) {
    let t = now as f32 / 1e6;
    match which {
        Illus::Twist => {
            // Turn like a key, hold, turn back slowly; the piece above rotates once.
            let (a, seg) = keyframes(t, &[(0.35, 45.0), (0.6, 45.0), (0.7, 0.0), (0.6, 0.0)]);
            remote_end(g, cx, cy + (20.0 * s) as i32, s, a);
            arc(g, cx, cy + (20.0 * s) as i32, 78.0 * s, -10.0, 55.0, theme::CYAN);
            let rot = if seg >= 1 { 90.0 } else { 90.0 * ease(a / 45.0) };
            mini_t(g, cx, cy - (95.0 * s) as i32, s, rot);
        }
        Illus::Circle => {
            for i in 0..16 {
                let a = t * 5.0 - i as f32 * 0.15;
                let x = cx as f32 + 55.0 * s * a.cos();
                let y = cy as f32 + 55.0 * s * a.sin();
                let d = ((18 - i) as f32 * s) as i32;
                let canvas = &mut g.canvas;
                Gfx::tinted(
                    &mut g.tex.dot,
                    canvas,
                    Rect::new(
                        x as i32 - d / 2,
                        y as i32 - d / 2,
                        d.max(2) as u32,
                        d.max(2) as u32,
                    ),
                    alpha(theme::CYAN, 1.0 - i as f32 / 16.0),
                    true,
                );
            }
            let turns = (t * 5.0 / std::f32::consts::TAU) as i32;
            mini_t(g, cx, cy, s * 0.8, (turns % 4) as f32 * 90.0);
        }
        Illus::Swing => {
            // Quick swing right (piece moves 3), pause, slow pull back (piece stays).
            let (a, seg) = keyframes(t, &[(0.25, 24.0), (0.45, 24.0), (0.9, 0.0), (0.7, 0.0)]);
            remote_top(g, cx, cy + (70.0 * s) as i32, s, a);
            let total = 0.25 + 0.45 + 0.9 + 0.7;
            let u = t.rem_euclid(total);
            let col = if seg == 0 { 3.0 + 3.0 * ease(u / 0.25) } else { 6.0 };
            strip(
                g,
                cx,
                cy - (100.0 * s) as i32,
                s,
                col.round().clamp(3.0, 6.0),
                None,
            );
            caption(
                g,
                "quick swing: moves",
                cx,
                cy + (95.0 * s) as i32,
                s,
                if seg <= 1 { 1.0 } else { 0.0 },
            );
            caption(
                g,
                "pull back: ignored",
                cx,
                cy + (95.0 * s) as i32,
                s,
                if seg == 2 { 1.0 } else { 0.0 },
            );
        }
        Illus::Point => {
            let a = (t * 1.1).sin() * 22.0;
            remote_top(g, cx, cy + (70.0 * s) as i32, s, a);
            let col = 4.5 + a / 22.0 * 4.5;
            strip(
                g,
                cx,
                cy - (100.0 * s) as i32,
                s,
                col.round().clamp(0.0, 9.0),
                Some(col.round().clamp(0.0, 9.0)),
            );
        }
        Illus::Flick => {
            let (a, seg) = keyframes(t, &[(0.12, 14.0), (0.5, 0.0), (0.9, 0.0)]);
            remote_top(g, cx, cy + (70.0 * s) as i32, s, a);
            let cycle = (t / 1.52) as i32;
            let col = 2 + (cycle % 5) + if seg == 0 && a > 7.0 { 1 } else { 0 };
            strip(g, cx, cy - (100.0 * s) as i32, s, col.min(9) as f32, None);
        }
        Illus::Drop => {
            // Side view: the tip dips quickly, the piece falls to the floor.
            let (a, seg) = keyframes(t, &[(0.15, 28.0), (0.3, 0.0), (1.0, 0.0)]);
            remote_top(g, cx - (70.0 * s) as i32, cy + (80.0 * s) as i32, s, 90.0 + a);
            let cw = (22.0 * s) as i32;
            let x = cx + (50.0 * s) as i32;
            let top = cy - (110.0 * s) as i32;
            for i in 0..8 {
                g.round(
                    Rect::new(x, top + i * cw, (cw - 4) as u32, (cw - 4) as u32),
                    4,
                    Color::RGB(28, 31, 38),
                );
            }
            let fall = if seg == 0 {
                ease(a / 28.0)
            } else if seg == 1 {
                1.0
            } else {
                0.0
            };
            let py = top + (fall * 7.0 * cw as f32) as i32;
            if fall > 0.0 && fall < 1.0 {
                let canvas = &mut g.canvas;
                Gfx::tinted(
                    &mut g.tex.vgrad,
                    canvas,
                    Rect::new(x, top, (cw - 4) as u32, (py - top).max(1) as u32),
                    alpha(theme::CORAL, 0.5),
                    true,
                );
            }
            let canvas = &mut g.canvas;
            Gfx::tinted(
                &mut g.tex.block,
                canvas,
                Rect::new(x, py, (cw - 4) as u32, (cw - 4) as u32),
                theme::CORAL,
                false,
            );
        }
        Illus::Still => {
            let wob = (t * 7.0).sin() * 0.6;
            remote_top(g, cx, cy + (70.0 * s) as i32, s, wob);
        }
    }
}

/// A small T piece rotated by `deg`, centred at (cx, cy).
pub fn mini_t(g: &mut Gfx, cx: i32, cy: i32, s: f32, deg: f32) {
    let c = (22.0 * s) as i32;
    let a = deg.to_radians();
    for (x, y) in [(-1.0f32, 0.5f32), (0.0, 0.5), (1.0, 0.5), (0.0, -0.5)] {
        let rx = x * a.cos() - y * a.sin();
        let ry = x * a.sin() + y * a.cos();
        let px = cx + (rx * c as f32) as i32 - c / 2;
        let py = cy + (ry * c as f32) as i32 - c / 2;
        let canvas = &mut g.canvas;
        Gfx::tinted(
            &mut g.tex.block,
            canvas,
            Rect::new(px + 1, py + 1, (c - 2) as u32, (c - 2) as u32),
            Color::RGB(168, 118, 238),
            false,
        );
    }
}

/// What the in-game remote indicator shows.
pub struct RemoteLive {
    /// Recent movement of the aim, screen heights (drifts back to the centre).
    pub aim: (f32, f32),
    /// Wrist twist, degrees (clockwise positive).
    pub twist_deg: f32,
    /// How close the current rotation gesture is to counting, -1..1 (sign = direction).
    pub progress: f32,
    /// Motion data is arriving.
    pub live: bool,
    /// The last recognized gesture and how far its flash has faded (0 = fresh, 1 = gone).
    pub flash: Option<(luma_input::Gesture, f32)>,
}

/// Small live remote for the play screen: the remote follows the movement and turns with
/// the wrist, an arc fills toward a rotation, and recognized gestures flash.
pub fn remote_live(g: &mut Gfx, cx: i32, cy: i32, r: &RemoteLive, now: u64) {
    use luma_input::Gesture;
    let dim = if r.live { 1.0 } else { 0.35 };
    let ring = 78;
    g.round_outline(
        Rect::new(cx - ring, cy - ring, (ring * 2) as u32, (ring * 2) as u32),
        ring,
        alpha(Color::RGB(255, 255, 255), 0.12 * dim),
        2,
    );
    // Progress toward a rotation.
    if r.live && r.progress.abs() > 0.08 {
        let to = r.progress.clamp(-1.0, 1.0) * 120.0;
        let c = if r.progress.abs() >= 1.0 {
            theme::OK
        } else {
            theme::CYAN
        };
        arc(g, cx, cy, ring as f32 + 10.0, 0.0, to, alpha(c, 0.8));
    }
    // The remote: offset by recent movement, rotated by the twist.
    let k = 260.0;
    let px = cx + (r.aim.0 * k).clamp(-48.0, 48.0) as i32;
    let py = cy + (r.aim.1 * k).clamp(-48.0, 48.0) as i32;
    let (w, h) = (22u32, 70u32);
    let dst = Rect::new(px - w as i32 / 2, py - h as i32 / 2, w, h);
    let body = scale(REMOTE, if r.live { 1.0 } else { 0.5 });
    g.tex.block.set_color_mod(body.r, body.g, body.b);
    g.tex.block.set_alpha_mod((230.0 * dim) as u8);
    g.tex.block.set_blend_mode(sdl2::render::BlendMode::Blend);
    let _ = g
        .canvas
        .copy_ex(&g.tex.block, None, dst, r.twist_deg as f64, None, false, false);
    // Emitter at the tip, turning with the body.
    let a = (r.twist_deg - 90.0).to_radians();
    let tip = (px as f32 + 27.0 * a.cos(), py as f32 + 27.0 * a.sin());
    let canvas = &mut g.canvas;
    Gfx::tinted(
        &mut g.tex.dot,
        canvas,
        Rect::new(tip.0 as i32 - 5, tip.1 as i32 - 5, 10, 10),
        alpha(theme::CYAN, dim),
        true,
    );
    // Recognized gesture.
    let Some((gesture, t)) = r.flash else { return };
    if t >= 1.0 {
        return;
    }
    let a = 1.0 - t;
    let c = alpha(theme::OK, a);
    match gesture {
        Gesture::RotateCw => arc(g, cx, cy, ring as f32 - 14.0, -60.0, 60.0, c),
        Gesture::RotateCcw => arc(g, cx, cy, ring as f32 - 14.0, 60.0, -60.0, c),
        Gesture::MoveLeft | Gesture::MoveRight | Gesture::HardDrop => {
            let (dx, dy) = match gesture {
                Gesture::MoveLeft => (-1.0, 0.0),
                Gesture::MoveRight => (1.0, 0.0),
                _ => (0.0, 1.0),
            };
            // Three dots sliding outward in the gesture's direction.
            let travel = 30.0 * t;
            for i in 0..3 {
                let d = 34.0 + i as f32 * 16.0 + travel;
                let x = cx as f32 + dx * d;
                let y = cy as f32 + dy * d;
                let s = 8 + i * 3;
                let canvas = &mut g.canvas;
                Gfx::tinted(
                    &mut g.tex.dot,
                    canvas,
                    Rect::new(x as i32 - s / 2, y as i32 - s / 2, s as u32, s as u32),
                    alpha(theme::OK, a * (0.5 + 0.25 * i as f32)),
                    true,
                );
            }
        }
    }
    let _ = now;
}
