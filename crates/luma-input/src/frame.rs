//! Turns normalized samples into calibrated per-sample "frames": a 2D aim position in screen
//! heights (x right, y down), its timestamp-aware velocity, and the wrist-twist angle and rate
//! about the calibrated long axis. Recognizers only ever see frames.
//!
//! Motion comes from the **gyroscope** whenever it is delivered: it is a direct body-frame
//! measurement with documented axes, while the fused orientation's reference convention is a
//! firmware detail. Short-term aim positions are integrated from the gyro with a slow leak, so
//! they never drift into a long-term position estimate. Absolute aim (for Pointing) comes from
//! the orientation, then the sensor's own cursor, then the system pointer. The gyro's units
//! (rad/s per LG's documentation) are cross-checked against the orientation at runtime.

use crate::calib::{AimSource, Calibration};
use crate::config::Thresholds;
use crate::math::{wrap_deg, Quat, Vec2};
use crate::sample::MotionSample;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Frame {
    pub t_us: u64,
    /// Seconds since the previous frame of the same source (0 for the first).
    pub dt_s: f32,
    /// Aim position, screen heights.
    pub aim: Vec2,
    /// Smoothed aim velocity, SH/s.
    pub vel: Vec2,
    /// Twist about the long axis relative to neutral, degrees, + = clockwise for the player.
    pub twist_deg: Option<f32>,
    /// Twist rate, degrees/s, + = clockwise.
    pub twist_rate_dps: Option<f32>,
    /// Absolute aim for pointing (screen heights), when the stream can provide one.
    pub abs: Option<Vec2>,
    pub source: AimSource,
    /// True when the stream stalled before this frame; trajectory state should restart.
    pub gap: bool,
}

#[derive(Debug, Clone, Default)]
pub struct AimTracker {
    last_t: Option<u64>,
    last_aim: Vec2,
    vel: Vec2,
    last_twist: Option<f32>,
    /// Integrated orientation for gyro-only streams (degrees).
    gyro_yaw: f32,
    gyro_pitch: f32,
    gyro_twist: f32,
    pub source: Option<AimSource>,
    last_quat: Option<Quat>,
    unit_quat_deg: f32,
    unit_gyro_raw: f32,
    /// Multiplier from the delivered gyro values to degrees per second.
    pub gyro_to_dps: f32,
    /// Whether the gyro units were confirmed against the orientation.
    pub gyro_units_checked: bool,
}

/// LG documents the gyroscope in rad/s.
pub const RAD_TO_DEG: f32 = 57.295_78;

impl AimTracker {
    pub fn reset(&mut self) {
        // The unit check survives resets: it describes the hardware, not the gesture.
        let (g, checked) = (self.gyro_to_dps, self.gyro_units_checked);
        *self = AimTracker::default();
        self.gyro_to_dps = g;
        self.gyro_units_checked = checked;
    }

    fn scale(&self) -> f32 {
        if self.gyro_to_dps > 0.0 {
            self.gyro_to_dps
        } else {
            RAD_TO_DEG
        }
    }

    /// Compare how far the orientation turned with how far the gyro says it turned. A ratio
    /// near 57 confirms rad/s; near 1 means the firmware reports deg/s.
    fn check_units(&mut self, s: &MotionSample, dt: f32) {
        if self.gyro_units_checked {
            return;
        }
        let (Some(q), Some(g)) = (s.quat, s.gyro) else {
            return;
        };
        if let Some(p) = self.last_quat {
            if dt > 0.0 && dt < 0.1 {
                let (_, ang) = p.relative_to(q).axis_angle();
                self.unit_quat_deg += ang.to_degrees();
                self.unit_gyro_raw += g.len() * dt;
            }
        }
        if self.unit_quat_deg > 90.0 && self.unit_gyro_raw > 0.0 {
            let ratio = self.unit_quat_deg / self.unit_gyro_raw;
            self.gyro_to_dps = if ratio < 8.0 { 1.0 } else { RAD_TO_DEG };
            self.gyro_units_checked = true;
        }
    }

