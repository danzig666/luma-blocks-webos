//! Tetromino shapes, rotation states and SRS wall-kick tables.
//!
//! Coordinates are `(x, y)` with **x to the right and y downward**, matching the board
//! (row 0 is the top of the hidden buffer). Each piece lives in a square bounding box
//! (2x2 for O, 3x3 for J L S T Z, 4x4 for I); its position is the box's top-left cell.
//! The four rotation states are the box rotated by 90 degrees, which is exactly the SRS
//! "true rotation" definition. Kick offsets below are the published SRS tables converted
//! from their y-up convention (dy negated).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum Kind {
    I,
    O,
    T,
    S,
    Z,
    J,
    L,
}

pub const ALL_KINDS: [Kind; 7] = [Kind::I, Kind::O, Kind::T, Kind::S, Kind::Z, Kind::J, Kind::L];

impl Kind {
    /// Board cell value for a settled block of this kind (0 means empty).
    pub fn cell(self) -> u8 {
        self as u8 + 1
    }
    pub fn from_cell(c: u8) -> Option<Kind> {
        ALL_KINDS.get((c as usize).checked_sub(1)?).copied()
    }
    pub fn box_size(self) -> i32 {
        match self {
            Kind::I => 4,
            Kind::O => 2,
            _ => 3,
        }
    }
    /// Cells of rotation state 0 inside the bounding box.
    fn spawn_cells(self) -> [(i32, i32); 4] {
        match self {
            Kind::I => [(0, 1), (1, 1), (2, 1), (3, 1)],
            Kind::O => [(0, 0), (1, 0), (0, 1), (1, 1)],
            Kind::T => [(1, 0), (0, 1), (1, 1), (2, 1)],
            Kind::S => [(1, 0), (2, 0), (0, 1), (1, 1)],
            Kind::Z => [(0, 0), (1, 0), (1, 1), (2, 1)],
            Kind::J => [(0, 0), (0, 1), (1, 1), (2, 1)],
            Kind::L => [(2, 0), (0, 1), (1, 1), (2, 1)],
        }
    }
    /// Cells for rotation state `rot` (0 = spawn, 1 = R, 2 = 180, 3 = L).
    pub fn cells(self, rot: u8) -> [(i32, i32); 4] {
        let n = self.box_size();
        let mut c = self.spawn_cells();
        for _ in 0..(rot % 4) {
            for p in c.iter_mut() {
                // Clockwise rotation of the box (y down): (x, y) -> (n-1-y, x).
                *p = (n - 1 - p.1, p.0);
            }
        }
        c
    }
    pub fn name(self) -> &'static str {
        match self {
            Kind::I => "I",
            Kind::O => "O",
            Kind::T => "T",
            Kind::S => "S",
            Kind::Z => "Z",
            Kind::J => "J",
            Kind::L => "L",
        }
    }
}

/// SRS kicks for J L S T Z in y-up notation, indexed by (from, to) transition.
const JLSTZ_UP: [[(i32, i32); 5]; 8] = [
    [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)], // 0->R
    [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],     // R->0
    [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],     // R->2
    [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)], // 2->R
    [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],    // 2->L
    [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)],  // L->2
    [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)],  // L->0
    [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],    // 0->L
];

/// SRS kicks for I (the "I-piece exceptions"), y-up notation, same transition order.
const I_UP: [[(i32, i32); 5]; 8] = [
    [(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)], // 0->R
    [(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)], // R->0
    [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)], // R->2
    [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)], // 2->R
    [(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)], // 2->L
    [(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)], // L->2
    [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)], // L->0
    [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)], // 0->L
];

fn transition_index(from: u8, to: u8) -> usize {
    match (from % 4, to % 4) {
        (0, 1) => 0,
        (1, 0) => 1,
        (1, 2) => 2,
        (2, 1) => 3,
        (2, 3) => 4,
        (3, 2) => 5,
        (3, 0) => 6,
        (0, 3) => 7,
        _ => unreachable!("only 90-degree rotations are kicked"),
    }
}

/// Kick offsets to try, in order, converted to the board's y-down convention.
pub fn kicks(kind: Kind, from: u8, to: u8) -> [(i32, i32); 5] {
    if kind == Kind::O {
        return [(0, 0); 5];
    }
    let table = if kind == Kind::I { &I_UP } else { &JLSTZ_UP };
    let mut k = table[transition_index(from, to)];
    for p in k.iter_mut() {
        p.1 = -p.1;
    }
    k
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Piece {
    pub kind: Kind,
    pub rot: u8,
    pub x: i32,
    pub y: i32,
}

impl Piece {
    pub fn cells(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.kind
            .cells(self.rot)
            .into_iter()
            .map(move |(cx, cy)| (self.x + cx, self.y + cy))
    }
    /// Inclusive column span occupied by the piece.
    pub fn column_span(&self) -> (i32, i32) {
        let mut lo = i32::MAX;
        let mut hi = i32::MIN;
        for (x, _) in self.cells() {
            lo = lo.min(x);
            hi = hi.max(x);
        }
        (lo, hi)
    }
    /// Horizontal centre of the occupied cells in column units (a 3-wide piece spanning
    /// columns 3..=5 has centre 4.5; the pointing anchor convention uses this).
    pub fn centre_x(&self) -> f32 {
        let (lo, hi) = self.column_span();
        (lo + hi + 1) as f32 / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_states_match_srs_reference() {
        // T: 0 = pointing up, R = pointing right, 2 = down, L = left.
        let mut r = Kind::T.cells(1);
        r.sort();
        assert_eq!(r, [(1, 0), (1, 1), (1, 2), (2, 1)]);
        let mut d = Kind::T.cells(2);
        d.sort();
        assert_eq!(d, [(0, 1), (1, 1), (1, 2), (2, 1)]);
        // I vertical in state R occupies box column 2.
        assert!(Kind::I.cells(1).iter().all(|c| c.0 == 2));
        // I state 2 is the horizontal bar in box row 2, L is box column 1.
        assert!(Kind::I.cells(2).iter().all(|c| c.1 == 2));
        assert!(Kind::I.cells(3).iter().all(|c| c.0 == 1));
        // O does not move when rotated.
        let mut o = Kind::O.cells(3);
        o.sort();
        assert_eq!(o, [(0, 0), (0, 1), (1, 0), (1, 1)]);
    }

    #[test]
    fn four_rotations_return_to_spawn() {
        for k in ALL_KINDS {
            assert_eq!(k.cells(0), k.cells(4));
        }
    }

    #[test]
    fn kick_tables_are_inverse_pairs() {
        // Kicks for A->B are the negation of B->A, a property of the SRS offset tables.
        for k in [Kind::T, Kind::I] {
            for (a, b) in [(0u8, 1u8), (1, 2), (2, 3), (3, 0)] {
                let f = kicks(k, a, b);
                let r = kicks(k, b, a);
                for i in 0..5 {
                    assert_eq!(f[i], (-r[i].0, -r[i].1), "{k:?} {a}->{b} kick {i}");
                }
            }
        }
    }
}
