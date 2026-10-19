//! Process-level platform setup: monotonic clock, app identity, data directory, logging and
//! webOS SDL hints. Approach adapted from the same owner's webos-rustdesk-client.

use std::{io::Write, path::PathBuf, sync::Mutex, time::Instant};

static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Monotonic microseconds since process start (shared by input, sensor and simulation).
pub fn now_us() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_micros() as u64
}

pub fn is_webos() -> bool {
    cfg!(target_arch = "arm")
}

/// The application id the launcher started us as (from the packaged appinfo.json next to
/// `bin/`), falling back to the compiled default. Debug and release builds have separate ids
/// and therefore separate data directories.
pub fn app_id() -> String {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent().and_then(|b| b.parent()) {
            if let Ok(t) = std::fs::read_to_string(dir.join("appinfo.json")) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                    if let Some(id) = v.get("id").and_then(|i| i.as_str()) {
                        return id.to_owned();
                    }
                }
            }
        }
    }
    "io.github.danzig666.lumablocks.debug".to_owned()
}

/// Private, persistent data directory. On webOS the launcher sets `HOME` to the app's own
/// directory; data lives in `$HOME/data`. `LUMA_DATA_DIR` overrides (tests, simulator).
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("LUMA_DATA_DIR") {
        let p = PathBuf::from(d);
        let _ = std::fs::create_dir_all(&p);
        return p;
    }
    let base = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/luma-blocks"));
    let dir = if is_webos() {
        base.join("data")
    } else {
        base.join(".local/share/luma-blocks")
    };
    if std::fs::create_dir_all(&dir).is_ok() {
        dir
    } else {
        let t = PathBuf::from("/tmp/luma-blocks");
        let _ = std::fs::create_dir_all(&t);
        t
    }
}

struct FileLogger {
    file: Mutex<Option<std::fs::File>>,
    level: log::LevelFilter,
}

impl log::Log for FileLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= self.level
    }
    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let line = format!(
            "{:>10.3} {:5} {}: {}\n",
            now_us() as f64 / 1e6,
            r.level(),
            r.target(),
            r.args()
        );
        eprint!("{line}");
        if let Ok(mut g) = self.file.lock() {
            if let Some(f) = g.as_mut() {
                let _ = f.write_all(line.as_bytes());
            }
        }
    }
    fn flush(&self) {}
}

/// Log to stderr and `<data>/logs/luma-blocks.log` (previous run kept as `.1`).
pub fn init_logging(dir: &std::path::Path) -> PathBuf {
    let logs = dir.join("logs");
    let _ = std::fs::create_dir_all(&logs);
    let path = logs.join("luma-blocks.log");
    let _ = std::fs::rename(&path, logs.join("luma-blocks.log.1"));
    let file = std::fs::File::create(&path).ok();
    let level = if std::env::var("LUMA_DEBUG").is_ok() {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };
    let logger = Box::leak(Box::new(FileLogger {
        file: Mutex::new(file),
        level,
    }));
    let _ = log::set_logger(logger);
    log::set_max_level(level);
    path
}

/// webOS SDL hints, set before the window is created.
/// * Back/Exit are delivered to the app (without both, a long Back press makes webOS
///   terminate the app, reported by punktfunk docs/NOTES.md). Home stays with the system.
/// * The system cursor sleeps after 30 s without motion; gameplay may hide it explicitly.
pub fn set_sdl_hints() {
    use sdl2::hint::set;
    set("SDL_WEBOS_ACCESS_POLICY_KEYS_BACK", "true");
    set("SDL_WEBOS_ACCESS_POLICY_KEYS_EXIT", "true");
    set("SDL_WEBOS_CURSOR_SLEEP_TIME", "30000");
    set("SDL_TOUCH_MOUSE_EVENTS", "0");
    set("SDL_RENDER_SCALE_QUALITY", "1");
}

/// webOS release string, for diagnostics.
pub fn webos_release() -> Option<String> {
    for p in ["/var/run/nyx/os_info.json", "/etc/starfish-release"] {
        if let Ok(t) = std::fs::read_to_string(p) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                if let Some(r) = v.get("webos_release").and_then(|x| x.as_str()) {
                    return Some(r.to_owned());
                }
            }
            return Some(t.lines().next().unwrap_or("").trim().to_owned());
        }
    }
    None
}

