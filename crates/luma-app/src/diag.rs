//! Input diagnostics (the first milestone: prove the input path on the TV) and the developer
//! screen for thresholds, traces and backend options.

use crate::app::{App, Screen};
use crate::audio::Sfx;
use crate::gfx::{alpha, theme, Face, Gfx, H};
use crate::keys::{self, In};
use crate::platform::now_us;
use crate::ui::{Ev, Item, UiIn};
use luma_input::buttons::Button;
use luma_input::config::Thresholds;
use luma_input::math::wrap_deg;
use luma_input::mrcu::LegacyQuatOrder;
use sdl2::rect::Rect;

impl App {
    pub fn diag_input(&mut self, inp: &In) {
        match inp {
            In::Button {
                b: Button::Back,
                down: true,
                ..
            } => {
                self.cursor_test_hidden = false;
                self.go(Screen::Menu)
            }
            In::Button {
                b: Button::Red,
                down: true,
                ..
            } => self.toggle_recording(),
            In::Button {
                b: Button::Blue,
                down: true,
                ..
            } => self.recenter(),
            In::Button {
                b: Button::Yellow,
                down: true,
                ..
            } => self.cursor_test_hidden = !self.cursor_test_hidden,
            In::Button {
                b: Button::Green,
                down: true,
                ..
            } => self.motion.restart(),
            _ => {}
        }
    }

    pub fn update_diag(&mut self, commits: &[luma_input::Commit]) {
        if !commits.is_empty() {
            self.sfx(Sfx::Recognized);
            self.diag_counts.0 += commits.len() as u64;
        }
    }

