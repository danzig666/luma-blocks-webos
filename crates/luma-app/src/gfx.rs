//! Rendering helpers over the SDL2 renderer (OpenGL ES 2 backend on webOS): theme colours,
//! procedurally generated textures (blocks, glows, outlines, particles, gradients), rounded
//! shapes and a glyph-cached text renderer (fontdue, Inter). All coordinates are in a
//! 1920x1080 logical space; SDL's logical size maps them to the real drawable. Blank pixels
//! are always true black.

use fontdue::{Font, FontSettings};
use sdl2::{
    pixels::{Color, PixelFormatEnum},
    rect::Rect,
    render::{BlendMode, Canvas, Texture, TextureCreator},
    video::{Window, WindowContext},
};
use std::collections::HashMap;

pub const W: i32 = 1920;
pub const H: i32 = 1080;

pub mod theme {
    use sdl2::pixels::Color;
    pub const BG: Color = Color::RGB(0, 0, 0);
    /// Brand colours from the launcher icon.
    pub const CYAN: Color = Color::RGB(14, 249, 243);
    pub const CORAL: Color = Color::RGB(253, 115, 91);
    pub const TEXT: Color = Color::RGB(226, 229, 236);
    pub const TEXT_DIM: Color = Color::RGB(140, 145, 158);
    pub const TEXT_FAINT: Color = Color::RGB(86, 90, 102);
    pub const LINE: Color = Color::RGBA(255, 255, 255, 22);
    pub const PANEL: Color = Color::RGB(12, 13, 17);
    pub const PANEL_HI: Color = Color::RGB(22, 25, 32);
    pub const OK: Color = Color::RGB(90, 215, 140);
    pub const WARN: Color = Color::RGB(240, 180, 80);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Face {
    Regular,
    Semi,
    Light,
}

struct Glyph {
    tex: Option<Texture<'static>>,
    w: u32,
    h: u32,
    xmin: i32,
    ymin: i32,
    advance: f32,
}

pub struct Textures {
    pub block: Texture<'static>,
    pub glow: Texture<'static>,
    pub outline: Texture<'static>,
    pub dot: Texture<'static>,
    pub vgrad: Texture<'static>,
    pub hband: Texture<'static>,
    pub circle: Texture<'static>,
    pub patterns: Vec<Texture<'static>>,
    /// The launcher icon artwork (black keyed to transparent).
    pub brand: Texture<'static>,
}

pub struct Gfx {
    pub canvas: Canvas<Window>,
    pub tc: &'static TextureCreator<WindowContext>,
    fonts: [Font; 3],
    glyphs: HashMap<(char, u32, Face), Glyph>,
    pub tex: Textures,
    rings: HashMap<(i32, i32), Texture<'static>>,
    pub raster_scale: f32,
}

static REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
static SEMI: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");
static LIGHT: &[u8] = include_bytes!("../assets/fonts/InterDisplay-Light.ttf");
static BRAND: &[u8] = include_bytes!("../assets/brand_150x182.rgba");

const BLOCK_PX: u32 = 64;

fn make_tex(
    tc: &'static TextureCreator<WindowContext>,
    w: u32,
    h: u32,
    f: impl Fn(f32, f32) -> [u8; 4],
) -> Result<Texture<'static>, String> {
    let mut t = tc
        .create_texture_static(PixelFormatEnum::ABGR8888, w, h)
        .map_err(|e| e.to_string())?;
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            px.extend_from_slice(&f((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32));
        }
    }
    t.update(None, &px, (w * 4) as usize).map_err(|e| e.to_string())?;
    t.set_blend_mode(BlendMode::Blend);
    Ok(t)
}

/// Signed distance to a rounded square of half-size 0.5 with corner radius `r` (unit space).
fn rounded_sd(x: f32, y: f32, r: f32) -> f32 {
    let qx = (x - 0.5).abs() - (0.5 - r);
    let qy = (y - 0.5).abs() - (0.5 - r);
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
}

fn pattern(kind: usize, x: f32, y: f32) -> f32 {
    // Distinct, low-key inner marks for colour-assisted play.
    let cx = x - 0.5;
    let cy = y - 0.5;
    let r = (cx * cx + cy * cy).sqrt();
    let aa = |d: f32| (0.5 - d * 64.0).clamp(0.0, 1.0);
    match kind {
        0 => aa((cx.abs().max(cy.abs()) - 0.14).abs() - 0.035), // square ring (I)
        1 => aa(r - 0.11),                                      // dot (O)
        2 => aa((cy + 0.08).abs() - 0.04).min(aa(cx.abs() - 0.2)), // bar (T)
        3 => aa(((cx - cy) / 1.414).abs() - 0.04).min(aa(r - 0.22)), // slash (S)
        4 => aa(((cx + cy) / 1.414).abs() - 0.04).min(aa(r - 0.22)), // backslash (Z)
        5 => aa((r - 0.15).abs() - 0.035),                      // ring (J)
        _ => aa(cx.abs().min(cy.abs()) - 0.035).min(aa(r - 0.2)), // plus (L)
    }
}

