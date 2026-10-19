//! Menus: main menu, resume prompt, pause, motion loss, results, settings and records.

use crate::app::{motion_summary, App, PauseReason, Screen};
use crate::audio::Sfx;
use crate::gfx::{alpha, theme, Face, Gfx, H, W};
use crate::illus::RemoteLive as RemoteLiveState;
use crate::store::ControlMode;
use crate::strings as S;
use crate::ui::{Ev, Item, UiIn};
use crate::view::{self, BOARD_H, BOARD_W, BOARD_X, BOARD_Y};
use luma_core::Mode;
use luma_input::buttons::{Button, Shortcut};
use luma_input::config::{HorizontalStyle, RotationStyle};
use sdl2::rect::Rect;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Act {
    Continue,
    Mode(Mode),
    Settings,
    Records,
    Diagnostics,
    Developer,
    Exit,
    NewGame,
    Menu,
    StartNew(Mode),
    KeepSaved,
    Resume,
    RotateCcw,
    Hold,
    Recenter,
    PauseSettings,
    Restart,
    QuitToMenu,
    TryAgain,
    ButtonsOnly,
    PlayAgain,
    // settings
    Control,
    Rotation,
    Sideways,
    DropGesture,
    DropSlide,
    RemoteHud,
    RollComp,
    SensSide,
    SensRot,
    SensDrop,
    Calibrate,
    Volume,
    Mute,
    More,
    Ghost,
    Glow,
    Reduced,
    Patterns,
    Das,
    Arr,
    Shortcuts,
    ZenPace,
    StartLevel,
    DimAfter,
    AutoPause,
    HideCursor,
    DeveloperToggle,
    RunSetup,
    Colour(Button),
    Wheel,
}

fn on_off(b: bool) -> &'static str {
    if b {
        S::ON
    } else {
        S::OFF
    }
}

fn seconds(s: u32) -> String {
    if s >= 60 && s.is_multiple_of(60) {
        format!("{} min", s / 60)
    } else {
        format!("{s} s")
    }
}

