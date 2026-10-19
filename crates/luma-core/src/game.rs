//! The deterministic game engine: board, active piece, gravity, lock delay, hold, queue,
//! clears and modes. It knows nothing about rendering, input devices or wall-clock time:
//! the caller feeds [`Action`]s and elapsed simulation time in microseconds, and drains
//! [`Event`]s for sound and effects. The same action/tick sequence always produces the same
//! game, which is what replays, saves and tests rely on.
//!
//! Field: 10 columns x 40 rows, y downward. Rows 0..20 are the hidden spawn buffer and rows
//! 20..40 are the 20 visible rows. Pieces spawn with their box top-left at (3, 18) (O at
//! (4, 18)), i.e. in the two hidden rows directly above the field, then immediately drop
//! one row when nothing blocks them.
//!
//! Top-out rules:
//! * Block out: a newly spawned piece overlaps a settled block.
//! * Lock out: a piece locks with every cell in the hidden buffer.

use crate::piece::{kicks, Kind, Piece};
use crate::rng::Bag;
use crate::scoring::{Chain, ClearScore, Spin, HARD_DROP_POINTS_PER_ROW, SOFT_DROP_POINTS_PER_ROW};
use serde::{Deserialize, Serialize};

pub const COLS: i32 = 10;
pub const ROWS: i32 = 40;
pub const HIDDEN_ROWS: i32 = 20;
pub const VISIBLE_ROWS: i32 = ROWS - HIDDEN_ROWS;
pub const NEXT_COUNT: usize = 5;
pub const SPRINT_LINES: u32 = 40;

/// Time per row by level, from the common guideline curve
/// `(0.8 - (level - 1) * 0.007) ^ (level - 1)` seconds, precomputed in integer microseconds
/// so gravity is bit-identical on every platform.
pub const GRAVITY_US: [u32; 20] = [
    1_000_000, 793_000, 617_796, 472_729, 355_197, 262_004, 189_677, 134_735, 93_882, 64_152, 42_976, 28_218,
    18_153, 11_439, 7_059, 4_264, 2_520, 1_457, 824, 455,
];

pub fn row_time_us(level: u32) -> u32 {
    GRAVITY_US[(level.clamp(1, 20) - 1) as usize]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum Mode {
    Marathon,
    Sprint,
    Zen,
    Practice,
}

impl Mode {
    pub fn key(self) -> &'static str {
        match self {
            Mode::Marathon => "marathon",
            Mode::Sprint => "sprint",
            Mode::Zen => "zen",
            Mode::Practice => "practice",
        }
    }
}

/// Application defaults, not platform guarantees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rules {
    pub lock_delay_us: u32,
    pub max_lock_resets: u32,
    /// Soft drop moves this many times faster than gravity (at least one row per 33 ms).
    pub soft_drop_factor: u32,
    pub start_level: u32,
    /// Zen pace, as a level whose gravity is used without levelling up.
    pub zen_level: u32,
    /// After a hard drop the piece rests on the stack this long before it locks, and may
    /// still be slid sideways or turned (it drops straight down again). 0 = lock at once.
    #[serde(default)]
    pub drop_slide_us: u32,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            lock_delay_us: 500_000,
            max_lock_resets: 15,
            soft_drop_factor: 20,
            start_level: 1,
            zen_level: 1,
            drop_slide_us: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    MoveLeft,
    MoveRight,
    RotateCw,
    RotateCcw,
    HardDrop,
    Hold,
    SoftDropStart,
    SoftDropStop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Playing,
    /// Block out or lock out.
    ToppedOut,
    /// Sprint goal reached.
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Moved,
    Rotated {
        kick: usize,
        cw: bool,
    },
    /// An action that could not be applied (blocked move/rotation, second hold).
    Blocked,
    SoftDropRow,
    HardDrop {
        from: Piece,
        to: Piece,
    },
    Locked {
        cells: Vec<(i32, i32)>,
        kind: Kind,
    },
    Cleared {
        rows: Vec<i32>,
        score: ClearScore,
    },
    /// A T-spin or other scored lock without lines.
    SpinNoLines {
        score: ClearScore,
    },
    Held,
    Spawned,
    LevelUp(u32),
    ToppedOut,
    Finished,
    /// Practice mode reset the board after a top-out instead of ending.
    PracticeReset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Game {
    pub mode: Mode,
    pub rules: Rules,
    /// Row-major, `COLS * ROWS`, 0 = empty, otherwise `Kind::cell()`.
    pub board: Vec<u8>,
    pub active: Piece,
    pub queue: Vec<Kind>,
    pub bag: Bag,
    pub hold: Option<Kind>,
    pub hold_used: bool,
    pub score: u64,
    pub lines: u32,
    pub level: u32,
    pub chain: Chain,
    pub pieces_placed: u32,
    pub status: Status,
    /// Simulation time while playing, microseconds (pauses are simply not ticked).
    pub elapsed_us: u64,
    pub fall_acc_us: u64,
    pub lock_acc_us: u64,
    pub lock_resets: u32,
    pub lowest_y: i32,
    pub soft_drop: bool,
    pub last_was_rotation: bool,
    pub last_kick: usize,
    pub gravity_enabled: bool,
    pub seed: u64,
    /// Time left in the slide after a hard drop (0 = not sliding), microseconds.
    #[serde(default)]
    pub slide_left_us: u64,
    #[serde(skip)]
    events: Vec<Event>,
}