    pub fn draw_diag(&mut self, g: &mut Gfx, now: u64) {
        let st = self.motion.status();
        let small = 22.0;
        let lh = 30;
        g.text("Input diagnostics", 60, 40, 40.0, Face::Light, theme::TEXT);
        let origin = self.motion.origin.map(|o| o.label()).unwrap_or("none");
        let hw = self.motion.origin.is_some_and(|o| o.is_hardware());
        g.text(
            &format!(
                "Source: {origin}{}",
                if hw {
                    "  (real hardware)"
                } else {
                    "  (not hardware)"
                }
            ),
            60,
            98,
            24.0,
            Face::Regular,
            if hw { theme::OK } else { theme::WARN },
        );

        // Column 1: buttons, pointer, wheel.
        let mut y = 160;
        let head = |g: &mut Gfx, s: &str, x: i32, y: i32| {
            g.text(s, x, y, 20.0, Face::Semi, theme::TEXT_FAINT);
        };
        head(g, "BUTTONS, POINTER, WHEEL (newest last)", 60, y);
        y += 36;
        let log = keys::raw_log();
        let mut prev: Option<u64> = None;
        for e in log.iter().rev().take(16).collect::<Vec<_>>().into_iter().rev() {
            let gap = prev
                .map(|p| format!("+{:.0}ms", (e.t_us - p) as f32 / 1000.0))
                .unwrap_or_default();
            prev = Some(e.t_us);
            g.text(
                &format!("{:>8} {}", gap, e.text),
                60,
                y,
                small,
                Face::Regular,
                theme::TEXT_DIM,
            );
            y += lh;
        }
        let ptr = self
            .pointer
            .map(|(x, y)| format!("pointer {x}, {y}"))
            .unwrap_or_else(|| "pointer: no events yet".into());
        g.text(&ptr, 60, H - 150, small, Face::Regular, theme::TEXT);
        g.text(
            &format!(
                "system cursor {}",
                if self.cursor_test_hidden {
                    "HIDDEN (Yellow)"
                } else {
                    "shown (Yellow hides)"
                }
            ),
            60,
            H - 116,
            small,
            Face::Regular,
            theme::TEXT_DIM,
        );

        // Column 2: motion backend.
        let x2 = 700;
        let mut y = 160;
        head(g, "MOTION BACKEND", x2, y);
        y += 36;
        let hz = self.motion.rate.hz(now);
        let age = self.motion.rate.age_ms(now);
        let rows: Vec<(String, String)> = vec![
            ("state".into(), st.state.label()),
            ("backend".into(), format!("{:?}", st.backend)),
            (
                "registration".into(),
                st.registration.clone().unwrap_or_else(|| "-".into()),
            ),
            (
                "api version".into(),
                st.api_version.clone().unwrap_or_else(|| "-".into()),
            ),
            (
                "interval cur/min/max".into(),
                st.interval
                    .map(|(a, b, c)| format!("{a}/{b}/{c} ms"))
                    .unwrap_or_else(|| "-".into()),
            ),
            (
                "requested".into(),
                st.requested_ms
                    .map(|v| format!("{v} ms"))
                    .unwrap_or_else(|| "-".into()),
            ),
            ("measured rate".into(), format!("{hz:.1} Hz")),
            (
                "freshness".into(),
                age.map(|a| format!("{a:.0} ms")).unwrap_or_else(|| "-".into()),
            ),
            (
                "max gap".into(),
                format!("{:.0} ms", self.motion.rate.max_gap_us as f32 / 1000.0),
            ),
            ("fields".into(), self.motion.fields.describe()),
            (
                "samples / dropped".into(),
                format!("{} / {}", self.motion.rate.count, self.motion.dropped()),
            ),
            ("stream restarts".into(), st.generation.to_string()),
            ("malformed".into(), st.malformed.to_string()),
            (
                "last error".into(),
                st.last_error
                    .as_ref()
                    .map(|e| e.detail())
                    .unwrap_or_else(|| "-".into()),
            ),
        ];
        for (k, v) in rows {
            g.text(&k, x2, y, small, Face::Regular, theme::TEXT_FAINT);
            g.text(
                &v.chars().take(46).collect::<String>(),
                x2 + 230,
                y,
                small,
                Face::Regular,
                theme::TEXT,
            );
            y += lh;
        }
        y += 10;
        head(g, "SAMPLE", x2, y);
        y += 32;
        let f = self.engine.debug.last_frame;
        let mut lines = Vec::new();
        if let Some(s) = self.motion.last {
            if let Some(q) = s.quat {
                lines.push(format!(
                    "quat w {:+.3} x {:+.3} y {:+.3} z {:+.3}",
                    q.w, q.x, q.y, q.z
                ));
            }
            if let Some(gy) = s.gyro {
                lines.push(format!(
                    "gyro {:+.2} {:+.2} {:+.2} rad/s  ({:+.0} {:+.0} {:+.0} °/s)",
                    gy.x,
                    gy.y,
                    gy.z,
                    gy.x.to_degrees(),
                    gy.y.to_degrees(),
                    gy.z.to_degrees()
                ));
            }
            if let Some(a) = s.accel {
                lines.push(format!("accel {:+.2} {:+.2} {:+.2} m/s²", a.x, a.y, a.z));
            }
            if let Some(a) = s.linear_accel {
                lines.push(format!("linear {:+.2} {:+.2} {:+.2} m/s²", a.x, a.y, a.z));
            }
            if let Some(c) = s.cursor {
                lines.push(format!("service cursor {:.0}, {:.0}", c.0, c.1));
            }
        } else {
            lines.push("no samples yet".into());
        }
        let dps = self.engine.cfg.deg_per_sh;
        lines.push(format!(
            "aim yaw {:+.1}° pitch {:+.1}°  twist {:+.1}°",
            f.aim.x * dps,
            -f.aim.y * dps,
            f.twist_deg.map(wrap_deg).unwrap_or(0.0)
        ));
        lines.push(format!("aim speed {:.2} SH/s  ({:?})", f.vel.len(), f.source));
        let (gs, gchk) = self.engine.gyro_units();
        lines.push(format!(
            "gyro units: {}  ·  stroke travel {:.2} SH  return left {:.2} SH",
            if !gchk {
                "rad/s (documented, not yet confirmed)".to_owned()
            } else if gs > 2.0 {
                "rad/s (confirmed)".to_owned()
            } else {
                "deg/s (detected)".to_owned()
            },
            self.engine.swing.travel,
            self.engine.swing.ret_budget.max(0.0)
        ));
        let (oy, op) = self.engine.orientation_signs();
        let sign = |v: f32| match v {
            v if v > 0.0 => "matches gyro",
            v if v < 0.0 => "mirrored (corrected)",
            _ => "learning",
        };
        lines.push(format!(
            "orientation vs gyro: yaw {}  ·  pitch {}  ·  roll: gravity {} (points {}), pointer {}",
            sign(oy),
            sign(op),
            self.engine
                .roll_deg()
                .map(|r| format!("{r:+.0}°"))
                .unwrap_or_else(|| "?".into()),
            self.engine
                .pointing_elevation_deg()
                .map(|p| format!("{p:+.0}°"))
                .unwrap_or_else(|| "?".into()),
            self.engine
                .pointer_roll_deg()
                .map(|r| format!("{r:+.0}° (for comparison)"))
                .unwrap_or_else(|| "learning".into())
        ));
        for l in lines {
            g.text(&l, x2, y, small, Face::Regular, theme::TEXT_DIM);
            y += lh;
        }

        // Column 3: visual axes and gestures.
        let x3 = 1400;
        head(g, "REMOTE", x3, 160);
        let cx = x3 + 200;
        let cy = 330;
        g.round_outline(Rect::new(cx - 150, cy - 150, 300, 300), 150, theme::LINE, 1);
        let px = cx + (f.aim.x * 300.0).clamp(-150.0, 150.0) as i32;
        let py = cy + (f.aim.y * 300.0).clamp(-150.0, 150.0) as i32;
        let twist = f.twist_deg.unwrap_or(0.0);
        let dst = Rect::new(px - 14, py - 46, 28, 92);
        g.tex.block.set_color_mod(150, 160, 180);
        g.tex.block.set_alpha_mod(220);
        g.tex.block.set_blend_mode(sdl2::render::BlendMode::Blend);
        let _ = g
            .canvas
            .copy_ex(&g.tex.block, None, dst, twist as f64, None, false, false);
        let mut y = 520;
        head(g, "RECOGNIZERS", x3, y);
        y += 34;
        let ph = [
            ("twist", self.engine.twist.phase.label()),
            ("circle", self.engine.circle.phase.label()),
            ("swing", self.engine.swing.phase.label()),
            ("drop", self.engine.drop.phase.label()),
        ];
        for (n, p) in ph {
            let c = if p == "idle" {
                theme::TEXT_FAINT
            } else {
                theme::CYAN
            };
            g.text(&format!("{n}: {p}"), x3, y, small, Face::Regular, c);
            y += lh;
        }
        g.text(
            &format!(
                "armed {}  pointing {}",
                self.engine.is_armed(),
                if self.engine.debug.pointing_frozen {
                    "frozen"
                } else {
                    "free"
                }
            ),
            x3,
            y,
            small,
            Face::Regular,
            theme::TEXT_DIM,
        );
        y += lh + 10;
        head(g, "RECOGNIZED (decision time)", x3, y);
        y += 34;
        for c in self.engine.debug.recent.iter().rev().take(7) {
            let age = (now.saturating_sub(c.t_us)) as f32 / 1e6;
            g.text(
                &format!(
                    "{} · {:.0} ms · {}",
                    c.gesture.label(),
                    c.recognition_ms(),
                    c.recognizer
                ),
                x3,
                y,
                small,
                Face::Regular,
                alpha(theme::TEXT, (1.0 - age / 10.0).max(0.3)),
            );
            y += lh;
        }
        for (t, gst, why) in self.engine.debug.suppressed.iter().rev().take(3) {
            let _ = t;
            g.text(
                &format!("suppressed {} ({why})", gst.label()),
                x3,
                y,
                20.0,
                Face::Regular,
                theme::TEXT_FAINT,
            );
            y += 28;
        }
        // Frame timing.
        let (avg, max) = self.stats.summary();
        g.text(
            &format!("frame {avg:.1} ms avg, {max:.1} ms max   {}", self.sdl_desc),
            700,
            H - 116,
            small,
            Face::Regular,
            theme::TEXT_FAINT,
        );
        let rec = if self.recorder.is_some() {
            "Red: stop recording"
        } else {
            "Red: record trace"
        };
        g.text(
            &format!("{rec}   Green: restart motion   Blue: recenter   Yellow: hide cursor   Back: menu"),
            60,
            H - 70,
            22.0,
            Face::Regular,
            theme::TEXT_DIM,
        );
        if !self.dev_msg.is_empty() {
            g.text(&self.dev_msg, 700, H - 150, small, Face::Regular, theme::CYAN);
        }
        let mut notes = vec![format!("log {}", self.log_path.display())];
        if let Some(e) = &self.audio.error {
            notes.push(format!("audio unavailable: {e}"));
        }
        notes.extend(self.store.load_notes.iter().cloned());
        let mut ny = 160;
        for n in notes.iter().take(4) {
            g.text(
                &n.chars().take(60).collect::<String>(),
                60,
                H - 260 + ny - 160,
                18.0,
                Face::Regular,
                theme::TEXT_FAINT,
            );
            ny += 24;
        }
    }

