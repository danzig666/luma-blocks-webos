//! The gesture arbiter: routes calibrated frames to the recognizers selected by the profile,
//! enforces trajectory ownership between them, applies button guards and re-arm rules, and
//! provides the pointing column. Its output is a list of [`Commit`]s; the game never sees raw
//! samples.
//!
//! Ownership rules:
//! * A circle candidate owns the trajectory: no stroke or drop commits and pointing is frozen.
//! * A twist candidate freezes pointing and blocks drops.
//! * A drop candidate blocks sideways strokes and freezes pointing.
//! * After a sideways stroke, the pull back is a return, not a move (see `Swing`).
//! * After any commit, the other recognizers' in-flight candidates are cancelled, so one
//!   trajectory yields at most one discrete action; pointing stays frozen briefly.
//! * A button press cancels candidates and suppresses commits for `button_guard_ms` (the
//!   press itself jiggles the remote).
//! * After `reset` (pause, resume, focus change, remote wake) nothing commits until the remote
//!   has been still for `rearm_still_ms`.

use crate::calib::{AimSource, Calibration, PointRange};
use crate::config::{effective, HorizontalStyle, Profile, RotationStyle, Thresholds};
use crate::frame::{AimTracker, Frame};
use crate::math::Vec2;
use crate::recognize::{Circle, Commit, Drop, Gesture, Phase, StrokeMode, Swing, Twist};
use crate::sample::MotionSample;
use std::collections::VecDeque;

#[derive(Debug, Clone, Default)]
pub struct Debug {
    pub last_frame: Frame,
    pub recent: VecDeque<Commit>,
    pub suppressed: VecDeque<(u64, Gesture, &'static str)>,
    pub armed: bool,
    pub pointing_frozen: bool,
    pub pointing_suspended: bool,
}

#[derive(Debug, Clone)]
pub struct GestureEngine {
    pub cfg: Thresholds,
    pub profile: Profile,
    pub calib: Calibration,
    eff: Thresholds,
    sensor: AimTracker,
    pointer: AimTracker,
    pub twist: Twist,
    pub circle: Circle,
    pub swing: Swing,
    pub drop: Drop,
    armed: bool,
    /// First frame seen while disarmed (how long re-arming has been waiting).
    disarmed_us: Option<u64>,
    /// The sensor stream is paused; recognizers were reset for it.
    stale: bool,
    still_since: Option<u64>,
    guard_until: u64,
    freeze_until: u64,
    last_rotate_us: u64,
    last_motion_us: Option<u64>,
    last_pointer: Option<(u64, Vec2)>,
    last_quat: Option<crate::math::Quat>,
    active: Option<AimSource>,
    smoothed_x: Option<f32>,
    held_column: Option<f32>,
    suspended_at: Option<f32>,
    out: Vec<Commit>,
    probe: Option<crate::calib::Probe>,
    pub debug: Debug,
    /// When false, frames update diagnostics but no gesture commits (Buttons control mode).
    pub enabled: bool,
}

const RECENT: usize = 12;
/// Data resuming while the remote turns faster than this (deg/s) resumes mid-movement.
const RESUME_MID_MOTION_DPS: f32 = 90.0;
/// Re-arm after this long even without a calm moment, ms.
const REARM_GIVE_UP_MS: f32 = 2500.0;

impl GestureEngine {
    pub fn new(cfg: Thresholds, profile: Profile, calib: Calibration) -> Self {
        let eff = effective(&cfg, &profile);
        GestureEngine {
            cfg,
            profile,
            calib,
            eff,
            sensor: AimTracker::default(),
            pointer: AimTracker::default(),
            twist: Twist::default(),
            circle: Circle::default(),
            swing: Swing::default(),
            drop: Drop::default(),
            armed: false,
            disarmed_us: None,
            stale: false,
            still_since: None,
            guard_until: 0,
            freeze_until: 0,
            last_rotate_us: 0,
            last_motion_us: None,
            last_pointer: None,
            last_quat: None,
            active: None,
            smoothed_x: None,
            held_column: None,
            suspended_at: None,
            out: Vec::new(),
            probe: None,
            debug: Debug::default(),
            enabled: true,
        }
    }