impl App {
    fn items(&self) -> Vec<(Item, Act)> {
        let st = &self.store.settings;
        let twist_ok = !self.motion.state().is_unavailable() || !self.motion.is_real();
        match self.screen {
            Screen::Menu => {
                let mut v = Vec::new();
                if let Some(sg) = &self.store.save {
                    v.push((
                        Item::choice(S::CONTINUE, mode_name(sg.mode())).kind_action(),
                        Act::Continue,
                    ));
                }
                v.push((Item::action(S::MARATHON), Act::Mode(Mode::Marathon)));
                v.push((Item::action(S::SPRINT), Act::Mode(Mode::Sprint)));
                v.push((Item::action(S::ZEN), Act::Mode(Mode::Zen)));
                v.push((Item::action(S::PRACTICE), Act::Mode(Mode::Practice)));
                v.push((Item::action(S::RECORDS), Act::Records));
                v.push((Item::action(S::SETTINGS), Act::Settings));
                if st.developer {
                    v.push((Item::action(S::DIAGNOSTICS), Act::Diagnostics));
                    v.push((Item::action(S::DEVELOPER), Act::Developer));
                }
                v.push((Item::action(S::EXIT), Act::Exit));
                v
            }
            Screen::Resume => vec![
                (Item::action(S::CONTINUE), Act::Continue),
                (Item::action(S::NEW_GAME), Act::NewGame),
                (Item::action(S::BACK_TO_MENU), Act::Menu),
            ],
            Screen::ConfirmNew(m) => vec![
                (Item::action(S::START_NEW), Act::StartNew(m)),
                (Item::action(S::KEEP_SAVED), Act::KeepSaved),
            ],
            Screen::Pause => {
                let mut v = vec![(Item::action(S::RESUME), Act::Resume)];
                v.push((Item::action(S::ROTATE_CCW), Act::RotateCcw));
                v.push((Item::action(S::HOLD), Act::Hold));
                if self.motion_control_active() {
                    v.push((Item::action(S::RECENTER), Act::Recenter));
                }
                v.push((Item::action(S::CONTROLS_SETTINGS), Act::PauseSettings));
                v.push((Item::action(S::RESTART), Act::Restart));
                v.push((
                    Item::action(
                        if self
                            .session
                            .as_ref()
                            .is_some_and(|s| s.game.mode == Mode::Practice)
                        {
                            S::BACK_TO_MENU
                        } else {
                            S::QUIT_TO_MENU
                        },
                    ),
                    Act::QuitToMenu,
                ));
                v
            }
            Screen::MotionLost => vec![
                (Item::action(S::TRY_AGAIN), Act::TryAgain),
                (Item::action(S::PLAY_WITH_BUTTONS), Act::ButtonsOnly),
                (Item::action(S::CONTROLS_SETTINGS), Act::PauseSettings),
            ],
            Screen::Over => {
                let zen = self.over.as_ref().is_some_and(|o| o.mode == Mode::Zen);
                vec![
                    (
                        Item::action(if zen { S::START_FRESH } else { S::PLAY_AGAIN }),
                        Act::PlayAgain,
                    ),
                    (Item::action(S::BACK_TO_MENU), Act::Menu),
                ]
            }
            Screen::Settings => {
                let p = &st.profile;
                let motion = st.control == ControlMode::Motion;
                vec![
                    (
                        Item::choice(
                            S::CONTROL,
                            if motion {
                                S::CONTROL_MOTION
                            } else {
                                S::CONTROL_BUTTONS
                            },
                        ),
                        Act::Control,
                    ),
                    (
                        Item::choice(
                            S::ROTATION,
                            match p.rotation {
                                RotationStyle::Twist => S::ROT_TWIST,
                                RotationStyle::Circle => S::ROT_CIRCLE,
                            },
                        )
                        .disabled(!motion),
                        Act::Rotation,
                    ),
                    (
                        Item::choice(
                            S::SIDEWAYS,
                            match p.horizontal {
                                HorizontalStyle::Swing => S::SIDE_SWING,
                                HorizontalStyle::Pointing => S::SIDE_POINT,
                                HorizontalStyle::Flick => S::SIDE_FLICK,
                            },
                        )
                        .disabled(!motion),
                        Act::Sideways,
                    ),
                    (
                        Item::choice(S::DROP_GESTURE, on_off(p.gesture_drop)).disabled(!motion),
                        Act::DropGesture,
                    ),
                    (
                        Item::choice(S::DROP_SLIDE, drop_slide_label(st.drop_slide_ms)),
                        Act::DropSlide,
                    ),
                    (
                        Item::choice(S::REMOTE_HUD, on_off(st.remote_hud)).disabled(!motion),
                        Act::RemoteHud,
                    ),
                    (
                        Item::choice(S::ROLL_COMP, on_off(p.roll_compensation)).disabled(!motion),
                        Act::RollComp,
                    ),
                    (
                        Item::choice(S::SENS_SIDE, format!("{:.1}", p.sens_horizontal)).disabled(!motion),
                        Act::SensSide,
                    ),
                    (
                        Item::choice(S::SENS_ROT, format!("{:.1}", p.sens_rotation)).disabled(!motion),
                        Act::SensRot,
                    ),
                    (
                        Item::choice(S::SENS_DROP, format!("{:.1}", p.sens_drop)).disabled(!motion),
                        Act::SensDrop,
                    ),
                    (Item::action(S::CALIBRATE).disabled(!motion), Act::Calibrate),
                    (Item::choice(S::VOLUME, st.volume.to_string()), Act::Volume),
                    (Item::choice(S::MUTE, on_off(st.mute)), Act::Mute),
                    (Item::action(S::MORE), Act::More),
                ]
                .into_iter()
                .map(|(mut i, a)| {
                    if a == Act::Rotation && !twist_ok && p.rotation == RotationStyle::Twist {
                        i.value = Some(format!("{} ({})", S::ROT_TWIST, S::UNAVAILABLE_TWIST));
                    }
                    (i, a)
                })
                .collect()
            }
            Screen::MoreSettings => vec![
                (Item::choice(S::GHOST, on_off(st.ghost)), Act::Ghost),
                (
                    Item::choice(S::GLOW, format!("{:.0}%", st.glow * 100.0)),
                    Act::Glow,
                ),
                (
                    Item::choice(S::REDUCED_MOTION, on_off(st.reduced_motion)),
                    Act::Reduced,
                ),
                (Item::choice(S::PATTERNS, on_off(st.patterns)), Act::Patterns),
                (Item::choice(S::DAS, format!("{} ms", st.repeat.das_ms)), Act::Das),
                (Item::choice(S::ARR, format!("{} ms", st.repeat.arr_ms)), Act::Arr),
                (Item::action(S::SHORTCUTS), Act::Shortcuts),
                (Item::choice(S::ZEN_PACE, st.zen_level.to_string()), Act::ZenPace),
                (
                    Item::choice(S::START_LEVEL, st.marathon_start_level.to_string()),
                    Act::StartLevel,
                ),
                (Item::choice(S::DIM_AFTER, seconds(st.idle_dim_s)), Act::DimAfter),
                (
                    Item::choice(
                        S::AUTO_PAUSE,
                        if st.auto_pause_s == 0 {
                            S::NEVER.to_owned()
                        } else {
                            seconds(st.auto_pause_s)
                        },
                    ),
                    Act::AutoPause,
                ),
                (
                    Item::choice(S::HIDE_CURSOR, on_off(st.hide_cursor)),
                    Act::HideCursor,
                ),
                (
                    Item::choice(S::DEVELOPER_TOGGLE, on_off(st.developer)),
                    Act::DeveloperToggle,
                ),
                (Item::action(S::RUN_SETUP), Act::RunSetup),
            ],
            Screen::Shortcuts => {
                let mut v = Vec::new();
                for b in [Button::Red, Button::Green, Button::Yellow, Button::Blue] {
                    let seen = st.seen_buttons.contains(&b);
                    let label = if seen {
                        b.name().to_owned()
                    } else {
                        format!("{} (not seen yet)", b.name())
                    };
                    v.push((
                        Item::choice(label, st.bindings.for_button(b).label()),
                        Act::Colour(b),
                    ));
                }
                v.push((
                    Item::choice(S::WHEEL_ROTATES, on_off(st.bindings.wheel_rotates)),
                    Act::Wheel,
                ));
                v
            }
            _ => vec![(Item::action(S::BACK_TO_MENU), Act::Menu)],
        }
    }