    // ------------------------------------------------------------------ developer

    fn threshold_items(&self) -> (Vec<Item>, Vec<String>) {
        let mut items = vec![
            Item::action("Replay last trace with current profile"),
            Item::action("Restart motion backend"),
            Item::choice(
                "Legacy quaternion order",
                match self.store.settings.legacy_quat_order {
                    LegacyQuatOrder::Xyzw => "x y z w",
                    LegacyQuatOrder::Wxyz => "w x y z",
                },
            ),
            Item::action("Reset all thresholds"),
            Item::heading("THRESHOLDS (Left/Right adjust 5%)"),
        ];
        let mut keys = vec![String::new(); items.len()];
        if let Ok(serde_json::Value::Object(m)) = serde_json::to_value(&self.store.thresholds) {
            for (k, v) in m {
                let val = v.as_f64().unwrap_or(0.0);
                items.push(Item::choice(k.clone(), format!("{val:.3}")));
                keys.push(k);
            }
        }
        (items, keys)
    }

    pub fn developer_input(&mut self, u: UiIn) {
        let (items, keys) = self.threshold_items();
        match self.list.handle(&items, u) {
            Ev::Activate(0) => {
                self.dev_msg = match self
                    .last_trace
                    .as_ref()
                    .and_then(|p| std::fs::read_to_string(p).ok())
                {
                    Some(t) => {
                        let (lines, bad) = luma_input::trace::parse(&t);
                        let out = luma_input::trace::replay(
                            &lines,
                            Some(self.store.settings.profile),
                            Some(self.store.thresholds.clone()),
                        );
                        let names: Vec<&str> = out.iter().map(|c| c.gesture.label()).collect();
                        format!("{} lines ({} bad): {}", lines.len(), bad, names.join(", "))
                    }
                    None => "No trace recorded in this session (Diagnostics, Red).".into(),
                };
            }
            Ev::Activate(1) => self.motion.restart(),
            Ev::Adjust(2, _) | Ev::Activate(2) => {
                let o = match self.store.settings.legacy_quat_order {
                    LegacyQuatOrder::Xyzw => LegacyQuatOrder::Wxyz,
                    LegacyQuatOrder::Wxyz => LegacyQuatOrder::Xyzw,
                };
                self.store.settings.legacy_quat_order = o;
                self.motion.send(crate::motion::Cmd::SetQuatOrder(o));
                self.settings_changed();
            }
            Ev::Activate(3) => {
                self.store.thresholds = Thresholds::default();
                self.store.save_thresholds();
                self.settings_changed();
            }
            Ev::Adjust(i, d) if i >= 5 => {
                if let Ok(serde_json::Value::Object(mut m)) = serde_json::to_value(&self.store.thresholds) {
                    if let Some(v) = m.get_mut(&keys[i]).and_then(|v| v.as_f64()) {
                        let nv = if d > 0 { v * 1.05 } else { v / 1.05 };
                        m.insert(keys[i].clone(), serde_json::json!(nv));
                        if let Ok(t) = serde_json::from_value::<Thresholds>(serde_json::Value::Object(m)) {
                            self.store.thresholds = t;
                            self.store.save_thresholds();
                            self.settings_changed();
                        }
                    }
                }
            }
            Ev::Back => self.go(Screen::Menu),
            Ev::Moved => self.sfx(Sfx::MenuMove),
            _ => {}
        }
    }