    fn advance(&mut self, t_us: u64, aim: Vec2, cfg: &Thresholds, source: AimSource) -> (f32, bool) {
        let mut gap = self.source != Some(source);
        let dt = match self.last_t {
            Some(l) if t_us > l => (t_us - l) as f32 / 1e6,
            Some(_) => 0.0,
            None => {
                gap = true;
                0.0
            }
        };
        if dt * 1000.0 > cfg.max_sample_gap_ms {
            gap = true;
        }
        if gap {
            self.vel = Vec2::default();
        } else if dt > 0.0 {
            let raw = aim.sub(self.last_aim).scale(1.0 / dt);
            let tau = cfg.velocity_smoothing_ms / 1000.0;
            let a = dt / (tau + dt);
            self.vel = self.vel.add(raw.sub(self.vel).scale(a));
        }
        self.last_t = Some(t_us);
        self.last_aim = aim;
        self.source = Some(source);
        (if gap { 0.0 } else { dt }, gap)
    }

    /// Frame from a motion sample. Needs a gyro or an orientation; returns None otherwise.
    pub fn motion(&mut self, s: &MotionSample, calib: &Calibration, cfg: &Thresholds) -> Option<Frame> {
        let long = calib.long_axis;
        let up = calib.up_axis;
        let right = calib.right_axis();
        let dt_raw = self
            .last_t
            .filter(|l| s.t_us > *l)
            .map(|l| (s.t_us - l) as f32 / 1e6)
            .unwrap_or(0.0);
        self.check_units(s, dt_raw);
        let dt_int = dt_raw.min(0.1);
        // Absolute aim from the orientation (relative to the calibrated neutral pose).
        let orient = s.quat.map(|q| {
            let neutral = calib.neutral.unwrap_or(Quat::IDENTITY);
            let rel = neutral.relative_to(q);
            let f = rel.rotate(long);
            let yaw = f.dot(right).atan2(f.dot(long)).to_degrees() * calib.yaw_sign;
            let pitch = f.dot(up).clamp(-1.0, 1.0).asin().to_degrees() * calib.pitch_sign;
            let twist = rel.twist_angle(long).to_degrees() * calib.twist_sign;
            (Vec2::new(yaw / cfg.deg_per_sh, -pitch / cfg.deg_per_sh), twist)
        });
        let cursor = s.cursor.map(|(x, y)| Vec2::new(x / 1080.0, y / 1080.0));
        let use_gyro = s.gyro.is_some() && (cfg.prefer_gyro || orient.is_none());
        let (aim, twist, twist_rate_g) = if use_gyro {
            let g = s.gyro.unwrap().scale(self.scale());
            let yaw_rate = -g.dot(up) * calib.yaw_sign;
            let pitch_rate = g.dot(right) * calib.pitch_sign;
            let twist_rate = g.dot(long) * calib.twist_sign;
            // Short-term integration with a slow leak toward zero (no long-term drift).
            let leak = (-dt_int / 4.0).exp();
            self.gyro_yaw = (self.gyro_yaw + yaw_rate * dt_int) * leak;
            self.gyro_pitch = (self.gyro_pitch + pitch_rate * dt_int) * leak;
            self.gyro_twist = (self.gyro_twist + twist_rate * dt_int) * (-dt_int / 8.0).exp();
            (
                Vec2::new(self.gyro_yaw / cfg.deg_per_sh, -self.gyro_pitch / cfg.deg_per_sh),
                Some(self.gyro_twist),
                Some(twist_rate),
            )
        } else if let Some((a, t)) = orient {
            (a, Some(t), None)
        } else {
            return None;
        };
        let (dt, gap) = self.advance(s.t_us, aim, cfg, AimSource::Sensor);
        let twist_rate = match (twist_rate_g, twist, self.last_twist) {
            (Some(r), _, _) => Some(r),
            (None, Some(t), Some(l)) if dt > 0.0 => Some(wrap_deg(t - l) / dt),
            _ => None,
        };
        self.last_twist = twist;
        self.last_quat = s.quat;
        Some(Frame {
            t_us: s.t_us,
            dt_s: dt,
            aim,
            vel: self.vel,
            twist_deg: twist,
            twist_rate_dps: twist_rate,
            abs: orient.map(|o| o.0).or(cursor),
            source: AimSource::Sensor,
            gap,
        })
    }

    /// Frame from the system pointer, position already converted to screen heights.
    pub fn pointer(&mut self, t_us: u64, aim: Vec2, cfg: &Thresholds) -> Frame {
        let (dt, gap) = self.advance(t_us, aim, cfg, AimSource::Pointer);
        self.last_twist = None;
        Frame {
            t_us,
            dt_s: dt,
            aim,
            vel: self.vel,
            twist_deg: None,
            twist_rate_dps: None,
            abs: Some(aim),
            source: AimSource::Pointer,
            gap,
        }
    }
}