impl Gfx {
    pub fn new(mut canvas: Canvas<Window>) -> Result<Gfx, String> {
        canvas
            .set_logical_size(W as u32, H as u32)
            .map_err(|e| e.to_string())?;
        canvas.set_blend_mode(BlendMode::Blend);
        let (dw, _) = canvas.output_size()?;
        let raster_scale = (dw as f32 / W as f32).clamp(0.5, 2.0);
        let tc: &'static TextureCreator<WindowContext> = Box::leak(Box::new(canvas.texture_creator()));
        let font = |b: &[u8]| Font::from_bytes(b, FontSettings::default()).map_err(|e| e.to_string());
        let fonts = [font(REGULAR)?, font(SEMI)?, font(LIGHT)?];
        let n = BLOCK_PX;
        let block = make_tex(tc, n, n, |x, y| {
            let d = rounded_sd(x, y, 0.2);
            let a = (0.5 - d * n as f32).clamp(0.0, 1.0);
            // Soft bevel: lighter top-left rim, slightly darker centre-bottom.
            let rim = (1.0 - (-d * 9.0).clamp(0.0, 1.0)).powf(2.0);
            let light = 0.80 + 0.20 * (1.0 - y) + 0.30 * rim * (1.2 - (x + y) * 0.5);
            let v = (light.clamp(0.0, 1.0) * 255.0) as u8;
            [v, v, v, (a * 255.0) as u8]
        })?;
        let glow = make_tex(tc, 128, 128, |x, y| {
            let dx = x - 0.5;
            let dy = y - 0.5;
            let r2 = (dx * dx + dy * dy) * 4.0;
            let a = (-r2 * 3.0).exp() * (1.0 - r2).max(0.0).powi(2);
            [255, 255, 255, (a * 255.0) as u8]
        })?;
        let outline = make_tex(tc, n, n, |x, y| {
            let d = rounded_sd(x, y, 0.2);
            let a = (1.0 - ((d + 0.035).abs() * n as f32 - 1.2).max(0.0)).clamp(0.0, 1.0);
            [255, 255, 255, (a * 255.0) as u8]
        })?;
        let dot = make_tex(tc, 32, 32, |x, y| {
            let r = ((x - 0.5).powi(2) + (y - 0.5).powi(2)).sqrt() * 2.0;
            let a = (1.0 - r).max(0.0).powf(1.8);
            [255, 255, 255, (a * 255.0) as u8]
        })?;
        let vgrad = make_tex(tc, 16, 128, |x, y| {
            let edge = 1.0 - ((x - 0.5).abs() * 2.0).powf(2.0);
            let a = y.powf(1.6) * edge.max(0.0);
            [255, 255, 255, (a * 255.0) as u8]
        })?;
        let hband = make_tex(tc, 128, 32, |x, y| {
            let ex = 1.0 - ((x - 0.5).abs() * 2.0).powf(3.0);
            let ey = (-((y - 0.5) * 4.0).powi(2)).exp();
            [255, 255, 255, ((ex * ey).max(0.0) * 255.0) as u8]
        })?;
        let circle = make_tex(tc, 64, 64, |x, y| {
            let r = ((x - 0.5).powi(2) + (y - 0.5).powi(2)).sqrt();
            let a = ((0.5 - r) * 64.0 + 0.5).clamp(0.0, 1.0);
            [255, 255, 255, (a * 255.0) as u8]
        })?;
        let mut brand = tc
            .create_texture_static(PixelFormatEnum::ABGR8888, 150, 182)
            .map_err(|e| e.to_string())?;
        brand.update(None, BRAND, 150 * 4).map_err(|e| e.to_string())?;
        brand.set_blend_mode(BlendMode::Blend);
        let mut patterns = Vec::new();
        for k in 0..7 {
            patterns.push(make_tex(tc, n, n, |x, y| {
                [0, 0, 0, (pattern(k, x, y) * 150.0) as u8]
            })?);
        }
        Ok(Gfx {
            canvas,
            tc,
            fonts,
            glyphs: HashMap::new(),
            tex: Textures {
                block,
                glow,
                outline,
                dot,
                vgrad,
                hband,
                circle,
                patterns,
                brand,
            },
            rings: HashMap::new(),
            raster_scale,
        })
    }

