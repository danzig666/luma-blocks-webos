//! A large, TV-friendly vertical list used by every menu: arrows/OK/Back navigation plus
//! Magic Remote pointer hover and click, with a clearly visible focus.

use crate::gfx::{alpha, theme, Face, Gfx};
use luma_input::buttons::Button;
use sdl2::rect::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// OK activates.
    Action,
    /// Left/Right (and OK) change the value.
    Choice,
    /// Not focusable (a section label).
    Heading,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub label: String,
    pub value: Option<String>,
    pub kind: Kind,
    pub enabled: bool,
}

impl Item {
    pub fn action(label: impl Into<String>) -> Item {
        Item {
            label: label.into(),
            value: None,
            kind: Kind::Action,
            enabled: true,
        }
    }
    pub fn choice(label: impl Into<String>, value: impl Into<String>) -> Item {
        Item {
            label: label.into(),
            value: Some(value.into()),
            kind: Kind::Choice,
            enabled: true,
        }
    }
    pub fn heading(label: impl Into<String>) -> Item {
        Item {
            label: label.into(),
            value: None,
            kind: Kind::Heading,
            enabled: false,
        }
    }
    pub fn disabled(mut self, off: bool) -> Item {
        if off {
            self.enabled = false;
        }
        self
    }
    fn focusable(&self) -> bool {
        self.kind != Kind::Heading && self.enabled
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UiIn {
    Press(Button),
    Pointer(i32, i32),
    Click(i32, i32),
    Wheel(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ev {
    None,
    Moved,
    Activate(usize),
    Adjust(usize, i32),
    Back,
}

#[derive(Debug, Clone, Default)]
pub struct List {
    pub focus: usize,
    scroll: usize,
    rects: Vec<(usize, Rect)>,
    last_ptr: Option<(i32, i32)>,
}

impl List {
    pub fn new(focus: usize) -> List {
        List {
            focus,
            ..Default::default()
        }
    }

    fn fix_focus(&mut self, items: &[Item]) {
        if items.is_empty() {
            return;
        }
        self.focus = self.focus.min(items.len() - 1);
        if !items[self.focus].focusable() {
            if let Some(i) = (self.focus..items.len())
                .chain(0..self.focus)
                .find(|i| items[*i].focusable())
            {
                self.focus = i;
            }
        }
    }

    fn step(&mut self, items: &[Item], d: i32) -> bool {
        let n = items.len() as i32;
        let mut i = self.focus as i32;
        for _ in 0..n {
            i += d;
            if i < 0 || i >= n {
                return false;
            }
            if items[i as usize].focusable() {
                self.focus = i as usize;
                return true;
            }
        }
        false
    }

    pub fn handle(&mut self, items: &[Item], input: UiIn) -> Ev {
        self.fix_focus(items);
        match input {
            UiIn::Press(Button::Up) => {
                if self.step(items, -1) {
                    Ev::Moved
                } else {
                    Ev::None
                }
            }
            UiIn::Press(Button::Down) => {
                if self.step(items, 1) {
                    Ev::Moved
                } else {
                    Ev::None
                }
            }
            UiIn::Press(Button::Left) if items.get(self.focus).is_some_and(|i| i.kind == Kind::Choice) => {
                Ev::Adjust(self.focus, -1)
            }
            UiIn::Press(Button::Right) if items.get(self.focus).is_some_and(|i| i.kind == Kind::Choice) => {
                Ev::Adjust(self.focus, 1)
            }
            UiIn::Press(Button::Ok) => match items.get(self.focus) {
                Some(it) if it.focusable() && it.kind == Kind::Choice => Ev::Adjust(self.focus, 1),
                Some(it) if it.focusable() => Ev::Activate(self.focus),
                _ => Ev::None,
            },
            UiIn::Press(Button::Back) => Ev::Back,
            UiIn::Pointer(x, y) => {
                let moved = self
                    .last_ptr
                    .is_none_or(|(lx, ly)| (lx - x).abs() + (ly - y).abs() > 2);
                self.last_ptr = Some((x, y));
                if !moved {
                    return Ev::None;
                }
                match self.hit(items, x, y) {
                    Some(i) if i != self.focus => {
                        self.focus = i;
                        Ev::Moved
                    }
                    _ => Ev::None,
                }
            }
            UiIn::Click(x, y) => match self.hit(items, x, y) {
                Some(i) => {
                    self.focus = i;
                    if items[i].kind == Kind::Choice {
                        Ev::Adjust(i, 1)
                    } else {
                        Ev::Activate(i)
                    }
                }
                None => Ev::None,
            },
            UiIn::Wheel(dy) => {
                if self.step(items, if dy > 0 { -1 } else { 1 }) {
                    Ev::Moved
                } else {
                    Ev::None
                }
            }
            _ => Ev::None,
        }
    }

    fn hit(&self, items: &[Item], x: i32, y: i32) -> Option<usize> {
        self.rects
            .iter()
            .find(|(i, r)| r.contains_point((x, y)) && items.get(*i).is_some_and(|it| it.focusable()))
            .map(|(i, _)| *i)
    }

    /// Draw the list in a column; returns the bottom y.
    pub fn draw(&mut self, g: &mut Gfx, items: &[Item], x: i32, y: i32, w: i32, max_rows: usize) -> i32 {
        self.fix_focus(items);
        let row = 64;
        if self.focus < self.scroll {
            self.scroll = self.focus;
        }
        if self.focus >= self.scroll + max_rows {
            self.scroll = self.focus + 1 - max_rows;
        }
        self.rects.clear();
        let mut cy = y;
        for (i, it) in items.iter().enumerate().skip(self.scroll).take(max_rows) {
            let r = Rect::new(x, cy, w as u32, (row - 12) as u32);
            if it.kind == Kind::Heading {
                g.text(&it.label, x + 24, cy + 22, 22.0, Face::Semi, theme::TEXT_FAINT);
                cy += row;
                continue;
            }
            let focused = i == self.focus;
            if focused {
                g.round(r, 26, theme::PANEL_HI);
                g.round_outline(r, 26, alpha(theme::CYAN, 0.9), 3);
            }
            let tc = if !it.enabled {
                theme::TEXT_FAINT
            } else if focused {
                theme::TEXT
            } else {
                theme::TEXT_DIM
            };
            g.text(&it.label, x + 32, cy + 11, 30.0, Face::Regular, tc);
            if let Some(v) = &it.value {
                let vc = if focused { theme::CYAN } else { theme::TEXT_DIM };
                let s = if focused && it.kind == Kind::Choice {
                    format!("‹  {v}  ›")
                } else {
                    v.clone()
                };
                g.text_right(&s, x + w - 32, cy + 12, 28.0, Face::Regular, vc);
            }
            self.rects.push((i, r));
            cy += row;
        }
        if items.len() > self.scroll + max_rows {
            g.text_centered("⌄", x + w / 2, cy - 8, 30.0, Face::Regular, theme::TEXT_FAINT);
        }
        cy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_skips_headings_and_disabled() {
        let items = vec![
            Item::heading("A"),
            Item::action("one"),
            Item::action("two").disabled(true),
            Item::choice("three", "x"),
        ];
        let mut l = List::new(0);
        assert_eq!(l.handle(&items, UiIn::Press(Button::Ok)), Ev::Activate(1));
        assert_eq!(l.handle(&items, UiIn::Press(Button::Down)), Ev::Moved);
        assert_eq!(l.focus, 3);
        assert_eq!(l.handle(&items, UiIn::Press(Button::Left)), Ev::Adjust(3, -1));
        assert_eq!(l.handle(&items, UiIn::Press(Button::Down)), Ev::None);
        assert_eq!(l.handle(&items, UiIn::Press(Button::Back)), Ev::Back);
    }
}