    pub fn screen_input(&mut self, u: UiIn) {
        // The results appear after the board has greyed out; a drop press still in flight
        // must not start a new game.
        if self.screen == Screen::Over
            && crate::platform::now_us().saturating_sub(self.screen_since) < 1_100_000
        {
            return;
        }
        if self.screen == Screen::Developer {
            return self.developer_input(u);
        }
        let items = self.items();
        let plain: Vec<Item> = items.iter().map(|(i, _)| i.clone()).collect();
        if self.screen == Screen::Records {
            match u {
                UiIn::Press(Button::Left) | UiIn::Press(Button::Right) => {
                    self.records_tab ^= 1;
                    self.sfx(Sfx::MenuMove);
                }
                UiIn::Press(Button::Back) | UiIn::Press(Button::Ok) | UiIn::Click(..) => {
                    self.go(Screen::Menu)
                }
                _ => {}
            }
            return;
        }
        match self.list.handle(&plain, u) {
            Ev::Moved => self.sfx(Sfx::MenuMove),
            Ev::Activate(i) => {
                self.sfx(Sfx::MenuSelect);
                self.act(items[i].1, 1);
            }
            Ev::Adjust(i, d) => {
                self.sfx(Sfx::MenuMove);
                self.act(items[i].1, d);
            }
            Ev::Back => self.back(),
            Ev::None => {}
        }
    }

    fn back(&mut self) {
        match self.screen {
            Screen::Pause => self.resume(),
            Screen::MotionLost => self.go(Screen::Pause),
            Screen::Resume | Screen::ConfirmNew(_) | Screen::Over | Screen::Records => self.go(Screen::Menu),
            Screen::Settings => self.go(self.return_to),
            Screen::MoreSettings => self.go(Screen::Settings),
            Screen::Shortcuts => self.go(Screen::MoreSettings),
            Screen::Menu => {} // never exit on Back from the main menu by accident
            _ => self.go(Screen::Menu),
        }
    }