    pub fn set_profile(&mut self, profile: Profile, cfg: Thresholds) {
        self.profile = profile;
        self.cfg = cfg;
        self.eff = effective(&self.cfg, &self.profile);
        self.reset_recognizers();
    }

    pub fn set_calibration(&mut self, calib: Calibration) {
        self.calib = calib;
        self.sensor.reset();
        self.reset_recognizers();
    }

    fn reset_recognizers(&mut self) {
        self.twist.reset();
        self.circle.reset();
        self.swing.reset();
        self.drop.reset();
    }

    /// Clear all in-flight gesture state and require a fresh still/neutral pose before the
    /// next commit. Call on pause, resume, focus change, remote wake and reconnection.
    pub fn reset(&mut self) {
        self.reset_recognizers();
        self.sensor.reset();
        self.pointer.reset();
        self.armed = false;
        self.still_since = None;
        self.out.clear();
        self.held_column = None;
        self.smoothed_x = None;
    }

    pub fn is_armed(&self) -> bool {
        self.armed
    }

    /// The source currently driving gestures, if any.
    pub fn active_source(&self) -> Option<AimSource> {
        self.active
    }

    /// Re-centre: the current orientation becomes the neutral pose.
    pub fn recenter(&mut self) -> bool {
        match self.last_quat {
            Some(q) => {
                self.calib.neutral = Some(q);
                self.sensor.reset();
                self.reset_recognizers();
                self.held_column = None;
                self.smoothed_x = None;
                true
            }
            None => false,
        }
    }

    pub fn last_quat(&self) -> Option<crate::math::Quat> {
        self.last_quat
    }

    /// A button was pressed: cancel candidates and guard against the press jiggle.
    pub fn button(&mut self, t_us: u64) {
        self.guard_until = t_us + (self.cfg.button_guard_ms * 1000.0) as u64;
        self.twist.cancel();
        self.circle.cancel();
        let c = self.eff.clone();
        self.swing.cancel(t_us, &c);
        self.drop.cancel();
    }

    /// Arrow buttons override pointing until a deliberate new pointing movement.
    pub fn suspend_pointing(&mut self) {
        self.suspended_at = Some(self.smoothed_x.unwrap_or(self.debug.last_frame.aim.x));
        self.held_column = None;
    }

    /// Start measuring one guided calibration motion (see `calib::Probe`).
    pub fn start_probe(&mut self) {
        self.probe = Some(crate::calib::Probe::default());
    }

    pub fn probe(&self) -> Option<&crate::calib::Probe> {
        self.probe.as_ref()
    }

    pub fn stop_probe(&mut self) -> Option<crate::calib::Probe> {
        self.probe.take()
    }

    /// Delivered gyro units in degrees per second per unit, and whether that was confirmed.
    pub fn gyro_units(&self) -> (f32, bool) {
        (
            if self.sensor.gyro_to_dps > 0.0 {
                self.sensor.gyro_to_dps
            } else {
                crate::frame::RAD_TO_DEG
            },
            self.sensor.gyro_units_checked,
        )
    }

    /// How the orientation's yaw and pitch relate to the gyro's: +1 same, -1 mirrored
    /// (corrected), 0 not yet known.
    pub fn orientation_signs(&self) -> (f32, f32) {
        (self.sensor.orient_yaw_sign, self.sensor.orient_pitch_sign)
    }

    /// The remote's roll about its length relative to the room (from gravity), degrees.
    pub fn roll_deg(&self) -> Option<f32> {
        self.sensor.roll_rad.map(f32::to_degrees)
    }

    /// The roll compensation learned from LG's pointer (screen plane), degrees.
    pub fn pointer_roll_deg(&self) -> Option<f32> {
        self.sensor.cursor_roll_rad.map(f32::to_degrees)
    }