fn spawn_piece(kind: Kind) -> Piece {
    Piece {
        kind,
        rot: 0,
        x: if kind == Kind::O { 4 } else { 3 },
        y: 18,
    }
}

impl Game {
    pub fn new(mode: Mode, rules: Rules, seed: u64) -> Game {
        let mut bag = Bag::new(seed);
        let first = bag.next();
        let queue = (0..NEXT_COUNT).map(|_| bag.next()).collect();
        let level = match mode {
            Mode::Marathon => rules.start_level.clamp(1, 20),
            Mode::Zen => rules.zen_level.clamp(1, 20),
            Mode::Sprint | Mode::Practice => 1,
        };
        let mut g = Game {
            mode,
            board: vec![0; (COLS * ROWS) as usize],
            active: spawn_piece(first),
            queue,
            bag,
            hold: None,
            hold_used: false,
            score: 0,
            lines: 0,
            level,
            chain: Chain::new(),
            pieces_placed: 0,
            status: Status::Playing,
            elapsed_us: 0,
            fall_acc_us: 0,
            lock_acc_us: 0,
            lock_resets: 0,
            lowest_y: 0,
            soft_drop: false,
            last_was_rotation: false,
            last_kick: 0,
            gravity_enabled: mode != Mode::Practice,
            seed,
            slide_left_us: 0,
            rules,
            events: Vec::new(),
        };
        g.enter_piece(first);
        g
    }

    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub fn cell(&self, x: i32, y: i32) -> u8 {
        if !(0..COLS).contains(&x) || !(0..ROWS).contains(&y) {
            return 0;
        }
        self.board[(y * COLS + x) as usize]
    }

    /// Occupied for collision purposes: walls, floor and settled blocks.
    pub fn solid(&self, x: i32, y: i32) -> bool {
        if !(0..COLS).contains(&x) || y >= ROWS {
            return true;
        }
        if y < 0 {
            return false;
        }
        self.board[(y * COLS + x) as usize] != 0
    }

    pub fn fits(&self, p: &Piece) -> bool {
        p.cells().all(|(x, y)| !self.solid(x, y))
    }

    fn on_ground(&self) -> bool {
        let mut p = self.active;
        p.y += 1;
        !self.fits(&p)
    }

    /// Where the active piece would land (the ghost).
    pub fn ghost(&self) -> Piece {
        let mut p = self.active;
        loop {
            let mut n = p;
            n.y += 1;
            if !self.fits(&n) {
                return p;
            }
            p = n;
        }
    }

    pub fn is_playing(&self) -> bool {
        self.status == Status::Playing
    }

    fn row_time(&self) -> u64 {
        let base = row_time_us(self.level) as u64;
        if self.soft_drop {
            (base / self.rules.soft_drop_factor.max(1) as u64).clamp(1, 33_333)
        } else {
            base
        }
    }

    fn enter_piece(&mut self, kind: Kind) {
        self.slide_left_us = 0;
        self.active = spawn_piece(kind);
        self.lock_acc_us = 0;
        self.lock_resets = 0;
        self.fall_acc_us = 0;
        self.last_was_rotation = false;
        self.last_kick = 0;
        if !self.fits(&self.active) {
            self.top_out();
            return;
        }
        let mut d = self.active;
        d.y += 1;
        if self.fits(&d) {
            self.active = d;
        }
        self.lowest_y = self.active.y;
        self.events.push(Event::Spawned);
    }

    fn spawn_next(&mut self) {
        let kind = self.queue.remove(0);
        self.queue.push(self.bag.next());
        self.hold_used = false;
        self.enter_piece(kind);
    }