    fn act(&mut self, a: Act, d: i32) {
        let st = &mut self.store.settings;
        let step = |v: f32, d: i32| ((v + d as f32 * 0.1) * 10.0).round() / 10.0;
        let mut changed = true;
        match a {
            Act::Continue => {
                changed = false;
                self.continue_saved();
            }
            Act::Mode(m) => {
                changed = false;
                self.menu_focus = self.list.focus;
                self.request_mode(m);
            }
            Act::Settings => {
                changed = false;
                self.menu_focus = self.list.focus;
                self.return_to = Screen::Menu;
                self.go(Screen::Settings);
            }
            Act::Records => {
                changed = false;
                self.menu_focus = self.list.focus;
                self.go(Screen::Records);
            }
            Act::Diagnostics => {
                changed = false;
                self.menu_focus = self.list.focus;
                self.go(Screen::Diagnostics);
            }
            Act::Developer => {
                changed = false;
                self.menu_focus = self.list.focus;
                self.go(Screen::Developer);
            }
            Act::Exit => {
                changed = false;
                self.exit();
            }
            Act::NewGame => {
                changed = false;
                self.menu_focus = 0;
                self.go(Screen::Menu);
            }
            Act::Menu => {
                changed = false;
                self.session = None;
                self.go(Screen::Menu);
            }
            Act::StartNew(m) => {
                changed = false;
                self.store.clear_game();
                self.new_game(m);
            }
            Act::KeepSaved => {
                changed = false;
                self.go(Screen::Menu);
            }
            Act::Resume => {
                changed = false;
                self.resume();
            }
            Act::RotateCcw | Act::Hold => {
                changed = false;
                if let Some(s) = self.session.as_mut() {
                    s.game.apply(if a == Act::Hold {
                        luma_core::Action::Hold
                    } else {
                        luma_core::Action::RotateCcw
                    });
                    s.game.drain_events();
                }
                self.resume();
            }
            Act::Recenter => {
                changed = false;
                self.recenter();
                self.resume();
            }
            Act::PauseSettings => {
                changed = false;
                self.return_to = Screen::Pause;
                self.go(Screen::Settings);
            }
            Act::Restart => {
                changed = false;
                if let Some(m) = self.session.as_ref().map(|s| s.game.mode) {
                    self.store.clear_game();
                    self.new_game(m);
                }
            }
            Act::QuitToMenu => {
                changed = false;
                self.quit_to_menu();
            }
            Act::TryAgain => {
                changed = false;
                self.motion.restart();
            }
            Act::ButtonsOnly => {
                changed = false;
                if let Some(s) = self.session.as_mut() {
                    s.buttons_only = true;
                }
                self.pause_reason = PauseReason::User;
                self.go(Screen::Pause);
            }
            Act::PlayAgain => {
                changed = false;
                if let Some(m) = self.over.as_ref().map(|o| o.mode) {
                    self.new_game(m);
                }
            }
            Act::Control => {
                st.control = if st.control == ControlMode::Motion {
                    ControlMode::Buttons
                } else {
                    ControlMode::Motion
                };
            }
            Act::Rotation => {
                st.profile.rotation = match st.profile.rotation {
                    RotationStyle::Twist => RotationStyle::Circle,
                    RotationStyle::Circle => RotationStyle::Twist,
                }
            }
            Act::Sideways => {
                let order = [
                    HorizontalStyle::Swing,
                    HorizontalStyle::Pointing,
                    HorizontalStyle::Flick,
                ];
                let i = order
                    .iter()
                    .position(|h| *h == st.profile.horizontal)
                    .unwrap_or(0) as i32;
                st.profile.horizontal = order[(i + d).rem_euclid(3) as usize];
            }
            Act::DropGesture => st.profile.gesture_drop = !st.profile.gesture_drop,
            Act::RemoteHud => st.remote_hud = !st.remote_hud,
            Act::RollComp => st.profile.roll_compensation = !st.profile.roll_compensation,
            Act::DropSlide => {
                let opts = [
                    0,
                    crate::store::DROP_SLIDE_SHORT_MS,
                    crate::store::DROP_SLIDE_LONG_MS,
                ];
                st.drop_slide_ms = cycle(&opts, st.drop_slide_ms, d);
            }
            Act::SensSide => st.profile.sens_horizontal = step(st.profile.sens_horizontal, d),
            Act::SensRot => st.profile.sens_rotation = step(st.profile.sens_rotation, d),
            Act::SensDrop => st.profile.sens_drop = step(st.profile.sens_drop, d),
            Act::Calibrate => {
                changed = false;
                let back = self.screen;
                self.start_calibration(back);
            }
            Act::Volume => {
                st.volume = (st.volume as i32 + d).clamp(0, 10) as u32;
            }
            Act::Mute => st.mute = !st.mute,
            Act::More => {
                changed = false;
                self.go(Screen::MoreSettings);
            }
            Act::Ghost => st.ghost = !st.ghost,
            Act::Glow => st.glow = (st.glow + d as f32 * 0.1).clamp(0.4, 1.4),
            Act::Reduced => st.reduced_motion = !st.reduced_motion,
            Act::Patterns => st.patterns = !st.patterns,
            Act::Das => st.repeat.das_ms = (st.repeat.das_ms as i32 + d * 10).clamp(60, 400) as u32,
            Act::Arr => st.repeat.arr_ms = (st.repeat.arr_ms as i32 + d * 5).clamp(16, 200) as u32,
            Act::Shortcuts => {
                changed = false;
                self.go(Screen::Shortcuts);
            }
            Act::ZenPace => st.zen_level = (st.zen_level as i32 + d).clamp(1, 10) as u32,
            Act::StartLevel => {
                st.marathon_start_level = (st.marathon_start_level as i32 + d).clamp(1, 15) as u32
            }
            Act::DimAfter => {
                let opts = [30, 60, 90, 120, 180, 300, 600];
                st.idle_dim_s = cycle(&opts, st.idle_dim_s, d);
            }
            Act::AutoPause => {
                let opts = [0, 60, 120, 180, 300, 600];
                st.auto_pause_s = cycle(&opts, st.auto_pause_s, d);
            }
            Act::HideCursor => st.hide_cursor = !st.hide_cursor,
            Act::DeveloperToggle => st.developer = !st.developer,
            Act::RunSetup => {
                changed = false;
                self.start_onboarding();
            }
            Act::Colour(b) => {
                let cur = st.bindings.for_button(b);
                let i = Shortcut::ALL.iter().position(|s| *s == cur).unwrap_or(0) as i32;
                let n = Shortcut::ALL.len() as i32;
                let next = Shortcut::ALL[((i + d).rem_euclid(n)) as usize];
                match b {
                    Button::Red => st.bindings.red = next,
                    Button::Green => st.bindings.green = next,
                    Button::Yellow => st.bindings.yellow = next,
                    _ => st.bindings.blue = next,
                }
            }
            Act::Wheel => st.bindings.wheel_rotates = !st.bindings.wheel_rotates,
        }
        if changed {
            self.settings_changed();
        }
    }

    // ------------------------------------------------------------------ drawing

