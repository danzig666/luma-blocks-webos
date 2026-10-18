//! Remote-button gameplay mapping: delayed auto-shift and repeat for left/right, held soft
//! drop, single-shot actions, colour shortcuts, short/long press separation, centre-click
//! de-duplication and protection against lost key-up events.
//!
//! The platform's own key repeat is never used for gameplay timing (its rate is the TV's,
//! not the player's); repeats only serve as a liveness signal for held keys.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum Button {
    Up,
    Down,
    Left,
    Right,
    Ok,
    Back,
    Red,
    Green,
    Yellow,
    Blue,
    ChUp,
    ChDown,
}

impl Button {
    pub fn name(self) -> &'static str {
        match self {
            Button::Up => "Up",
            Button::Down => "Down",
            Button::Left => "Left",
            Button::Right => "Right",
            Button::Ok => "OK",
            Button::Back => "Back",
            Button::Red => "Red",
            Button::Green => "Green",
            Button::Yellow => "Yellow",
            Button::Blue => "Blue",
            Button::ChUp => "CH+",
            Button::ChDown => "CH-",
        }
    }
    pub fn is_colour(self) -> bool {
        matches!(self, Button::Red | Button::Green | Button::Yellow | Button::Blue)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Shortcut {
    #[default]
    None,
    RotateCcw,
    RotateCw,
    Hold,
    Recenter,
    Pause,
}

impl Shortcut {
    pub const ALL: [Shortcut; 6] = [
        Shortcut::None,
        Shortcut::RotateCcw,
        Shortcut::RotateCw,
        Shortcut::Hold,
        Shortcut::Recenter,
        Shortcut::Pause,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Shortcut::None => "Nothing",
            Shortcut::RotateCcw => "Rotate anticlockwise",
            Shortcut::RotateCw => "Rotate clockwise",
            Shortcut::Hold => "Hold piece",
            Shortcut::Recenter => "Recenter",
            Shortcut::Pause => "Pause",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bindings {
    pub red: Shortcut,
    pub green: Shortcut,
    pub yellow: Shortcut,
    pub blue: Shortcut,
    /// Wheel steps rotate the piece (optional, never required).
    pub wheel_rotates: bool,
}

impl Default for Bindings {
    fn default() -> Self {
        Bindings {
            red: Shortcut::RotateCcw,
            green: Shortcut::Hold,
            yellow: Shortcut::None,
            blue: Shortcut::Recenter,
            wheel_rotates: true,
        }
    }
}

impl Bindings {
    pub fn for_button(&self, b: Button) -> Shortcut {
        match b {
            Button::Red => self.red,
            Button::Green => self.green,
            Button::Yellow => self.yellow,
            Button::Blue => self.blue,
            _ => Shortcut::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RepeatTiming {
    /// Delay before auto-shift starts, ms.
    pub das_ms: u32,
    /// Interval between auto-shift moves, ms.
    pub arr_ms: u32,
}

impl Default for RepeatTiming {
    fn default() -> Self {
        RepeatTiming {
            das_ms: 170,
            arr_ms: 50,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    Move(i32),
    SoftDrop(bool),
    RotateCw,
    RotateCcw,
    HardDrop,
    Hold,
    Pause,
    Recenter,
    /// A long press of a colour button (separate from its short action).
    Calibrate,
}

impl Cmd {
    pub fn from_shortcut(s: Shortcut) -> Option<Cmd> {
        Some(match s {
            Shortcut::None => return None,
            Shortcut::RotateCcw => Cmd::RotateCcw,
            Shortcut::RotateCw => Cmd::RotateCw,
            Shortcut::Hold => Cmd::Hold,
            Shortcut::Recenter => Cmd::Recenter,
            Shortcut::Pause => Cmd::Pause,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct Held {
    since_us: u64,
    next_us: u64,
    last_platform_us: u64,
    saw_repeat: bool,
}

/// Long-press threshold for buttons that have a long action, ms.
pub const LONG_PRESS_MS: u64 = 600;
/// A key whose platform repeats stop for this long is treated as released, ms.
pub const LOST_KEYUP_MS: u64 = 500;
/// OK key and pointer click within this window are one press, ms.
pub const CLICK_DEDUP_MS: u64 = 90;

#[derive(Debug, Clone, Default)]
pub struct ButtonMapper {
    pub timing: RepeatTiming,
    pub bindings: Bindings,
    left: Option<Held>,
    right: Option<Held>,
    down: Option<Held>,
    /// Last horizontal direction pressed wins when both are held.
    last_dir: i32,
    last_ok_us: Option<u64>,
    long_pending: Option<(Button, u64, bool)>,
    soft_flag: bool,
}

impl ButtonMapper {
    pub fn new(timing: RepeatTiming, bindings: Bindings) -> Self {
        ButtonMapper {
            timing,
            bindings,
            ..Default::default()
        }
    }

    /// Whether a button has a separate long-press action (its short action then fires on
    /// release, so a long press never runs both).
    fn has_long(&self, b: Button) -> bool {
        b == Button::Blue && self.bindings.blue == Shortcut::Recenter
    }

    pub fn is_horizontal_held(&self) -> bool {
        self.left.is_some() || self.right.is_some()
    }

    pub fn event(&mut self, t: u64, b: Button, down: bool, repeat: bool) -> Vec<Cmd> {
        let mut out = Vec::new();
        let das = self.timing.das_ms as u64 * 1000;
        let held = |slot: &mut Option<Held>| {
            if let Some(h) = slot.as_mut() {
                h.last_platform_us = t;
                h.saw_repeat |= repeat;
            }
        };
        match (b, down) {
            (Button::Left | Button::Right, true) => {
                let slot = if b == Button::Left {
                    &mut self.left
                } else {
                    &mut self.right
                };
                if slot.is_some() {
                    held(slot);
                    return out;
                }
                *slot = Some(Held {
                    since_us: t,
                    next_us: t + das,
                    last_platform_us: t,
                    saw_repeat: false,
                });
                let d = if b == Button::Left { -1 } else { 1 };
                self.last_dir = d;
                out.push(Cmd::Move(d));
            }
            (Button::Left, false) => self.left = None,
            (Button::Right, false) => self.right = None,
            (Button::Down, true) => {
                if self.down.is_some() {
                    held(&mut self.down);
                } else {
                    self.down = Some(Held {
                        since_us: t,
                        next_us: 0,
                        last_platform_us: t,
                        saw_repeat: false,
                    });
                    out.push(Cmd::SoftDrop(true));
                }
            }
            (Button::Down, false) => {
                self.soft_flag = false;
                if self.down.take().is_some() {
                    out.push(Cmd::SoftDrop(false));
                }
            }
            (_, true) if repeat => {}
            (Button::Up, true) => out.push(Cmd::RotateCw),
            (Button::Ok, true) => {
                if !self.recent_ok(t) {
                    out.push(Cmd::HardDrop);
                }
                self.last_ok_us = Some(t);
            }
            (Button::Back, true) => out.push(Cmd::Pause),
            (c, true) if c.is_colour() => {
                if self.has_long(c) {
                    self.long_pending = Some((c, t, false));
                } else if let Some(cmd) = Cmd::from_shortcut(self.bindings.for_button(c)) {
                    out.push(cmd);
                }
            }
            (c, false) if c.is_colour() => {
                if let Some((pb, since, fired)) = self.long_pending {
                    if pb == c {
                        self.long_pending = None;
                        if !fired && t.saturating_sub(since) < LONG_PRESS_MS * 1000 {
                            if let Some(cmd) = Cmd::from_shortcut(self.bindings.for_button(c)) {
                                out.push(cmd);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        out
    }

    fn recent_ok(&self, t: u64) -> bool {
        self.last_ok_us
            .is_some_and(|l| t.saturating_sub(l) < CLICK_DEDUP_MS * 1000)
    }

    /// The pointer's click (the Magic Remote's centre button with the cursor visible).
    /// Merged with a key OK arriving for the same press.
    pub fn click(&mut self, t: u64) -> Vec<Cmd> {
        let dup = self.recent_ok(t);
        self.last_ok_us = Some(t);
        if dup {
            Vec::new()
        } else {
            vec![Cmd::HardDrop]
        }
    }

    pub fn tick(&mut self, t: u64) -> Vec<Cmd> {
        let mut out = Vec::new();
        let lost = LOST_KEYUP_MS * 1000;
        for slot in [&mut self.left, &mut self.right, &mut self.down] {
            if let Some(h) = slot {
                if h.saw_repeat && t.saturating_sub(h.last_platform_us) > lost {
                    *slot = None;
                }
            }
        }
        if self.soft_drop_was_on() {
            out.push(Cmd::SoftDrop(false));
        }
        let arr = (self.timing.arr_ms.max(1) as u64) * 1000;
        let dir = match (self.left.is_some(), self.right.is_some()) {
            (true, true) => self.last_dir,
            (true, false) => -1,
            (false, true) => 1,
            _ => 0,
        };
        if dir != 0 {
            let slot = if dir < 0 { &mut self.left } else { &mut self.right };
            if let Some(h) = slot.as_mut() {
                let mut n = 0;
                while t >= h.next_us && n < 10 {
                    out.push(Cmd::Move(dir));
                    h.next_us += arr;
                    n += 1;
                }
                if n == 10 {
                    h.next_us = t + arr;
                }
            }
        }
        if let Some((b, since, fired)) = self.long_pending {
            if !fired && t.saturating_sub(since) >= LONG_PRESS_MS * 1000 {
                self.long_pending = Some((b, since, true));
                out.push(Cmd::Calibrate);
            }
        }
        out
    }

    fn soft_drop_was_on(&mut self) -> bool {
        // Tracks the soft-drop state reported to the game so a lost key-up is reported once.
        let was = self.soft_flag;
        self.soft_flag = self.down.is_some();
        was && !self.soft_flag
    }

    /// Release everything (focus loss, pause): returns the commands that undo held state.
    pub fn release_all(&mut self) -> Vec<Cmd> {
        let mut out = Vec::new();
        if self.down.is_some() {
            out.push(Cmd::SoftDrop(false));
        }
        self.left = None;
        self.right = None;
        self.down = None;
        self.soft_flag = false;
        self.long_pending = None;
        out
    }

    pub fn any_held(&self) -> bool {
        self.left.is_some() || self.right.is_some() || self.down.is_some() || self.long_pending.is_some()
    }

    pub fn held_since(&self, b: Button) -> Option<u64> {
        match b {
            Button::Left => self.left.map(|h| h.since_us),
            Button::Right => self.right.map(|h| h.since_us),
            Button::Down => self.down.map(|h| h.since_us),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(v: u64) -> u64 {
        v * 1000
    }

    #[test]
    fn das_and_arr() {
        let mut m = ButtonMapper::default();
        assert_eq!(m.event(0, Button::Left, true, false), vec![Cmd::Move(-1)]);
        assert!(m.tick(ms(100)).is_empty());
        assert_eq!(m.tick(ms(170)), vec![Cmd::Move(-1)]);
        assert_eq!(m.tick(ms(220)), vec![Cmd::Move(-1)]);
        assert!(m.tick(ms(240)).is_empty());
        m.event(ms(250), Button::Left, false, false);
        assert!(m.tick(ms(400)).is_empty());
    }

    #[test]
    fn platform_repeats_do_not_drive_movement() {
        let mut m = ButtonMapper::default();
        m.event(0, Button::Right, true, false);
        assert!(m.event(ms(30), Button::Right, true, true).is_empty());
        assert!(m.event(ms(60), Button::Up, true, true).is_empty());
    }

    #[test]
    fn soft_drop_hold_and_lost_keyup() {
        let mut m = ButtonMapper::default();
        assert_eq!(m.event(0, Button::Down, true, false), vec![Cmd::SoftDrop(true)]);
        m.tick(ms(10));
        m.event(ms(100), Button::Down, true, true);
        assert!(m.tick(ms(400)).is_empty());
        // Repeats stop and the key-up never arrives: released after the timeout.
        assert_eq!(m.tick(ms(700)), vec![Cmd::SoftDrop(false)]);
        assert!(m.tick(ms(800)).is_empty());
    }

    #[test]
    fn without_repeats_a_held_key_is_not_released() {
        let mut m = ButtonMapper::default();
        m.event(0, Button::Down, true, false);
        m.tick(ms(5));
        assert!(m.tick(ms(5000)).is_empty());
        assert!(m.any_held());
    }

    #[test]
    fn ok_and_click_are_one_press() {
        let mut m = ButtonMapper::default();
        assert_eq!(m.event(0, Button::Ok, true, false), vec![Cmd::HardDrop]);
        assert!(m.click(ms(20)).is_empty());
        assert_eq!(m.click(ms(500)), vec![Cmd::HardDrop]);
        assert!(m.event(ms(530), Button::Ok, true, false).is_empty());
        assert_eq!(m.event(ms(900), Button::Ok, true, false), vec![Cmd::HardDrop]);
    }

    #[test]
    fn long_press_never_runs_both_actions() {
        let mut m = ButtonMapper::default();
        assert!(m.event(0, Button::Blue, true, false).is_empty());
        assert!(m.tick(ms(300)).is_empty());
        assert_eq!(m.tick(ms(650)), vec![Cmd::Calibrate]);
        assert!(m.event(ms(900), Button::Blue, false, false).is_empty());
        // Short press runs the short action on release.
        m.event(ms(1000), Button::Blue, true, false);
        assert_eq!(m.event(ms(1200), Button::Blue, false, false), vec![Cmd::Recenter]);
    }

    #[test]
    fn colour_shortcuts_and_remap() {
        let mut m = ButtonMapper::default();
        assert_eq!(m.event(0, Button::Red, true, false), vec![Cmd::RotateCcw]);
        assert_eq!(m.event(0, Button::Green, true, false), vec![Cmd::Hold]);
        assert!(m.event(0, Button::Yellow, true, false).is_empty());
        m.bindings.yellow = Shortcut::Pause;
        assert_eq!(m.event(0, Button::Yellow, true, false), vec![Cmd::Pause]);
    }

    #[test]
    fn release_all_clears_stuck_keys() {
        let mut m = ButtonMapper::default();
        m.event(0, Button::Down, true, false);
        m.event(0, Button::Left, true, false);
        assert_eq!(m.release_all(), vec![Cmd::SoftDrop(false)]);
        assert!(!m.any_held());
        assert!(m.tick(ms(1000)).is_empty());
    }
}
