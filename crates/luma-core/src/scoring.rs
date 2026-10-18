//! The scoring model, all in one place.
//!
//! A documented modern model in the style of recent guideline games. It is the application's
//! own table: no claim of formal guideline certification is made.
//!
//! | Clear                     | Points (x level)  | Difficult (back-to-back eligible) |
//! |---------------------------|-------------------|-----------------------------------|
//! | Single / Double / Triple  | 100 / 300 / 500   | no                                |
//! | Four lines ("Tetris")     | 800               | yes                               |
//! | T-spin mini, no lines     | 100               | no (does not break a chain)       |
//! | T-spin mini single/double | 200 / 400         | yes                               |
//! | T-spin, no lines          | 400               | no (does not break a chain)       |
//! | T-spin single/double/triple | 800 / 1200 / 1600 | yes                             |
//!
//! * Back-to-back: a difficult clear directly following another difficult clear (non-clearing
//!   locks in between do not break the chain; an ordinary single/double/triple does) earns
//!   x1.5 on the clear's base points.
//! * Combo: consecutive locks that each clear at least one line. The n-th consecutive clear
//!   after the first adds 50 x n x level.
//! * Perfect clear (board empty after the clear) adds 800 / 1200 / 1800 / 2000 x level for
//!   1 / 2 / 3 / 4 lines, or 3200 x level for a back-to-back four-line perfect clear.
//! * Drop points: 1 per row of soft drop, 2 per row of hard drop (not multiplied by level).
//!
//! T-spin classification (applied at lock time, only for a T whose last successful action
//! was a rotation):
//! * Three-corner rule: at least three of the four cells diagonally adjacent to the T's centre
//!   are occupied (walls and floor count as occupied).
//! * Full T-spin if both corners on the side the T points to are occupied; otherwise mini,
//!   unless the rotation used the fifth (last) kick offset, which upgrades a mini to full.

use serde::{Deserialize, Serialize};

pub const SOFT_DROP_POINTS_PER_ROW: u64 = 1;
pub const HARD_DROP_POINTS_PER_ROW: u64 = 2;
pub const COMBO_POINTS: u64 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Spin {
    None,
    Mini,
    Full,
}

/// Base points for a clear, before level, back-to-back and combo.
pub fn base_points(lines: u32, spin: Spin) -> u64 {
    match (spin, lines) {
        (Spin::None, 0) => 0,
        (Spin::None, 1) => 100,
        (Spin::None, 2) => 300,
        (Spin::None, 3) => 500,
        (Spin::None, _) => 800,
        (Spin::Mini, 0) => 100,
        (Spin::Mini, 1) => 200,
        (Spin::Mini, _) => 400,
        (Spin::Full, 0) => 400,
        (Spin::Full, 1) => 800,
        (Spin::Full, 2) => 1200,
        (Spin::Full, _) => 1600,
    }
}

/// Whether a clear counts toward a back-to-back chain.
pub fn is_difficult(lines: u32, spin: Spin) -> bool {
    lines >= 4 || (lines > 0 && spin != Spin::None)
}

pub fn perfect_clear_points(lines: u32, b2b_four: bool) -> u64 {
    match lines {
        0 => 0,
        1 => 800,
        2 => 1200,
        3 => 1800,
        _ if b2b_four => 3200,
        _ => 2000,
    }
}

/// Outcome of scoring one lock, for display and tests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClearScore {
    pub lines: u32,
    pub spin: Spin,
    pub back_to_back: bool,
    /// Consecutive-clear count (1 = the second clear in a row), 0 when none.
    pub combo: u32,
    pub perfect: bool,
    pub points: u64,
}

/// Running chain state carried between locks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Chain {
    /// Last line-clearing lock was difficult.
    pub b2b_active: bool,
    /// -1 when no combo; 0 after the first clear, 1 after the second consecutive clear...
    pub combo: i32,
}

impl Chain {
    pub fn new() -> Self {
        Chain {
            b2b_active: false,
            combo: -1,
        }
    }

    /// Score one lock and advance the chain.
    pub fn score_lock(&mut self, lines: u32, spin: Spin, perfect: bool, level: u32) -> ClearScore {
        let level = level.max(1) as u64;
        let difficult = is_difficult(lines, spin);
        let mut back_to_back = false;
        let mut points = base_points(lines, spin);
        if lines > 0 {
            if difficult && self.b2b_active {
                back_to_back = true;
                points = points * 3 / 2;
            }
            self.b2b_active = difficult;
            self.combo += 1;
        } else {
            self.combo = -1;
        }
        let mut total = points * level;
        let combo = self.combo.max(0) as u32;
        if lines > 0 && combo > 0 {
            total += COMBO_POINTS * combo as u64 * level;
        }
        if perfect && lines > 0 {
            total += perfect_clear_points(lines, back_to_back && lines >= 4) * level;
        }
        ClearScore {
            lines,
            spin,
            back_to_back,
            combo,
            perfect: perfect && lines > 0,
            points: total,
        }
    }
}

/// Short human label for a clear, used in the on-board highlight.
pub fn clear_label(s: &ClearScore) -> Option<String> {
    let base = match (s.spin, s.lines) {
        (Spin::None, 4) => "Four lines".to_owned(),
        (Spin::None, _) => return None,
        (Spin::Mini, 0) => "T-spin mini".to_owned(),
        (Spin::Mini, n) => format!("T-spin mini {}", lines_word(n)),
        (Spin::Full, 0) => "T-spin".to_owned(),
        (Spin::Full, n) => format!("T-spin {}", lines_word(n)),
    };
    Some(if s.back_to_back {
        format!("{base}  ·  back to back")
    } else {
        base
    })
}

fn lines_word(n: u32) -> &'static str {
    match n {
        1 => "single",
        2 => "double",
        _ => "triple",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_table() {
        let mut c = Chain::new();
        assert_eq!(c.score_lock(1, Spin::None, false, 1).points, 100);
        // second consecutive clear: combo 1 adds 50
        assert_eq!(c.score_lock(2, Spin::None, false, 1).points, 350);
        c.score_lock(0, Spin::None, false, 1);
        assert_eq!(c.score_lock(4, Spin::None, false, 3).points, 2400);
    }

    #[test]
    fn back_to_back_chain() {
        let mut c = Chain::new();
        c.score_lock(4, Spin::None, false, 1);
        c.score_lock(0, Spin::None, false, 1); // non-clearing lock keeps b2b
        let s = c.score_lock(2, Spin::Full, false, 1);
        assert!(s.back_to_back);
        assert_eq!(s.points, 1800);
        c.score_lock(1, Spin::None, false, 1); // ordinary single breaks it
        let s = c.score_lock(4, Spin::None, false, 1);
        assert!(!s.back_to_back);
    }

    #[test]
    fn spins_without_lines_keep_chain_and_reset_combo() {
        let mut c = Chain::new();
        c.score_lock(4, Spin::None, false, 1);
        let s = c.score_lock(0, Spin::Full, false, 2);
        assert_eq!(s.points, 800);
        assert!(c.b2b_active);
        assert_eq!(c.combo, -1);
    }

    #[test]
    fn perfect_clear_bonus() {
        let mut c = Chain::new();
        let s = c.score_lock(4, Spin::None, true, 1);
        assert_eq!(s.points, 800 + 2000);
        let s = c.score_lock(4, Spin::None, true, 1);
        // b2b 1200 + combo 50 + b2b perfect 3200
        assert_eq!(s.points, 1200 + 50 + 3200);
    }
}
