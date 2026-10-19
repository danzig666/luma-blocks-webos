//! Local persistence: settings, calibration, advanced thresholds, records and the resumable
//! game. Each lives in its own versioned JSON file so a corrupt file never takes the others
//! with it; a file that fails to parse or validate is renamed to `*.corrupt` and replaced by
//! defaults. Writes are atomic (temp file, fsync, rename) and happen on a background thread,
//! never in the frame loop.

use luma_core::{Game, Mode};
use luma_input::buttons::{Bindings, RepeatTiming};
use luma_input::calib::Calibration;
use luma_input::config::{Profile, Thresholds};
use luma_input::mrcu::LegacyQuatOrder;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};

pub const SETTINGS_VERSION: u32 = 2;
pub const SAVE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ControlMode {
    /// Remote buttons only; motion has no gameplay effect.
    Buttons,
    /// Motion gestures plus buttons (default when a supported remote is present).
    #[default]
    Motion,
}

/// "Move after a drop" choices, ms.
pub const DROP_SLIDE_SHORT_MS: u32 = 300;
pub const DROP_SLIDE_LONG_MS: u32 = 600;
/// While a swing is under way, a sliding piece waits up to this much longer to lock, µs.
pub const DROP_SLIDE_SWING_HOLD_US: u64 = 400_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub first_run_done: bool,
    pub control: ControlMode,
    pub profile: Profile,
    pub bindings: Bindings,
    pub repeat: RepeatTiming,
    pub ghost: bool,
    /// Glow and brightness of pieces, 0.4 .. 1.4.
    pub glow: f32,
    pub reduced_motion: bool,
    /// Distinct inner patterns per piece colour.
    pub patterns: bool,
    /// Effects volume 0..=10.
    pub volume: u32,
    pub mute: bool,
    /// Menus and pause dim after this many seconds without intentional input.
    pub idle_dim_s: u32,
    /// Gameplay pauses after this many seconds without intentional input (0 = never).
    pub auto_pause_s: u32,
    pub marathon_start_level: u32,
    pub zen_level: u32,
    /// After a hard drop the piece can still be slid or turned this long, ms (0 = off).
    pub drop_slide_ms: u32,
    /// Hide the system cursor during play while the motion sensor is streaming.
    pub hide_cursor: bool,
    pub developer: bool,
    pub legacy_quat_order: LegacyQuatOrder,
    /// Buttons that have actually been delivered on this TV (shortcut hints show only these).
    pub seen_buttons: Vec<luma_input::buttons::Button>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: SETTINGS_VERSION,
            first_run_done: false,
            control: ControlMode::Motion,
            profile: Profile::default(),
            bindings: Bindings::default(),
            repeat: RepeatTiming::default(),
            ghost: true,
            glow: 1.0,
            reduced_motion: false,
            patterns: false,
            volume: 6,
            mute: false,
            idle_dim_s: 90,
            auto_pause_s: 120,
            marathon_start_level: 1,
            zen_level: 1,
            drop_slide_ms: DROP_SLIDE_SHORT_MS,
            hide_cursor: true,
            developer: false,
            legacy_quat_order: LegacyQuatOrder::default(),
            seen_buttons: Vec::new(),
        }
    }
}