    pub fn draw_screen(&mut self, g: &mut Gfx, now: u64) {
        match self.screen {
            Screen::Menu => self.draw_menu(g, now),
            Screen::Records => self.draw_records(g),
            Screen::Resume | Screen::ConfirmNew(_) => {
                let (title, body) = if self.screen == Screen::Resume {
                    let body = self
                        .store
                        .save
                        .as_ref()
                        .map(|s| {
                            let g = &s.game;
                            match g.mode {
                                Mode::Sprint => format!(
                                    "{}  ·  {} lines  ·  {}",
                                    mode_name(g.mode),
                                    g.lines,
                                    view::fmt_time(g.elapsed_us / 1000)
                                ),
                                _ => format!(
                                    "{}  ·  score {}  ·  level {}",
                                    mode_name(g.mode),
                                    view::fmt_score(g.score),
                                    g.level
                                ),
                            }
                        })
                        .unwrap_or_default();
                    (S::RESUME_TITLE, body)
                } else {
                    (S::REPLACE_SAVE_TITLE, S::REPLACE_SAVE_BODY.to_owned())
                };
                draw_brand(g, now, 0.35);
                g.text_centered(title, W / 2, 330, 54.0, Face::Light, theme::TEXT);
                g.text_centered(&body, W / 2, 420, 30.0, Face::Regular, theme::TEXT_DIM);
                let items: Vec<Item> = self.items().into_iter().map(|(i, _)| i).collect();
                self.list.draw(g, &items, W / 2 - 300, 520, 600, 4);
            }
            Screen::Settings | Screen::MoreSettings | Screen::Shortcuts => {
                let title = match self.screen {
                    Screen::Settings => S::SETTINGS,
                    Screen::MoreSettings => S::MORE,
                    _ => S::SHORTCUTS,
                };
                g.text(title, 220, 90, 54.0, Face::Light, theme::TEXT);
                let st = self.motion.state();
                g.text(
                    motion_summary(&st),
                    220,
                    170,
                    26.0,
                    Face::Regular,
                    theme::TEXT_FAINT,
                );
                let items: Vec<Item> = self.items().into_iter().map(|(i, _)| i).collect();
                self.list.draw(g, &items, 220, 240, 1100, 11);
                self.draw_settings_help(g);
            }
            _ => {}
        }
    }

    /// The small live remote under the stats while playing with motion.
    fn draw_remote_hud(&mut self, g: &mut Gfx, now: u64) {
        if !self.motion_control_active() || !self.store.settings.remote_hud {
            return;
        }
        let e = &self.engine;
        let f = e.debug.last_frame;
        let eff = luma_input::config::effective(&e.cfg, &e.profile);
        let progress = match e.profile.rotation {
            RotationStyle::Twist if e.twist.phase == luma_input::recognize::Phase::Candidate => {
                e.twist.last_delta / eff.twist_enter_deg.max(1.0)
            }
            RotationStyle::Circle if e.circle.is_candidate() => {
                e.circle.best.turn_deg / eff.circle_min_turn_deg.max(1.0)
            }
            _ => 0.0,
        };
        let flash = e.debug.recent.back().map(|c| {
            (
                c.gesture,
                (now.saturating_sub(c.t_us) as f32 / 650_000.0).min(1.0),
            )
        });
        let live = RemoteLiveState {
            aim: (f.aim.x, f.aim.y),
            twist_deg: f.twist_deg.unwrap_or(0.0),
            progress,
            live: self.motion.is_streaming(now),
            flash,
        };
        let lx = BOARD_X - 70;
        let cx = lx - 95;
        let cy = BOARD_Y + 810;
        g.text_right(
            S::HUD_MOTION,
            lx,
            BOARD_Y + 690,
            22.0,
            Face::Semi,
            theme::TEXT_FAINT,
        );
        crate::illus::remote_live(g, cx, cy, &live, now);
    }

    fn draw_settings_help(&mut self, g: &mut Gfx) {
        // A live illustration of the gesture behind the focused option.
        let focused = self.items().get(self.list.focus).map(|x| x.1);
        let p = self.store.settings.profile;
        let il = match focused {
            Some(Act::Rotation | Act::SensRot) => Some(match p.rotation {
                RotationStyle::Twist => crate::illus::Illus::Twist,
                RotationStyle::Circle => crate::illus::Illus::Circle,
            }),
            Some(Act::Sideways | Act::SensSide) => Some(match p.horizontal {
                HorizontalStyle::Swing => crate::illus::Illus::Swing,
                HorizontalStyle::Pointing => crate::illus::Illus::Point,
                HorizontalStyle::Flick => crate::illus::Illus::Flick,
            }),
            Some(Act::DropGesture | Act::SensDrop) => Some(crate::illus::Illus::Drop),
            _ => None,
        };
        let help = match focused {
            Some(Act::DropSlide) => Some(S::DROP_SLIDE_HELP),
            Some(Act::RollComp) => Some(S::ROLL_COMP_HELP),
            _ => None,
        };
        if let Some(text) = help {
            let r = Rect::new(1400, 250, 420, 360);
            g.round(r, 28, theme::PANEL);
            g.paragraph(text, r.x() + 30, r.y() + 34, 360, 26.0, theme::TEXT_DIM);
            return;
        }
        if let Some(il) = il {
            let r = Rect::new(1400, 250, 420, 420);
            g.round(r, 28, theme::PANEL);
            crate::illus::draw(
                g,
                il,
                r.center().x(),
                r.center().y() + 10,
                1.15,
                crate::platform::now_us(),
            );
            return;
        }
        let x = 1420;
        let st = &self.store.settings;
        let mut lines: Vec<String> = vec![
            "Buttons".into(),
            "Left / Right  move".into(),
            "Down  soft drop".into(),
        ];
        lines.push("Up  rotate clockwise".into());
        lines.push("OK  drop".into());
        lines.push("Back  pause".into());
        for b in [Button::Red, Button::Green, Button::Yellow, Button::Blue] {
            let sc = st.bindings.for_button(b);
            if sc != Shortcut::None && st.seen_buttons.contains(&b) {
                lines.push(format!("{}  {}", b.name(), sc.label().to_lowercase()));
            }
        }
        let mut y = 250;
        for (i, l) in lines.iter().enumerate() {
            let (px, c, f) = if i == 0 {
                (22.0, theme::TEXT_FAINT, Face::Semi)
            } else {
                (26.0, theme::TEXT_DIM, Face::Regular)
            };
            g.text(l, x, y, px, f, c);
            y += if i == 0 { 44 } else { 40 };
        }
    }