    pub fn motion(&mut self, s: &MotionSample) {
        if let Some(p) = self.probe.as_mut() {
            let dt = self
                .last_motion_us
                .filter(|l| s.t_us > *l)
                .map(|l| ((s.t_us - l) as f32 / 1e6).min(0.1))
                .unwrap_or(0.0);
            let scale = if self.sensor.gyro_to_dps > 0.0 {
                self.sensor.gyro_to_dps
            } else {
                crate::frame::RAD_TO_DEG
            };
            p.push(s.gyro.map(|g| g.scale(scale)), s.quat, dt);
        }
        if let Some(q) = s.quat {
            self.last_quat = Some(q);
        }
        if self.stale {
            // Data is back. From rest (the remote just started moving out of button mode)
            // gestures count at once; in the middle of a fast movement (a radio gap) that
            // movement is stale and must not complete a gesture, so wait for calm.
            let scale = if self.sensor.gyro_to_dps > 0.0 {
                self.sensor.gyro_to_dps
            } else {
                crate::frame::RAD_TO_DEG
            };
            if s.gyro.is_some_and(|g| g.len() * scale > RESUME_MID_MOTION_DPS) {
                self.armed = false;
                self.still_since = None;
            }
        }
        if let Some(f) = self.sensor.motion(s, &self.calib, &self.cfg) {
            self.last_motion_us = Some(s.t_us);
            self.stale = false;
            if self.active != Some(AimSource::Sensor) {
                self.switch_source(AimSource::Sensor);
            }
            self.frame(f);
        }
    }

    /// System pointer position in screen heights (pixels / screen height).
    pub fn pointer(&mut self, t_us: u64, aim: Vec2) {
        self.last_pointer = Some((t_us, aim));
        if self.last_motion_us.is_some() {
            // Once the remote's motion sensor has reported, gestures use only its relative
            // motion. The system pointer stops at the screen edges, and switching to it
            // whenever the sensor pauses (button mode, a short gap) reset the gestures.
            return;
        }
        let f = self.pointer.pointer(t_us, aim, &self.cfg);
        if self.active != Some(AimSource::Pointer) {
            self.switch_source(AimSource::Pointer);
        }
        self.frame(f);
    }

    fn sensor_fresh(&self, t_us: u64) -> bool {
        self.last_motion_us
            .is_some_and(|l| t_us.saturating_sub(l) as f32 / 1000.0 < self.cfg.max_sample_gap_ms)
    }

    fn switch_source(&mut self, s: AimSource) {
        self.active = Some(s);
        self.reset_recognizers();
        self.smoothed_x = None;
        self.armed = false;
        self.still_since = None;
    }

    /// Call once per frame: a pointer only reports while it moves, so a resting pointer is
    /// fed as repeated still samples to let velocities decay.
    pub fn tick(&mut self, now_us: u64) {
        if self.active == Some(AimSource::Pointer) {
            if let Some((t, p)) = self.last_pointer {
                if now_us > t + 25_000 && now_us.saturating_sub(self.debug.last_frame.t_us) >= 15_000 {
                    let f = self.pointer.pointer(now_us, p, &self.cfg);
                    self.frame(f);
                }
            }
        } else if self.active == Some(AimSource::Sensor) && !self.sensor_fresh(now_us) {
            // Stream paused (the remote rests or is in button mode) or stalled: drop in-flight
            // candidates rather than act on stale motion. Stay armed: when the remote moves
            // again the data resumes with that very movement, and it must count.
            if !self.stale {
                self.reset_recognizers();
                self.stale = true;
            }
        }
    }