    pub fn clear(&mut self) {
        self.canvas.set_draw_color(theme::BG);
        self.canvas.clear();
    }

    pub fn fill(&mut self, r: Rect, c: Color) {
        self.canvas.set_draw_color(c);
        let _ = self.canvas.fill_rect(r);
    }

    /// Copy a texture tinted with `c`, using `add` for additive glow.
    pub fn tinted(tex: &mut Texture<'static>, canvas: &mut Canvas<Window>, dst: Rect, c: Color, add: bool) {
        tex.set_color_mod(c.r, c.g, c.b);
        tex.set_alpha_mod(c.a);
        tex.set_blend_mode(if add { BlendMode::Add } else { BlendMode::Blend });
        let _ = canvas.copy(tex, None, dst);
    }

    /// Filled rounded rectangle.
    pub fn round(&mut self, r: Rect, radius: i32, c: Color) {
        let rad = radius.min(r.width() as i32 / 2).min(r.height() as i32 / 2).max(0);
        if rad == 0 {
            return self.fill(r, c);
        }
        let (x, y, w, h) = (r.x(), r.y(), r.width() as i32, r.height() as i32);
        self.canvas.set_draw_color(c);
        let _ = self
            .canvas
            .fill_rect(Rect::new(x + rad, y, (w - 2 * rad) as u32, h as u32));
        let _ = self
            .canvas
            .fill_rect(Rect::new(x, y + rad, rad as u32, (h - 2 * rad).max(0) as u32));
        let _ = self.canvas.fill_rect(Rect::new(
            x + w - rad,
            y + rad,
            rad as u32,
            (h - 2 * rad).max(0) as u32,
        ));
        let t = &mut self.tex.circle;
        t.set_color_mod(c.r, c.g, c.b);
        t.set_alpha_mod(c.a);
        t.set_blend_mode(BlendMode::Blend);
        let half = 32;
        for (sx, sy, dx, dy) in [
            (0, 0, x, y),
            (half, 0, x + w - rad, y),
            (0, half, x, y + h - rad),
            (half, half, x + w - rad, y + h - rad),
        ] {
            let _ = self.canvas.copy(
                t,
                Rect::new(sx, sy, half as u32, half as u32),
                Rect::new(dx, dy, rad as u32, rad as u32),
            );
        }
    }

    /// Rounded outline drawn just outside `r` (focus ring).
    pub fn round_outline(&mut self, r: Rect, radius: i32, c: Color, thickness: i32) {
        let t = thickness.max(1);
        let (x, y, w, h) = (
            r.x() - t,
            r.y() - t,
            r.width() as i32 + 2 * t,
            r.height() as i32 + 2 * t,
        );
        let rad = (radius + t).min(w / 2).min(h / 2).max(t);
        self.fill(Rect::new(x + rad, y, (w - 2 * rad).max(0) as u32, t as u32), c);
        self.fill(
            Rect::new(x + rad, y + h - t, (w - 2 * rad).max(0) as u32, t as u32),
            c,
        );
        self.fill(Rect::new(x, y + rad, t as u32, (h - 2 * rad).max(0) as u32), c);
        self.fill(
            Rect::new(x + w - t, y + rad, t as u32, (h - 2 * rad).max(0) as u32),
            c,
        );
        let key = (rad, t);
        if !self.rings.contains_key(&key) {
            let d = (rad * 2) as u32;
            let rr = rad as f32;
            let tt = t as f32;
            if let Ok(tex) = make_tex(self.tc, d, d, |u, v| {
                let dx = u * d as f32 - rr;
                let dy = v * d as f32 - rr;
                let dist = (dx * dx + dy * dy).sqrt();
                let outer = (rr - dist + 0.5).clamp(0.0, 1.0);
                let inner = (dist - (rr - tt) + 0.5).clamp(0.0, 1.0);
                [255, 255, 255, (outer * inner * 255.0) as u8]
            }) {
                self.rings.insert(key, tex);
            }
        }
        if let Some(tex) = self.rings.get_mut(&key) {
            tex.set_color_mod(c.r, c.g, c.b);
            tex.set_alpha_mod(c.a);
            let rr = rad as u32;
            for (sx, sy, dx, dy) in [
                (0, 0, x, y),
                (rad, 0, x + w - rad, y),
                (0, rad, x, y + h - rad),
                (rad, rad, x + w - rad, y + h - rad),
            ] {
                let _ = self
                    .canvas
                    .copy(tex, Rect::new(sx, sy, rr, rr), Rect::new(dx, dy, rr, rr));
            }
        }
    }