    fn draw_menu(&mut self, g: &mut Gfx, now: u64) {
        if !self.store.settings.reduced_motion {
            draw_ambient(g, now);
        }
        draw_brand(g, now, 1.0);
        let items: Vec<(Item, Act)> = self.items();
        let plain: Vec<Item> = items.iter().map(|(i, _)| i.clone()).collect();
        self.list.draw(g, &plain, 220, 300, 640, 10);
        // Context for the focused entry.
        let hint = match items.get(self.list.focus).map(|x| x.1) {
            Some(Act::Mode(Mode::Marathon)) => Some((
                S::MARATHON_HINT,
                self.store
                    .records
                    .best_marathon()
                    .map(|b| format!("{}  {}", S::HUD_BEST, view::fmt_score(b))),
            )),
            Some(Act::Mode(Mode::Sprint)) => Some((
                S::SPRINT_HINT,
                self.store
                    .records
                    .best_sprint()
                    .map(|b| format!("{}  {}", S::HUD_BEST, view::fmt_time(b))),
            )),
            Some(Act::Mode(Mode::Zen)) => Some((S::ZEN_HINT, None)),
            Some(Act::Mode(Mode::Practice)) => Some((S::PRACTICE_HINT, None)),
            _ => None,
        };
        if let Some((h, best)) = hint {
            g.text(h, 960, 336, 30.0, Face::Regular, theme::TEXT_DIM);
            if let Some(b) = best {
                g.text(&b, 960, 386, 26.0, Face::Regular, theme::TEXT_FAINT);
            }
        }
        let st = self.motion.state();
        let mode = if self.store.settings.control == ControlMode::Motion {
            motion_summary(&st)
        } else {
            S::CONTROL_BUTTONS
        };
        g.text(mode, 220, H - 90, 24.0, Face::Regular, theme::TEXT_FAINT);
        g.text_right(
            &format!("v{}", env!("CARGO_PKG_VERSION")),
            W - 120,
            H - 90,
            22.0,
            Face::Regular,
            theme::TEXT_FAINT,
        );
    }

    fn draw_records(&mut self, g: &mut Gfx) {
        g.text(S::RECORDS, 220, 90, 54.0, Face::Light, theme::TEXT);
        let tabs = [S::MARATHON, S::SPRINT];
        let mut x = 220;
        for (i, t) in tabs.iter().enumerate() {
            let on = i as u8 == self.records_tab;
            let w = g.text(
                t,
                x,
                190,
                32.0,
                Face::Regular,
                if on { theme::TEXT } else { theme::TEXT_FAINT },
            );
            if on {
                g.round(Rect::new(x, 236, w as u32, 3), 1, theme::CYAN);
            }
            x += w + 60;
        }
        let r = &self.store.records;
        let rows: Vec<(String, String)> = if self.records_tab == 0 {
            r.marathon
                .iter()
                .map(|m| {
                    (
                        view::fmt_score(m.score),
                        format!("level {}  ·  {} lines", m.level, m.lines),
                    )
                })
                .collect()
        } else {
            r.sprint
                .iter()
                .map(|s| (view::fmt_time(s.time_ms), format!("{} pieces", s.pieces)))
                .collect()
        };
        if rows.is_empty() {
            g.text(S::NO_RECORDS, 220, 300, 30.0, Face::Regular, theme::TEXT_DIM);
        }
        for (i, (a, b)) in rows.iter().enumerate() {
            let y = 300 + i as i32 * 62;
            g.text(
                &format!("{}", i + 1),
                220,
                y,
                30.0,
                Face::Regular,
                theme::TEXT_FAINT,
            );
            g.text(a, 300, y - 4, 38.0, Face::Light, theme::TEXT);
            g.text(b, 700, y + 2, 28.0, Face::Regular, theme::TEXT_DIM);
        }
        g.text(
            &format!("{} games played", r.games_played),
            220,
            H - 110,
            24.0,
            Face::Regular,
            theme::TEXT_FAINT,
        );
    }