    fn top_out(&mut self) {
        if self.mode == Mode::Practice {
            // Practice never punishes: clear the field and carry on with the same queue.
            self.board.iter_mut().for_each(|c| *c = 0);
            self.events.push(Event::PracticeReset);
            self.active = spawn_piece(self.active.kind);
            self.lowest_y = self.active.y;
            return;
        }
        self.status = Status::ToppedOut;
        self.events.push(Event::ToppedOut);
    }

    /// Whether a hard-dropped piece is still sliding (movable, about to lock).
    pub fn is_sliding(&self) -> bool {
        self.slide_left_us > 0
    }

    /// Keep a sliding piece from locking a little longer (a move is on its way), never
    /// beyond the full slide time.
    pub fn hold_slide(&mut self, us: u64) {
        if self.is_sliding() {
            self.slide_left_us = (self.slide_left_us + us).min(self.rules.drop_slide_us as u64);
        }
    }

    /// A successful move or rotation while grounded spends one lock reset. While sliding
    /// after a hard drop the piece drops straight down again instead and the slide time is
    /// not extended.
    fn after_manipulation(&mut self) {
        if self.is_sliding() {
            let from = self.active;
            let to = self.ghost();
            if to.y > from.y {
                self.active = to;
                self.last_was_rotation = false;
                self.events.push(Event::HardDrop { from, to });
            }
            return;
        }
        if self.on_ground() && self.lock_resets < self.rules.max_lock_resets {
            self.lock_acc_us = 0;
            self.lock_resets += 1;
        }
    }

    fn try_shift(&mut self, dx: i32) -> bool {
        let mut p = self.active;
        p.x += dx;
        if self.fits(&p) {
            self.active = p;
            self.last_was_rotation = false;
            self.after_manipulation();
            self.events.push(Event::Moved);
            true
        } else {
            self.events.push(Event::Blocked);
            false
        }
    }

    fn try_rotate(&mut self, cw: bool) -> bool {
        let from = self.active.rot;
        let to = if cw { (from + 1) % 4 } else { (from + 3) % 4 };
        for (i, (dx, dy)) in kicks(self.active.kind, from, to).into_iter().enumerate() {
            let p = Piece {
                rot: to,
                x: self.active.x + dx,
                y: self.active.y + dy,
                ..self.active
            };
            if self.fits(&p) {
                self.active = p;
                self.last_was_rotation = true;
                self.last_kick = i;
                if p.y > self.lowest_y {
                    self.lowest_y = p.y;
                    self.lock_resets = 0;
                    self.lock_acc_us = 0;
                }
                self.after_manipulation();
                self.events.push(Event::Rotated { kick: i, cw });
                return true;
            }
        }
        self.events.push(Event::Blocked);
        false
    }

    /// Apply one discrete action. Returns whether it changed anything.
    pub fn apply(&mut self, a: Action) -> bool {
        if !self.is_playing() {
            return false;
        }
        match a {
            Action::MoveLeft => self.try_shift(-1),
            Action::MoveRight => self.try_shift(1),
            Action::RotateCw => self.try_rotate(true),
            Action::RotateCcw => self.try_rotate(false),
            Action::SoftDropStart => {
                self.soft_drop = true;
                // Do not carry a long gravity accumulator into the faster rate.
                self.fall_acc_us = self.fall_acc_us.min(self.row_time());
                true
            }
            Action::SoftDropStop => {
                self.soft_drop = false;
                true
            }
            Action::Hold => {
                if self.hold_used || self.is_sliding() {
                    self.events.push(Event::Blocked);
                    return false;
                }
                let current = self.active.kind;
                self.hold_used = true;
                self.events.push(Event::Held);
                match self.hold.replace(current) {
                    Some(k) => self.enter_piece(k),
                    None => {
                        let kind = self.queue.remove(0);
                        self.queue.push(self.bag.next());
                        self.enter_piece(kind);
                    }
                }
                self.hold_used = true;
                true
            }
            Action::HardDrop => {
                if self.is_sliding() {
                    // A second drop while sliding locks at once.
                    self.lock();
                    return true;
                }
                let from = self.active;
                let to = self.ghost();
                let rows = (to.y - from.y).max(0) as u64;
                if rows > 0 {
                    self.last_was_rotation = false;
                }
                self.active = to;
                self.score += rows * HARD_DROP_POINTS_PER_ROW;
                self.events.push(Event::HardDrop { from, to });
                if self.rules.drop_slide_us > 0 {
                    self.slide_left_us = self.rules.drop_slide_us as u64;
                    self.fall_acc_us = 0;
                    self.lock_acc_us = 0;
                } else {
                    self.lock();
                }
                true
            }
        }
    }