impl Settings {
    pub fn sanitize(&mut self) {
        self.profile.clamp();
        // 0.2.2 - 0.2.5 offered 0.2 s and 0.4 s; both turned out too short for a swing.
        self.drop_slide_ms = match self.drop_slide_ms {
            200 => DROP_SLIDE_SHORT_MS,
            400 => DROP_SLIDE_LONG_MS,
            ms => ms.min(DROP_SLIDE_LONG_MS),
        };
        self.glow = if self.glow.is_finite() {
            self.glow.clamp(0.4, 1.4)
        } else {
            1.0
        };
        self.volume = self.volume.min(10);
        self.idle_dim_s = self.idle_dim_s.clamp(15, 600);
        self.auto_pause_s = if self.auto_pause_s == 0 {
            0
        } else {
            self.auto_pause_s.clamp(30, 900)
        };
        self.marathon_start_level = self.marathon_start_level.clamp(1, 15);
        self.zen_level = self.zen_level.clamp(1, 10);
        self.repeat.das_ms = self.repeat.das_ms.clamp(60, 400);
        self.repeat.arr_ms = self.repeat.arr_ms.clamp(16, 200);
        self.seen_buttons.sort();
        self.seen_buttons.dedup();
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarathonRecord {
    pub score: u64,
    pub lines: u32,
    pub level: u32,
    pub pieces: u32,
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SprintRecord {
    pub time_ms: u64,
    pub pieces: u32,
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Records {
    pub version: u32,
    pub marathon: Vec<MarathonRecord>,
    pub sprint: Vec<SprintRecord>,
    pub games_played: u64,
    pub seq: u64,
}

pub const TABLE: usize = 10;

impl Records {
    /// Insert and return the 1-based rank if it made the table.
    pub fn add_marathon(&mut self, g: &Game) -> Option<usize> {
        self.seq += 1;
        let r = MarathonRecord {
            score: g.score,
            lines: g.lines,
            level: g.level,
            pieces: g.pieces_placed,
            seq: self.seq,
        };
        self.marathon.push(r);
        self.marathon
            .sort_by(|a, b| b.score.cmp(&a.score).then(a.seq.cmp(&b.seq)));
        self.marathon.truncate(TABLE);
        self.marathon
            .iter()
            .position(|x| x.seq == self.seq)
            .map(|i| i + 1)
    }
    pub fn add_sprint(&mut self, g: &Game) -> Option<usize> {
        self.seq += 1;
        let r = SprintRecord {
            time_ms: g.elapsed_us / 1000,
            pieces: g.pieces_placed,
            seq: self.seq,
        };
        self.sprint.push(r);
        self.sprint
            .sort_by(|a, b| a.time_ms.cmp(&b.time_ms).then(a.seq.cmp(&b.seq)));
        self.sprint.truncate(TABLE);
        self.sprint.iter().position(|x| x.seq == self.seq).map(|i| i + 1)
    }
    pub fn best_marathon(&self) -> Option<u64> {
        self.marathon.first().map(|r| r.score)
    }
    pub fn best_sprint(&self) -> Option<u64> {
        self.sprint.first().map(|r| r.time_ms)
    }
}

/// The resumable game: the complete engine state (board, active piece, bag/RNG, queue, hold,
/// score, mode and timing). Restored paused, with input state cleared.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveGame {
    pub version: u32,
    pub game: Game,
}

impl SaveGame {
    pub fn mode(&self) -> Mode {
        self.game.mode
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Envelope<T> {
    version: u32,
    data: T,
}

enum Job {
    Write(PathBuf, Vec<u8>),
    Remove(PathBuf),
    Flush(Sender<()>),
}

pub struct Store {
    pub dir: PathBuf,
    tx: Sender<Job>,
    pub settings: Settings,
    pub calibration: Calibration,
    pub thresholds: Thresholds,
    pub records: Records,
    pub save: Option<SaveGame>,
    pub load_notes: Vec<String>,
}

fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    if let Some(d) = path.parent() {
        if let Ok(df) = std::fs::File::open(d) {
            let _ = df.sync_all();
        }
    }
    Ok(())
}

fn load<T: DeserializeOwned + Default>(dir: &Path, name: &str, version: u32, notes: &mut Vec<String>) -> T {
    let path = dir.join(name);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return T::default();
    };
    match serde_json::from_str::<Envelope<T>>(&text) {
        Ok(e) if e.version <= version => e.data,
        Ok(e) => {
            notes.push(format!("{name}: newer version {} ignored", e.version));
            T::default()
        }
        Err(err) => {
            notes.push(format!("{name}: unreadable ({err}); defaults used"));
            let _ = std::fs::rename(&path, dir.join(format!("{name}.corrupt")));
            T::default()
        }
    }
}

impl Store {
    pub fn open(dir: &Path) -> Store {
        let _ = std::fs::create_dir_all(dir);
        let mut notes = Vec::new();
        let mut settings: Settings = load(dir, "settings.json", SETTINGS_VERSION, &mut notes);
        if settings.version < 2 {
            // 0.1.0 defaulted to absolute pointing, which follows the remote back when the
            // player pulls it back; Swing replaces it as the default.
            settings.profile.horizontal = luma_input::config::HorizontalStyle::Swing;
            settings.version = 2;
            notes.push("settings migrated to version 2 (sideways style: Swing)".into());
        }
        settings.sanitize();
        let mut calibration: Calibration = load(dir, "calibration.json", 1, &mut notes);
        calibration.sanitize();
        let thresholds: Thresholds = load(dir, "thresholds.json", 1, &mut notes);
        let records: Records = load(dir, "records.json", 1, &mut notes);
        let save = Self::load_save(dir, &mut notes);
        let (tx, rx) = channel::<Job>();
        let _ = std::thread::Builder::new().name("store".into()).spawn(move || {
            for job in rx {
                match job {
                    Job::Write(p, d) => {
                        if let Err(e) = write_atomic(&p, &d) {
                            log::warn!("write {}: {e}", p.display());
                        }
                    }
                    Job::Remove(p) => {
                        let _ = std::fs::remove_file(p);
                    }
                    Job::Flush(done) => {
                        let _ = done.send(());
                    }
                }
            }
        });
        for n in &notes {
            log::warn!("store: {n}");
        }
        Store {
            dir: dir.to_path_buf(),
            tx,
            settings,
            calibration,
            thresholds,
            records,
            save,
            load_notes: notes,
        }
    }

    fn load_save(dir: &Path, notes: &mut Vec<String>) -> Option<SaveGame> {
        let path = dir.join("savegame.json");
        let text = std::fs::read_to_string(&path).ok()?;
        let parsed: Result<SaveGame, String> = serde_json::from_str::<SaveGame>(&text)
            .map_err(|e| e.to_string())
            .and_then(|s| {
                if s.version != SAVE_VERSION {
                    return Err(format!("version {}", s.version));
                }
                s.game.validate()?;
                if !s.game.is_playing() {
                    return Err("finished game".into());
                }
                Ok(s)
            });
        match parsed {
            Ok(mut s) => {
                s.game.clear_transient();
                Some(s)
            }
            Err(e) => {
                notes.push(format!("savegame.json discarded ({e})"));
                let _ = std::fs::rename(&path, dir.join("savegame.json.corrupt"));
                None
            }
        }
    }

    fn put<T: Serialize>(&self, name: &str, version: u32, data: &T) {
        let env = Envelope { version, data };
        if let Ok(bytes) = serde_json::to_vec_pretty(&env) {
            let _ = self.tx.send(Job::Write(self.dir.join(name), bytes));
        }
    }

    pub fn save_settings(&self) {
        self.put("settings.json", SETTINGS_VERSION, &self.settings);
    }
    pub fn save_calibration(&self) {
        self.put("calibration.json", 1, &self.calibration);
    }
    pub fn save_thresholds(&self) {
        self.put("thresholds.json", 1, &self.thresholds);
    }
    pub fn save_records(&self) {
        self.put("records.json", 1, &self.records);
    }

    /// Persist the running game (called at locks, on pause, background and exit).
    pub fn save_game(&mut self, g: &Game) {
        if !g.is_playing() {
            return self.clear_game();
        }
        let s = SaveGame {
            version: SAVE_VERSION,
            game: g.clone(),
        };
        if let Ok(bytes) = serde_json::to_vec(&s) {
            let _ = self.tx.send(Job::Write(self.dir.join("savegame.json"), bytes));
        }
        self.save = Some(s);
    }

    pub fn clear_game(&mut self) {
        self.save = None;
        let _ = self.tx.send(Job::Remove(self.dir.join("savegame.json")));
    }

    /// Block until queued writes are on disk (exit path only).
    pub fn flush(&self) {
        let (tx, rx) = channel();
        if self.tx.send(Job::Flush(tx)).is_ok() {
            let _ = rx.recv_timeout(std::time::Duration::from_secs(2));
        }
    }

    /// Write a trace file (bounded count; oldest removed).
    pub fn save_trace(&self, text: String) -> PathBuf {
        let dir = self.dir.join("traces");
        let _ = std::fs::create_dir_all(&dir);
        let mut existing: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|r| r.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default();
        existing.sort();
        while existing.len() >= 8 {
            let _ = self.tx.send(Job::Remove(existing.remove(0)));
        }
        let name = format!("trace-{:010}.jsonl", crate::platform::now_us() / 1000);
        let path = dir.join(name);
        let _ = self.tx.send(Job::Write(path.clone(), text.into_bytes()));
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use luma_core::{Action, Rules};

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("luma-store-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn save_and_restore_game_and_settings() {
        let d = tmpdir("rt");
        let mut s = Store::open(&d);
        s.settings.volume = 3;
        s.save_settings();
        let mut g = Game::new(Mode::Sprint, Rules::default(), 5);
        g.apply(Action::HardDrop);
        g.tick(1_000_000);
        g.apply(Action::SoftDropStart);
        s.save_game(&g);
        s.flush();
        let r = Store::open(&d);
        assert_eq!(r.settings.volume, 3);
        let sg = r.save.unwrap();
        assert_eq!(sg.game.mode, Mode::Sprint);
        assert_eq!(sg.game.board, g.board);
        assert_eq!(sg.game.elapsed_us, g.elapsed_us);
        assert!(!sg.game.soft_drop, "input state is cleared on restore");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn corrupt_file_does_not_affect_others() {
        let d = tmpdir("corrupt");
        let mut s = Store::open(&d);
        s.settings.volume = 2;
        s.save_settings();
        s.records.games_played = 9;
        s.save_records();
        s.flush();
        std::fs::write(d.join("records.json"), b"{ not json").unwrap();
        std::fs::write(d.join("savegame.json"), b"{\"version\":1,\"game\":{}}").unwrap();
        let r = Store::open(&d);
        assert_eq!(r.settings.volume, 2);
        assert_eq!(r.records.games_played, 0);
        assert!(r.save.is_none());
        assert!(d.join("records.json.corrupt").exists());
        assert!(!r.load_notes.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn old_slide_times_grow_to_the_new_choices() {
        for (old, new) in [(0, 0), (200, DROP_SLIDE_SHORT_MS), (400, DROP_SLIDE_LONG_MS)] {
            let mut st = Settings {
                drop_slide_ms: old,
                ..Settings::default()
            };
            st.sanitize();
            assert_eq!(st.drop_slide_ms, new);
            st.sanitize();
            assert_eq!(st.drop_slide_ms, new, "stable");
        }
    }

    #[test]
    fn settings_from_0_1_0_move_to_swing() {
        let d = tmpdir("migrate");
        std::fs::create_dir_all(&d).unwrap();
        let old = r#"{"version":1,"data":{"version":1,"first_run_done":true,"volume":4,"profile":{"rotation":"Twist","horizontal":"Pointing"}}}"#;
        std::fs::write(d.join("settings.json"), old).unwrap();
        let s = Store::open(&d);
        assert_eq!(
            s.settings.profile.horizontal,
            luma_input::config::HorizontalStyle::Swing
        );
        assert_eq!(s.settings.volume, 4);
        assert!(s.settings.first_run_done);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn records_tables() {
        let mut r = Records::default();
        let mut g = Game::new(Mode::Marathon, Rules::default(), 1);
        for sc in [100, 500, 300] {
            g.score = sc;
            r.add_marathon(&g);
        }
        assert_eq!(r.best_marathon(), Some(500));
        g.elapsed_us = 90_000_000;
        assert_eq!(r.add_sprint(&g), Some(1));
        g.elapsed_us = 80_000_000;
        assert_eq!(r.add_sprint(&g), Some(1));
        assert_eq!(r.best_sprint(), Some(80_000));
    }
}