    pub fn draw_play_overlay(&mut self, g: &mut Gfx, now: u64) {
        let panel_x = BOARD_X + 30;
        let panel_w = BOARD_W - 60;
        match self.screen {
            Screen::Game => {
                // A short, non-blocking note: motion gap, or motion coming back.
                let note = match (self.motion_note_due(now), self.notice) {
                    (true, _) => Some((S::MOTION_GAP, theme::WARN, 1.0)),
                    (_, Some((text, t0))) if now.saturating_sub(t0) < 2_500_000 => {
                        let a = 1.0 - now.saturating_sub(t0) as f32 / 2.5e6;
                        Some((text, theme::OK, a.min(1.0)))
                    }
                    _ => None,
                };
                if let Some((text, c, a)) = note {
                    g.text_centered(text, W / 2, BOARD_Y - 60, 28.0, Face::Regular, alpha(c, a));
                }
                self.draw_remote_hud(g, now);
                let ready = self.session.as_ref().and_then(|s| s.ready_until).is_some();
                if ready {
                    g.text_centered(
                        S::GET_READY,
                        W / 2,
                        BOARD_Y + 380,
                        44.0,
                        Face::Light,
                        theme::TEXT_DIM,
                    );
                }
                if let Some(s) = self.session.as_ref() {
                    if s.game.mode == Mode::Practice {
                        let x = BOARD_X + BOARD_W + 70;
                        let mut y = BOARD_Y + 640;
                        g.text("RECOGNIZED", x, y, 22.0, Face::Semi, theme::TEXT_FAINT);
                        y += 40;
                        for (t, l) in s.practice_log.iter().rev().take(5) {
                            let a = (1.0 - now.saturating_sub(*t) as f32 / 6e6).max(0.25);
                            g.text(l, x, y, 26.0, Face::Regular, alpha(theme::TEXT, a));
                            y += 36;
                        }
                        let phases = format!(
                            "twist {}  circle {}  swing {}  drop {}",
                            self.engine.twist.phase.label(),
                            self.engine.circle.phase.label(),
                            self.engine.swing.phase.label(),
                            self.engine.drop.phase.label()
                        );
                        g.text(
                            &phases,
                            BOARD_X - 380,
                            BOARD_Y + BOARD_H - 30,
                            20.0,
                            Face::Regular,
                            theme::TEXT_FAINT,
                        );
                    }
                }
            }
            Screen::Pause | Screen::MotionLost | Screen::Over => {
                // Dim the board; the menu sits on top of it. Results wait for the board to
                // grey out first.
                let since = now.saturating_sub(self.screen_since) as f32 / 1000.0;
                let delay = if self.screen == Screen::Over && !self.store.settings.reduced_motion {
                    800.0
                } else {
                    0.0
                };
                let a = ((since - delay) / 220.0).clamp(0.0, 1.0);
                if a <= 0.0 {
                    return;
                }
                g.fill(
                    Rect::new(0, 0, W as u32, H as u32),
                    sdl2::pixels::Color::RGBA(0, 0, 0, (222.0 * a) as u8),
                );
                let (title, body) = match self.screen {
                    Screen::Pause => (
                        match self.pause_reason {
                            PauseReason::Idle => S::PAUSED_IDLE,
                            _ => S::PAUSED,
                        },
                        None,
                    ),
                    Screen::MotionLost => (
                        S::MOTION_LOST_TITLE,
                        Some(match self.motion_detail() {
                            Some(d) => format!("{}\n\n{d}", S::MOTION_LOST_BODY),
                            None => S::MOTION_LOST_BODY.to_owned(),
                        }),
                    ),
                    _ => {
                        let o = self.over.as_ref();
                        let title = match o.map(|o| (o.mode, o.finished)) {
                            Some((Mode::Sprint, true)) => S::SPRINT_DONE,
                            Some((Mode::Zen, _)) => S::BOARD_FULL,
                            _ => S::GAME_OVER,
                        };
                        let body = o.map(|o| match o.mode {
                            Mode::Sprint if o.finished => {
                                format!("{}  ·  {}", view::fmt_time(o.time_ms), rank_text(o.rank))
                            }
                            Mode::Sprint => format!("{} lines", o.lines),
                            Mode::Marathon => format!(
                                "{}  ·  level {}  ·  {} lines  ·  {}",
                                view::fmt_score(o.score),
                                o.level,
                                o.lines,
                                rank_text(o.rank)
                            ),
                            _ => format!("{} lines", o.lines),
                        });
                        (title, body)
                    }
                };
                let wide = self.screen != Screen::Pause;
                let (px, pw) = if wide {
                    (W / 2 - 420, 840)
                } else {
                    (panel_x, panel_w)
                };
                g.text_centered(title, W / 2, 230, 50.0, Face::Light, theme::TEXT);
                let mut y = 320;
                if let Some(b) = body {
                    y += g.paragraph(&b, px + 20, y, pw - 40, 28.0, theme::TEXT_DIM) + 20;
                }
                let items: Vec<Item> = self.items().into_iter().map(|(i, _)| i).collect();
                self.list.draw(g, &items, px, y.max(330), pw, 8);
            }
            _ => {}
        }
    }
}