    fn frame(&mut self, f: Frame) {
        self.debug.last_frame = f;
        let c = self.eff.clone();
        // Re-arm after reset: wait for calm. "Calm" starts as true stillness and relaxes the
        // longer it takes (up to 4x after about two seconds), then gives up waiting: a shaky
        // or busy hand must never leave gestures off for good.
        if self.armed {
            self.disarmed_us = None;
        }
        let waited_ms = (f.t_us - *self.disarmed_us.get_or_insert(f.t_us)) as f32 / 1000.0;
        let relax = 1.0 + (waited_ms / 600.0).min(3.0);
        let twist_still = f
            .twist_rate_dps
            .is_none_or(|r| r.abs() < c.still_twist_dps * relax);
        let still = f.vel.len() < c.still_speed_sh * relax && twist_still;
        if still {
            self.still_since.get_or_insert(f.t_us);
        } else {
            self.still_since = None;
        }
        if !self.armed {
            let ok = waited_ms >= REARM_GIVE_UP_MS
                || self
                    .still_since
                    .is_some_and(|s| (f.t_us - s) as f32 / 1000.0 >= c.rearm_still_ms);
            if ok {
                self.armed = true;
                self.reset_recognizers();
            } else {
                self.update_pointing(&f, true);
                self.debug.armed = false;
                return;
            }
        }
        self.debug.armed = true;
        let circles = self.profile.rotation == RotationStyle::Circle;
        let mut commits: Vec<Commit> = Vec::new();

        // Rotation first: it owns trajectories that drops/flicks might otherwise claim.
        match self.profile.rotation {
            RotationStyle::Twist => {
                if let Some(cm) = self.twist.update(&f, &c) {
                    commits.push(cm);
                }
            }
            RotationStyle::Circle => {
                if let Some(cm) = self.circle.update(&f, &c) {
                    commits.push(cm);
                }
            }
        }
        let rotating = self.twist.phase == Phase::Candidate || self.circle.is_candidate();
        let recently_rotated =
            (f.t_us.saturating_sub(self.last_rotate_us) as f32 / 1000.0) < c.drop_after_rotate_block_ms;
        if self.profile.gesture_drop {
            // A twist in progress blocks a drop. A circle candidate does not: the drop needs a
            // nearly straight, fast downward stroke, which a circle's side is not, while a
            // small sideways wind-up before a flick often looks like the start of a circle.
            // ...and only while it is actually turning (a candidate that stopped is about to
            // be dropped as a change of grip).
            let twisting = self.twist.phase == Phase::Candidate
                && f.twist_rate_dps.is_some_and(|r| r.abs() > c.still_twist_dps);
            let blocked = twisting || recently_rotated || !commits.is_empty();
            if let Some(cm) = self.drop.update(&f, &c, blocked) {
                commits.push(cm);
            }
        }
        // Sideways strokes: several column moves may come from one stroke.
        let mut moves: Vec<Commit> = Vec::new();
        if self.profile.horizontal != HorizontalStyle::Pointing {
            self.swing.mode = if self.profile.horizontal == HorizontalStyle::Flick {
                StrokeMode::Single
            } else {
                StrokeMode::Columns
            };
            let settling = f.t_us < self.freeze_until || self.circle.phase == Phase::Latched;
            // With circles, a stroke holds its columns until it ends and is dropped if it
            // curves away like a circle, so a circle candidate need not block it (a curved
            // swing looks like the start of one); a committed circle still cancels it.
            let rotation_blocks = rotating && !circles;
            let blocked = rotation_blocks || !commits.is_empty() || settling;
            // A drop owns its wind-up (and, through the post-commit freeze, the start of its
            // recoil): no sideways move from either. After that a deliberate sideways swing
            // may still slide the dropped piece.
            let dropping = self.drop.is_candidate();
            moves = self.swing.update(&f, &c, circles, blocked, dropping);
            // A stroke that moved the piece is sideways movement: a circle starts after it,
            // so its out-and-back loop can never add up to a turn.
            if self.swing.is_moving_stroke() || !moves.is_empty() {
                self.circle.cancel();
            }
        }

        // One discrete action per trajectory: rotation, then drop; strokes yield to both.
        if let Some(first) = commits.first().copied() {
            for other in commits.iter().skip(1).chain(moves.iter()) {
                self.suppress(f.t_us, other.gesture, "conflict on one trajectory");
            }
            moves.clear();
            if first.gesture.is_rotation() {
                self.last_rotate_us = f.t_us;
                self.drop.cancel();
                self.swing.cancel(f.t_us, &c);
            } else {
                self.swing.cancel(f.t_us, &c);
                self.circle.cancel();
                self.twist.cancel();
            }
            self.freeze_until = f.t_us + (c.post_commit_freeze_ms * 1000.0) as u64;
        }
        for cm in commits.into_iter().take(1).chain(moves) {
            if f.t_us < self.guard_until {
                self.suppress(f.t_us, cm.gesture, "button guard");
            } else if !self.enabled {
                self.suppress(f.t_us, cm.gesture, "motion control off");
            } else {
                self.out.push(cm);
                self.debug.recent.push_back(cm);
                while self.debug.recent.len() > RECENT {
                    self.debug.recent.pop_front();
                }
            }
        }
        let frozen = rotating
            || self.drop.is_candidate()
            || f.t_us < self.freeze_until
            || (self.profile.rotation == RotationStyle::Twist
                && self.twist.phase == Phase::Latched
                && f.t_us < self.last_rotate_us + 400_000);
        self.update_pointing(&f, frozen);
    }