    fn glyph(&mut self, ch: char, px: f32, face: Face) -> &Glyph {
        let key = (ch, (px * 4.0) as u32, face);
        if !self.glyphs.contains_key(&key) {
            let font = &self.fonts[face as usize];
            let (m, bitmap) = font.rasterize(ch, px * self.raster_scale);
            let tex = if m.width > 0 && m.height > 0 {
                self.tc
                    .create_texture_static(PixelFormatEnum::ABGR8888, m.width as u32, m.height as u32)
                    .ok()
                    .map(|mut t| {
                        let mut rgba = Vec::with_capacity(bitmap.len() * 4);
                        for a in bitmap {
                            rgba.extend_from_slice(&[255, 255, 255, a]);
                        }
                        let _ = t.update(None, &rgba, m.width * 4);
                        t.set_blend_mode(BlendMode::Blend);
                        t
                    })
            } else {
                None
            };
            let s = self.raster_scale;
            if self.glyphs.len() > 3000 {
                self.glyphs.clear(); // bounded cache; rebuilt lazily
            }
            self.glyphs.insert(
                key,
                Glyph {
                    tex,
                    w: (m.width as f32 / s).round() as u32,
                    h: (m.height as f32 / s).round() as u32,
                    xmin: (m.xmin as f32 / s).round() as i32,
                    ymin: (m.ymin as f32 / s).round() as i32,
                    advance: m.advance_width / s,
                },
            );
        }
        &self.glyphs[&key]
    }

    pub fn text_width(&mut self, s: &str, px: f32, face: Face) -> i32 {
        let mut w = 0.0;
        for ch in s.chars() {
            w += self.glyph(ch, px, face).advance;
        }
        w.round() as i32
    }

    /// Draw text with its top-left at (x, y); returns the advance width.
    pub fn text(&mut self, s: &str, x: i32, y: i32, px: f32, face: Face, c: Color) -> i32 {
        let ascent = px * 0.95;
        let mut pen = x as f32;
        for ch in s.chars() {
            let (w, h, xmin, ymin, adv) = {
                let g = self.glyph(ch, px, face);
                (g.w, g.h, g.xmin, g.ymin, g.advance)
            };
            let key = (ch, (px * 4.0) as u32, face);
            if let Some(tex) = self.glyphs.get_mut(&key).and_then(|g| g.tex.as_mut()) {
                tex.set_color_mod(c.r, c.g, c.b);
                tex.set_alpha_mod(c.a);
                let gy = y as f32 + ascent - h as f32 - ymin as f32;
                let dst = Rect::new(
                    (pen + xmin as f32).round() as i32,
                    gy.round() as i32,
                    w.max(1),
                    h.max(1),
                );
                let _ = self.canvas.copy(tex, None, dst);
            }
            pen += adv;
        }
        (pen - x as f32).round() as i32
    }

    pub fn text_centered(&mut self, s: &str, cx: i32, y: i32, px: f32, face: Face, c: Color) {
        let w = self.text_width(s, px, face);
        self.text(s, cx - w / 2, y, px, face, c);
    }

    pub fn text_right(&mut self, s: &str, rx: i32, y: i32, px: f32, face: Face, c: Color) {
        let w = self.text_width(s, px, face);
        self.text(s, rx - w, y, px, face, c);
    }

    /// Word-wrapped paragraph; returns the height used.
    pub fn paragraph(&mut self, s: &str, x: i32, y: i32, max_w: i32, px: f32, c: Color) -> i32 {
        let lh = (px * 1.4) as i32;
        let mut cy = y;
        for para in s.split('\n') {
            let mut line = String::new();
            for word in para.split(' ') {
                let cand = if line.is_empty() {
                    word.to_owned()
                } else {
                    format!("{line} {word}")
                };
                if self.text_width(&cand, px, Face::Regular) > max_w && !line.is_empty() {
                    self.text(&line, x, cy, px, Face::Regular, c);
                    cy += lh;
                    line = word.to_owned();
                } else {
                    line = cand;
                }
            }
            self.text(&line, x, cy, px, Face::Regular, c);
            cy += lh;
        }
        cy - y
    }
}

/// Scale a colour's RGB by `k` (alpha kept).
pub fn scale(c: Color, k: f32) -> Color {
    let f = |v: u8| ((v as f32 * k).clamp(0.0, 255.0)) as u8;
    Color::RGBA(f(c.r), f(c.g), f(c.b), c.a)
}

pub fn alpha(c: Color, a: f32) -> Color {
    Color::RGBA(c.r, c.g, c.b, (a.clamp(0.0, 1.0) * 255.0) as u8)
}
