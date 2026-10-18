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