    pub fn draw_developer(&mut self, g: &mut Gfx) {
        g.text("Developer", 220, 60, 50.0, Face::Light, theme::TEXT);
        let (items, _) = self.threshold_items();
        self.list.draw(g, &items, 220, 150, 1100, 11);
        let st = self.motion.status();
        let mut y = 160;
        g.text("BACKEND LOG", 1400, y, 20.0, Face::Semi, theme::TEXT_FAINT);
        y += 34;
        for l in st.log.iter() {
            y += g.paragraph(l, 1400, y, 480, 18.0, theme::TEXT_DIM);
        }
        y += 10;
        g.text("LAST PAYLOAD", 1400, y, 20.0, Face::Semi, theme::TEXT_FAINT);
        g.paragraph(&st.last_payload, 1400, y + 34, 480, 16.0, theme::TEXT_FAINT);
        g.paragraph(&self.dev_msg, 220, 980, 1500, 22.0, theme::CYAN);
        let lat: Vec<String> = self
            .stats
            .latency
            .iter()
            .map(|(n, r, a)| format!("{n}: decide {r:.0} ms, apply +{a:.1} ms"))
            .collect();
        g.paragraph(&lat.join("   "), 220, 1030, 1600, 18.0, theme::TEXT_FAINT);
        let _ = now_us();
    }
}