    /// Advance simulation time.
    pub fn tick(&mut self, dt_us: u64) {
        if !self.is_playing() {
            return;
        }
        self.elapsed_us += dt_us;
        if self.is_sliding() {
            self.slide_left_us = self.slide_left_us.saturating_sub(dt_us);
            if self.slide_left_us == 0 {
                self.lock();
            }
            return;
        }
        if self.gravity_enabled || self.soft_drop {
            self.fall_acc_us += dt_us;
            let rt = if self.gravity_enabled {
                self.row_time()
            } else {
                // Practice: no gravity, but soft drop still works at its normal speed.
                (row_time_us(1) as u64 / self.rules.soft_drop_factor.max(1) as u64).max(1)
            };
            while self.fall_acc_us >= rt {
                self.fall_acc_us -= rt;
                let mut p = self.active;
                p.y += 1;
                if self.fits(&p) {
                    self.active = p;
                    self.last_was_rotation = false;
                    if self.soft_drop {
                        self.score += SOFT_DROP_POINTS_PER_ROW;
                        self.events.push(Event::SoftDropRow);
                    }
                    if p.y > self.lowest_y {
                        self.lowest_y = p.y;
                        self.lock_resets = 0;
                        self.lock_acc_us = 0;
                    }
                } else {
                    self.fall_acc_us = 0;
                    break;
                }
            }
        } else {
            self.fall_acc_us = 0;
        }
        if self.on_ground() {
            self.lock_acc_us += dt_us;
            // Once resets are exhausted, manipulation no longer clears the timer, so the
            // piece locks when the delay that started at the last reset runs out.
            if self.lock_acc_us >= self.rules.lock_delay_us as u64 {
                self.lock();
            }
        } else {
            self.lock_acc_us = 0;
        }
    }

    fn spin_kind(&self) -> Spin {
        let p = self.active;
        if p.kind != Kind::T || !self.last_was_rotation {
            return Spin::None;
        }
        // Centre of the T is box cell (1, 1) in every rotation state.
        let (cx, cy) = (p.x + 1, p.y + 1);
        let corners = [
            (cx - 1, cy - 1),
            (cx + 1, cy - 1),
            (cx + 1, cy + 1),
            (cx - 1, cy + 1),
        ];
        let filled: Vec<bool> = corners.iter().map(|&(x, y)| self.solid(x, y)).collect();
        if filled.iter().filter(|f| **f).count() < 3 {
            return Spin::None;
        }
        // Front corners: the two on the side the T points to.
        let front = match p.rot {
            0 => [0, 1], // up
            1 => [1, 2], // right
            2 => [2, 3], // down
            _ => [3, 0], // left
        };
        if (filled[front[0]] && filled[front[1]]) || self.last_kick == 4 {
            Spin::Full
        } else {
            Spin::Mini
        }
    }

    fn lock(&mut self) {
        self.slide_left_us = 0;
        let spin = self.spin_kind();
        let cells: Vec<(i32, i32)> = self.active.cells().collect();
        let kind = self.active.kind;
        let mut all_hidden = true;
        for &(x, y) in &cells {
            if (0..ROWS).contains(&y) && (0..COLS).contains(&x) {
                self.board[(y * COLS + x) as usize] = kind.cell();
            }
            if y >= HIDDEN_ROWS {
                all_hidden = false;
            }
        }
        self.pieces_placed += 1;
        self.events.push(Event::Locked {
            cells: cells.clone(),
            kind,
        });
        // Clear full rows.
        let mut rows = Vec::new();
        for y in 0..ROWS {
            if (0..COLS).all(|x| self.board[(y * COLS + x) as usize] != 0) {
                rows.push(y);
            }
        }
        let n = rows.len() as u32;
        if n > 0 {
            let mut kept: Vec<u8> = Vec::with_capacity(self.board.len());
            for y in 0..ROWS {
                if !rows.contains(&y) {
                    kept.extend_from_slice(&self.board[(y * COLS) as usize..((y + 1) * COLS) as usize]);
                }
            }
            let mut nb = vec![0u8; (n as i32 * COLS) as usize];
            nb.extend(kept);
            self.board = nb;
        }
        let perfect = n > 0 && self.board.iter().all(|c| *c == 0);
        let score_level = self.level;
        let s = self.chain.score_lock(n, spin, perfect, score_level);
        self.score += s.points;
        if n > 0 {
            self.lines += n;
            self.events.push(Event::Cleared { rows, score: s });
            if self.mode == Mode::Marathon {
                let lv = (self.rules.start_level + self.lines / 10).max(self.level);
                if lv > self.level {
                    self.level = lv;
                    self.events.push(Event::LevelUp(lv));
                }
            }
        } else if spin != Spin::None {
            self.events.push(Event::SpinNoLines { score: s });
        }
        if self.mode == Mode::Sprint && self.lines >= SPRINT_LINES {
            self.status = Status::Finished;
            self.events.push(Event::Finished);
            return;
        }
        if all_hidden && n == 0 {
            self.top_out();
            if !self.is_playing() {
                return;
            }
        }
        self.spawn_next();
    }