fn drop_slide_label(ms: u32) -> String {
    match ms {
        0 => S::DROP_SLIDE_OFF.to_owned(),
        m if m <= crate::store::DROP_SLIDE_SHORT_MS => {
            format!("{} ({:.1} s)", S::DROP_SLIDE_SHORT, m as f32 / 1000.0)
        }
        m => format!("{} ({:.1} s)", S::DROP_SLIDE_LONG, m as f32 / 1000.0),
    }
}

fn rank_text(r: Option<usize>) -> String {
    match r {
        Some(1) => S::NEW_BEST.to_owned(),
        Some(n) => format!("#{n} on the board"),
        None => String::new(),
    }
}

fn cycle(opts: &[u32], cur: u32, d: i32) -> u32 {
    let i = opts.iter().position(|v| *v >= cur).unwrap_or(0) as i32;
    opts[((i + d).rem_euclid(opts.len() as i32)) as usize]
}

pub fn mode_name(m: Mode) -> &'static str {
    match m {
        Mode::Marathon => S::MARATHON,
        Mode::Sprint => "Sprint",
        Mode::Zen => S::ZEN,
        Mode::Practice => S::PRACTICE,
    }
}

/// The wordmark with the launcher icon artwork, softly glowing.
pub fn draw_brand(g: &mut Gfx, now: u64, strength: f32) {
    let t = now as f32 / 1e6;
    let breathe = 0.85 + 0.15 * (t * 0.8).sin();
    let icon = Rect::new(214, 92, 99, 120);
    for (col, dx, dy) in [(theme::CYAN, 0, -20), (theme::CORAL, 10, 30)] {
        let s = 260;
        let gr = Rect::new(
            icon.center().x() + dx - s / 2,
            icon.center().y() + dy - s / 2,
            s as u32,
            s as u32,
        );
        let canvas = &mut g.canvas;
        Gfx::tinted(
            &mut g.tex.glow,
            canvas,
            gr,
            alpha(col, 0.10 * strength * breathe),
            true,
        );
    }
    let a = (strength.max(0.4) * 255.0) as u8;
    g.tex.brand.set_alpha_mod(a);
    g.tex.brand.set_color_mod(255, 255, 255);
    let _ = g.canvas.copy(&g.tex.brand, None, icon);
    g.text(
        S::APP_TITLE,
        350,
        118,
        76.0,
        Face::Light,
        alpha(theme::TEXT, strength.max(0.4)),
    );
}

trait ActionKind {
    fn kind_action(self) -> Self;
}
impl ActionKind for Item {
    fn kind_action(mut self) -> Self {
        self.kind = crate::ui::Kind::Action;
        self
    }
}

/// A few dim blocks drifting down behind the main menu: alive, quiet, and never static
/// (so nothing bright sits in one place on an OLED panel).
fn draw_ambient(g: &mut Gfx, now: u64) {
    let t = now as f32 / 1e6;
    let kinds = [
        luma_core::Kind::I,
        luma_core::Kind::T,
        luma_core::Kind::Z,
        luma_core::Kind::O,
        luma_core::Kind::J,
        luma_core::Kind::S,
        luma_core::Kind::L,
    ];
    for i in 0..14u32 {
        let h = (i.wrapping_mul(2_654_435_761)) as f32 / u32::MAX as f32;
        let h2 = (i.wrapping_mul(40_503).wrapping_add(17)) % 1000;
        let x = 1000.0 + h * 760.0;
        let speed = 18.0 + (h2 as f32 / 1000.0) * 26.0;
        let span = H as f32 + 240.0;
        let y = (t * speed + h2 as f32 * 3.1) % span - 120.0;
        let size = (24.0 + h * 22.0) as i32;
        let c = crate::view::colour(kinds[(i % 7) as usize]);
        // Fade in at the top, out at the bottom.
        let edge = ((y + 120.0) / 220.0)
            .min((span - 120.0 - y) / 220.0)
            .clamp(0.0, 1.0);
        let a = (0.10 + 0.08 * h) * edge;
        let r = Rect::new(x as i32, y as i32, size as u32, size as u32);
        let s = size * 3;
        let canvas = &mut g.canvas;
        Gfx::tinted(
            &mut g.tex.glow,
            canvas,
            Rect::new(r.center().x() - s / 2, r.center().y() - s / 2, s as u32, s as u32),
            alpha(c, a * 0.5),
            true,
        );
        Gfx::tinted(
            &mut g.tex.block,
            canvas,
            r,
            alpha(crate::gfx::scale(c, 0.8), a * 1.6),
            false,
        );
    }
}