/// The pointer image the app hands to the compositor.
///
/// SDL-webOS builds its default arrow from LG's cursor PNGs. When those cannot be loaded, the
/// arrow is empty: the TV's pointer still shows until the app first hides the cursor, but
/// after that the hidden (transparent) image is never replaced again. An image of our own
/// keeps hiding and showing reliable. Scaled to the window height.
pub fn make_cursor(window_h: u32) -> Result<sdl2::mouse::Cursor, String> {
    let (s, hot) = cursor_surface(window_h)?;
    sdl2::mouse::Cursor::from_surface(&s, hot, hot)
}

fn cursor_surface(window_h: u32) -> Result<(sdl2::surface::Surface<'static>, i32), String> {
    use sdl2::{pixels::PixelFormatEnum, surface::Surface};
    // Classic arrow outline in a 12 x 19 unit grid (tip at the origin).
    const ARROW: [(f32, f32); 7] = [
        (0.0, 0.0),
        (0.0, 16.0),
        (4.0, 12.5),
        (6.8, 18.6),
        (9.4, 17.4),
        (6.7, 11.5),
        (11.6, 11.5),
    ];
    let k = (window_h as f32 / 1080.0 * 2.6).clamp(1.5, 6.0);
    let pad = (3.0 * k).ceil();
    let w = (12.0 * k + 2.0 * pad).ceil() as u32;
    let h = (19.0 * k + 2.0 * pad).ceil() as u32;
    let pts: Vec<(f32, f32)> = ARROW.iter().map(|(x, y)| (x * k + pad, y * k + pad)).collect();
    let inside = |x: f32, y: f32| {
        let mut c = false;
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            if (a.1 > y) != (b.1 > y) && x < a.0 + (y - a.1) / (b.1 - a.1) * (b.0 - a.0) {
                c = !c;
            }
        }
        c
    };
    let edge = |x: f32, y: f32| {
        let mut d = f32::MAX;
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            let (vx, vy) = (b.0 - a.0, b.1 - a.1);
            let t = (((x - a.0) * vx + (y - a.1) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
            let (dx, dy) = (x - a.0 - t * vx, y - a.1 - t * vy);
            d = d.min((dx * dx + dy * dy).sqrt());
        }
        d
    };
    let mut s = Surface::new(w, h, PixelFormatEnum::ARGB8888)?;
    let pitch = s.pitch() as usize;
    let border = 0.7 * k;
    s.with_lock_mut(|px| {
        for y in 0..h as usize {
            for x in 0..w as usize {
                // 4 x 4 supersampling: white body, dark border, soft shadow.
                let (mut white, mut dark) = (0.0f32, 0.0f32);
                for sy in 0..4 {
                    for sx in 0..4 {
                        let fx = x as f32 + (sx as f32 + 0.5) / 4.0;
                        let fy = y as f32 + (sy as f32 + 0.5) / 4.0;
                        if inside(fx, fy) {
                            if edge(fx, fy) > border {
                                white += 1.0;
                            } else {
                                dark += 1.0;
                            }
                        } else if edge(fx, fy) < border * 0.5 {
                            dark += 1.0;
                        }
                    }
                }
                let (white, dark) = (white / 16.0, dark / 16.0);
                let fx = x as f32 + 0.5 - 0.8 * k;
                let fy = y as f32 + 0.5 - 1.2 * k;
                let shadow = if inside(fx, fy) || edge(fx, fy) < 1.5 * k {
                    0.35 * (1.0 - edge(fx, fy) / (1.5 * k)).clamp(0.3, 1.0)
                } else {
                    0.0
                };
                let a = (white + dark).min(1.0);
                let a_all = a + shadow * (1.0 - a);
                let v = if a_all > 0.0 { white / a_all } else { 0.0 };
                let c = (v * 255.0).round() as u32;
                let alpha = (a_all * 255.0).round() as u32;
                let o = y * pitch + x * 4;
                px[o..o + 4].copy_from_slice(&((alpha << 24) | (c << 16) | (c << 8) | c).to_ne_bytes());
            }
        }
    });
    Ok((s, pad as i32))
}

#[cfg(test)]
mod cursor_test {
    #[test]
    fn dump_cursor() {
        let (s, _) = super::cursor_surface(1080).unwrap();
        if let Ok(p) = std::env::var("LUMA_CURSOR_DUMP") {
            s.save_bmp(p).unwrap();
        }
    }
}