    fn suppress(&mut self, t: u64, g: Gesture, why: &'static str) {
        self.debug.suppressed.push_back((t, g, why));
        while self.debug.suppressed.len() > RECENT {
            self.debug.suppressed.pop_front();
        }
    }

    fn update_pointing(&mut self, f: &Frame, frozen: bool) {
        self.debug.pointing_frozen = frozen;
        if frozen {
            return;
        }
        let tau = self.cfg.point_smoothing_ms / 1000.0;
        let ax = f.abs.unwrap_or(f.aim).x;
        let x = match self.smoothed_x {
            Some(s) if f.dt_s > 0.0 && !f.gap => s + (ax - s) * (f.dt_s / (tau + f.dt_s)),
            _ => ax,
        };
        self.smoothed_x = Some(x);
    }

    /// Drain recognized gestures.
    pub fn take(&mut self) -> Vec<Commit> {
        std::mem::take(&mut self.out)
    }

    /// Continuous target column (0..cols) under the player's aim, or None when pointing is
    /// unavailable or suspended by a button correction. `default_range` is used when the
    /// player has not calibrated a range for the active source.
    pub fn aim_column(&mut self, cols: f32, default_range: PointRange) -> Option<f32> {
        let src = self.active?;
        let x = self.smoothed_x?;
        if let Some(s) = self.suspended_at {
            if (x - s).abs() < self.cfg.point_reengage_sh {
                self.debug.pointing_suspended = true;
                return None;
            }
            self.suspended_at = None;
        }
        self.debug.pointing_suspended = false;
        let range = match src {
            AimSource::Sensor => self.calib.sensor_range,
            AimSource::Pointer => self.calib.pointer_range,
        }
        .unwrap_or(default_range)
        .scaled(self.profile.sens_horizontal);
        let u = range.column(x, cols);
        if self.debug.pointing_frozen {
            return self.held_column.or(Some(u));
        }
        self.held_column = Some(u);
        Some(u)
    }

    /// The smoothed aim x for calibration capture.
    pub fn aim_x(&self) -> Option<f32> {
        self.smoothed_x
    }
}

/// Pointing anchor convention: the pointer selects where the **horizontal centre of the
/// piece's occupied cells** should be. For a piece `width` cells wide the reachable centres
/// are `k + width/2`; the target changes only once the pointer passes the midpoint between
/// two centres by `hysteresis` columns. Returns the target leftmost occupied column.
pub fn target_left_column(u: f32, width: i32, cols: i32, current: Option<i32>, hysteresis: f32) -> i32 {
    let half = width as f32 / 2.0;
    let max_left = (cols - width).max(0);
    let ideal = (u - half).round().clamp(0.0, max_left as f32) as i32;
    match current {
        Some(cur) if (0..=max_left).contains(&cur) => {
            let centre = cur as f32 + half;
            if (u - centre).abs() > 0.5 + hysteresis {
                ideal
            } else {
                cur
            }
        }
        _ => ideal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_and_hysteresis() {
        // 3-wide piece: centres at 1.5, 2.5, ...; pointer at 4.6 -> left column 3 (centre 4.5).
        assert_eq!(target_left_column(4.6, 3, 10, None, 0.2), 3);
        // Near the 5.0 boundary the current target holds...
        assert_eq!(target_left_column(5.1, 3, 10, Some(3), 0.2), 3);
        // ...until the pointer is clearly past it.
        assert_eq!(target_left_column(5.3, 3, 10, Some(3), 0.2), 4);
        // Edges clamp to reachable columns for every width.
        assert_eq!(target_left_column(9.9, 4, 10, None, 0.2), 6);
        assert_eq!(target_left_column(0.0, 2, 10, None, 0.2), 0);
        assert_eq!(target_left_column(9.99, 1, 10, None, 0.2), 9);
    }
}