    /// Visible-field height of the stack (0 = empty), for effects and auto-pause heuristics.
    pub fn stack_height(&self) -> i32 {
        for y in 0..ROWS {
            if (0..COLS).any(|x| self.board[(y * COLS + x) as usize] != 0) {
                return ROWS - y;
            }
        }
        0
    }

    /// Validate a restored game: dimensions, piece kinds, a collision-free active piece and a
    /// consistent bag. A save that fails is discarded instead of crashing or corrupting play.
    pub fn validate(&self) -> Result<(), String> {
        if self.board.len() != (COLS * ROWS) as usize {
            return Err("board size".into());
        }
        if self.board.iter().any(|c| *c > 7) {
            return Err("cell value".into());
        }
        if self.queue.len() != NEXT_COUNT {
            return Err("queue length".into());
        }
        if self.active.rot > 3 || !self.fits(&self.active) {
            return Err("active piece".into());
        }
        if !self.bag.is_valid() {
            return Err("bag".into());
        }
        if !(1..=40).contains(&self.level) {
            return Err("level".into());
        }
        Ok(())
    }

    /// Clear transient per-session input state after restoring or resuming.
    pub fn clear_transient(&mut self) {
        self.soft_drop = false;
        self.events.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty(mode: Mode) -> Game {
        Game::new(mode, Rules::default(), 1)
    }

    fn set_active(g: &mut Game, kind: Kind, rot: u8, x: i32, y: i32) {
        g.active = Piece { kind, rot, x, y };
        g.lowest_y = y;
        g.lock_resets = 0;
        g.lock_acc_us = 0;
    }

    fn fill(g: &mut Game, cells: &[(i32, i32)]) {
        for &(x, y) in cells {
            g.board[(y * COLS + x) as usize] = 8 - 1;
        }
    }

    fn fill_row_except(g: &mut Game, y: i32, holes: &[i32]) {
        for x in 0..COLS {
            if !holes.contains(&x) {
                g.board[(y * COLS + x) as usize] = 1;
            }
        }
    }

    fn sliding(mode: Mode) -> Game {
        let rules = Rules {
            drop_slide_us: 250_000,
            ..Rules::default()
        };
        Game::new(mode, rules, 1)
    }

    #[test]
    fn hard_drop_slide_allows_a_late_move_then_locks() {
        let mut g = sliding(Mode::Marathon);
        set_active(&mut g, Kind::O, 0, 4, 20);
        g.apply(Action::HardDrop);
        assert!(g.is_sliding());
        assert_eq!(g.pieces_placed, 0, "not locked yet");
        let y = g.active.y;
        g.tick(100_000);
        assert!(g.apply(Action::MoveRight));
        assert_eq!((g.active.x, g.active.y), (5, y), "slides along the floor");
        g.tick(100_000);
        assert!(g.is_sliding(), "a move does not extend the slide");
        g.tick(60_000);
        assert!(!g.is_sliding());
        assert_eq!(g.pieces_placed, 1);
        assert_eq!(g.cell(5, ROWS - 1), Kind::O.cell());
    }

    #[test]
    fn sliding_off_a_ledge_drops_again_and_a_second_drop_locks_at_once() {
        let mut g = sliding(Mode::Marathon);
        // A two-cell ledge under columns 3-4; the floor is free from column 5 on.
        fill(
            &mut g,
            &[(3, ROWS - 1), (4, ROWS - 1), (3, ROWS - 2), (4, ROWS - 2)],
        );
        set_active(&mut g, Kind::O, 0, 3, 20);
        g.apply(Action::HardDrop);
        let on_ledge = g.active.y;
        g.apply(Action::MoveRight);
        g.apply(Action::MoveRight);
        assert!(g.active.y > on_ledge, "fell off the ledge");
        assert!(g.is_sliding());
        assert!(!g.apply(Action::Hold), "no hold while sliding");
        g.apply(Action::HardDrop);
        assert!(!g.is_sliding());
        assert_eq!(g.pieces_placed, 1);
    }

    #[test]
    fn holding_the_slide_delays_the_lock_but_never_past_the_full_time() {
        let mut g = sliding(Mode::Marathon);
        set_active(&mut g, Kind::O, 0, 4, 20);
        g.apply(Action::HardDrop);
        g.tick(200_000);
        g.hold_slide(100_000);
        g.tick(100_000);
        assert!(g.is_sliding(), "held");
        g.hold_slide(1_000_000);
        assert_eq!(g.slide_left_us, 250_000, "capped at the full slide time");
        g.tick(250_000);
        assert_eq!(g.pieces_placed, 1);
    }

    #[test]
    fn old_snapshots_without_slide_fields_load() {
        let g = empty(Mode::Marathon);
        let mut v = serde_json::to_value(&g).unwrap();
        v.as_object_mut().unwrap().remove("slide_left_us");
        v["rules"].as_object_mut().unwrap().remove("drop_slide_us");
        let back: Game = serde_json::from_value(v).unwrap();
        assert_eq!(back.rules.drop_slide_us, 0);
        assert!(!back.is_sliding());
    }

    #[test]
    fn spawn_position_and_initial_drop() {
        let g = empty(Mode::Marathon);
        // Spawned at y=18 and dropped one row: lowest cells reach the first visible row.
        let max_y = g.active.cells().map(|c| c.1).max().unwrap();
        assert_eq!(max_y, HIDDEN_ROWS);
        let (lo, hi) = g.active.column_span();
        assert!(lo >= 3 && hi <= 6);
    }

    #[test]
    fn walls_block_movement() {
        let mut g = empty(Mode::Marathon);
        set_active(&mut g, Kind::O, 0, 4, 30);
        for _ in 0..10 {
            g.apply(Action::MoveLeft);
        }
        assert_eq!(g.active.column_span().0, 0);
        assert!(g.drain_events().contains(&Event::Blocked));
        for _ in 0..12 {
            g.apply(Action::MoveRight);
        }
        assert_eq!(g.active.column_span().1, COLS - 1);
    }

    #[test]
    fn i_piece_wall_kick_from_left_wall() {
        let mut g = empty(Mode::Marathon);
        // Vertical I (state L occupies box column 1) flush against the left wall.
        set_active(&mut g, Kind::I, 3, -1, 30);
        assert!(g.fits(&g.active));
        assert!(g.apply(Action::RotateCw)); // L -> 0 needs a kick (+1,0) is blocked? uses table
        assert!(g.fits(&g.active));
        assert_eq!(g.active.rot, 0);
        assert!(g.active.column_span().0 >= 0);
    }

    #[test]
    fn t_floor_kick_uses_srs_order() {
        let mut g = empty(Mode::Marathon);
        // T pointing up on the floor rotating to R: (0,0) fits on an empty floor.
        set_active(&mut g, Kind::T, 0, 4, ROWS - 2);
        assert!(g.apply(Action::RotateCw));
        // State R would poke through the floor and kick (-1,0) too; SRS kick 2 (-1,+1 up) lifts it.
        assert_eq!(g.last_kick, 2);
        assert!(g.fits(&g.active));
    }

    #[test]
    fn rotation_blocked_reports_blocked() {
        let mut g = empty(Mode::Marathon);
        // Box an O-shaped hole tightly around a vertical I so no kick fits.
        set_active(&mut g, Kind::I, 1, 2, 30); // occupies column 4, rows 30..34
        for y in 26..ROWS {
            for x in 0..COLS {
                if x != 4 {
                    g.board[(y * COLS + x) as usize] = 1;
                }
            }
        }
        for y in 34..ROWS {
            g.board[(y * COLS + 4) as usize] = 1;
        }
        assert!(g.fits(&g.active));
        assert!(!g.apply(Action::RotateCw));
        assert!(!g.apply(Action::RotateCcw));
    }

    #[test]
    fn hard_drop_locks_immediately_and_scores() {
        let mut g = empty(Mode::Marathon);
        set_active(&mut g, Kind::O, 0, 4, 20);
        g.apply(Action::HardDrop);
        assert_eq!(g.cell(4, ROWS - 1), Kind::O.cell());
        assert_eq!(g.score, 2 * (ROWS - 2 - 20) as u64);
        assert_eq!(g.pieces_placed, 1);
        let ev = g.drain_events();
        assert!(ev.iter().any(|e| matches!(e, Event::HardDrop { .. })));
        assert!(ev.iter().any(|e| matches!(e, Event::Spawned)));
    }

    #[test]
    fn single_double_triple_tetris_clear() {
        for n in 1..=4 {
            let mut g = empty(Mode::Marathon);
            for i in 0..n {
                fill_row_except(&mut g, ROWS - 1 - i, &[0]);
            }
            set_active(&mut g, Kind::I, 1, -2, 20); // vertical I in column 0
            g.apply(Action::HardDrop);
            assert_eq!(g.lines, n as u32);
            // Four lines empties the board here, so the perfect-clear bonus applies.
            let expected = [100, 300, 500, 800 + 2000][n as usize - 1];
            let drop_rows = (ROWS - 4 - 20) as u64;
            assert_eq!(g.score, expected + drop_rows * 2, "n={n}");
            // Remaining stack: the 4-n leftover I cells.
            assert_eq!(g.stack_height(), 4 - n);
        }
    }

    #[test]
    fn lock_delay_and_bounded_resets() {
        let mut g = empty(Mode::Marathon);
        set_active(&mut g, Kind::O, 0, 4, ROWS - 2);
        // 400 ms on ground: not yet locked.
        g.tick(400_000);
        assert_eq!(g.pieces_placed, 0);
        // A move resets the timer.
        g.apply(Action::MoveLeft);
        g.tick(400_000);
        assert_eq!(g.pieces_placed, 0);
        g.tick(150_000);
        assert_eq!(g.pieces_placed, 1);

        // Resets are bounded: wiggling forever must still lock.
        let mut g = empty(Mode::Marathon);
        set_active(&mut g, Kind::O, 0, 4, ROWS - 2);
        let mut t = 0;
        while g.pieces_placed == 0 && t < 100 {
            g.tick(100_000);
            g.apply(if t % 2 == 0 {
                Action::MoveLeft
            } else {
                Action::MoveRight
            });
            t += 1;
        }
        assert_eq!(g.pieces_placed, 1);
        assert!(t <= 15 + 6, "locked after {t} wiggles");
    }

    #[test]
    fn reaching_new_lowest_row_restores_resets() {
        let mut g = empty(Mode::Marathon);
        fill(&mut g, &[(5, 30), (6, 30)]);
        set_active(&mut g, Kind::O, 0, 4, 28);
        g.lock_resets = 15;
        g.apply(Action::MoveLeft); // now over empty column, will fall
        g.tick(row_time_us(1) as u64);
        assert!(g.active.y > 28);
        assert_eq!(g.lock_resets, 0);
    }

    #[test]
    fn gravity_follows_level_table() {
        let mut g = empty(Mode::Marathon);
        let y0 = g.active.y;
        g.tick(999_999);
        assert_eq!(g.active.y, y0);
        g.tick(1);
        assert_eq!(g.active.y, y0 + 1);
    }

    #[test]
    fn soft_drop_is_faster_and_scores_per_row() {
        let mut g = empty(Mode::Marathon);
        let y0 = g.active.y;
        g.apply(Action::SoftDropStart);
        g.tick(500_000); // 20x gravity, capped at one row per 33.3 ms
        assert_eq!(g.active.y, y0 + 15);
        assert_eq!(g.score, 15);
    }

    #[test]
    fn hold_once_per_piece() {
        let mut g = empty(Mode::Marathon);
        let first = g.active.kind;
        let next = g.queue[0];
        assert!(g.apply(Action::Hold));
        assert_eq!(g.hold, Some(first));
        assert_eq!(g.active.kind, next);
        assert!(!g.apply(Action::Hold));
        g.apply(Action::HardDrop);
        assert!(g.apply(Action::Hold));
        assert_eq!(g.active.kind, first);
    }

    #[test]
    fn block_out_ends_game() {
        let mut g = empty(Mode::Marathon);
        for y in 18..20 {
            for x in 3..7 {
                g.board[(y * COLS + x) as usize] = 1;
            }
        }
        g.apply(Action::HardDrop);
        assert_eq!(g.status, Status::ToppedOut);
        assert!(!g.apply(Action::MoveLeft));
    }

    #[test]
    fn lock_out_when_piece_locks_above_field() {
        let mut g = empty(Mode::Marathon);
        // Fill the visible field below the spawn except for column 9, without full rows.
        for y in HIDDEN_ROWS..ROWS {
            fill_row_except(&mut g, y, &[9]);
        }
        set_active(&mut g, Kind::O, 0, 4, 18);
        g.apply(Action::HardDrop);
        assert_eq!(g.status, Status::ToppedOut);
    }

    #[test]
    fn practice_resets_instead_of_ending() {
        let mut g = empty(Mode::Practice);
        for y in 18..20 {
            for x in 3..7 {
                g.board[(y * COLS + x) as usize] = 1;
            }
        }
        g.apply(Action::HardDrop);
        assert_eq!(g.status, Status::Playing);
        assert!(g.drain_events().contains(&Event::PracticeReset));
    }

    #[test]
    fn practice_has_no_gravity() {
        let mut g = empty(Mode::Practice);
        let y0 = g.active.y;
        g.tick(10_000_000);
        assert_eq!(g.active.y, y0);
    }

    #[test]
    fn t_spin_double_is_detected() {
        let mut g = empty(Mode::Marathon);
        // Classic TSD slot at the bottom: rows 38 and 39 full except a T-shaped hole.
        //  row 37: X X X . . . . . . .   (overhang at x=3)
        //  row 38: X X X . X X X X X X   hole at x=3
        //  row 39: X X X X . X X X X X   hole at x=4? (T pointing down fills (3,38),(4,38),(5,38),(4,39))
        for x in 0..COLS {
            if ![3, 4, 5].contains(&x) {
                g.board[(38 * COLS + x) as usize] = 1;
            }
            if x != 4 {
                g.board[(39 * COLS + x) as usize] = 1;
            }
        }
        g.board[(37 * COLS + 3) as usize] = 1; // overhang
        g.board[(37 * COLS + 5) as usize] = 0;
        // T pointing right, about to rotate CW into pointing-down in the slot.
        set_active(&mut g, Kind::T, 1, 3, 37);
        assert!(g.fits(&g.active));
        assert!(g.apply(Action::RotateCw));
        assert_eq!(g.active.rot, 2);
        g.drain_events();
        g.apply(Action::HardDrop);
        assert_eq!(g.lines, 2);
        let cleared = g
            .drain_events()
            .into_iter()
            .find_map(|e| match e {
                Event::Cleared { score, .. } => Some(score),
                _ => None,
            })
            .unwrap();
        assert_eq!(cleared.spin, Spin::Full);
        assert_eq!(cleared.points, 1200);
    }

    #[test]
    fn sprint_finishes_at_forty_lines() {
        let mut g = empty(Mode::Sprint);
        g.lines = 39;
        fill_row_except(&mut g, ROWS - 1, &[0]);
        set_active(&mut g, Kind::I, 1, -2, 20);
        g.apply(Action::HardDrop);
        assert_eq!(g.status, Status::Finished);
    }

    #[test]
    fn marathon_levels_up_every_ten_lines() {
        let mut g = empty(Mode::Marathon);
        g.lines = 9;
        fill_row_except(&mut g, ROWS - 1, &[0]);
        set_active(&mut g, Kind::I, 1, -2, 20);
        g.apply(Action::HardDrop);
        assert_eq!(g.level, 2);
        assert!(g.drain_events().contains(&Event::LevelUp(2)));
    }

    #[test]
    fn deterministic_replay() {
        let script = [
            Action::MoveLeft,
            Action::RotateCw,
            Action::HardDrop,
            Action::Hold,
            Action::MoveRight,
            Action::MoveRight,
            Action::HardDrop,
            Action::RotateCcw,
            Action::HardDrop,
        ];
        let run = || {
            let mut g = Game::new(Mode::Marathon, Rules::default(), 4242);
            for (i, a) in script.iter().cycle().take(300).enumerate() {
                g.apply(*a);
                g.tick(16_667 + (i as u64 % 3));
            }
            g
        };
        let a = run();
        let b = run();
        assert_eq!(a, b);
    }

    #[test]
    fn save_restore_round_trip_is_identical() {
        let mut g = Game::new(Mode::Marathon, Rules::default(), 77);
        for a in [Action::MoveLeft, Action::HardDrop, Action::Hold, Action::RotateCw] {
            g.apply(a);
            g.tick(100_000);
        }
        let json = serde_json::to_string(&g).unwrap();
        let mut r: Game = serde_json::from_str(&json).unwrap();
        r.validate().unwrap();
        // Continue both identically.
        for a in [Action::MoveRight, Action::HardDrop, Action::HardDrop] {
            g.apply(a);
            r.apply(a);
            g.tick(50_000);
            r.tick(50_000);
        }
        g.drain_events();
        r.drain_events();
        assert_eq!(g, r);
    }

    #[test]
    fn validate_rejects_corrupt_state() {
        let mut g = Game::new(Mode::Marathon, Rules::default(), 77);
        g.board.truncate(10);
        assert!(g.validate().is_err());
        let mut g = Game::new(Mode::Marathon, Rules::default(), 77);
        g.active.x = -5;
        assert!(g.validate().is_err());
    }
}
