//! Turns normalized samples into calibrated per-sample "frames": a 2D aim position in screen
//! heights (x right, y down), its timestamp-aware velocity, and the wrist-twist angle and rate
//! about the calibrated long axis. Recognizers only ever see frames.

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
}

impl AimTracker {
    pub fn reset(&mut self) {
        *self = AimTracker::default();
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

    /// Frame from a motion sample. Needs orientation or gyro; returns None otherwise.
    #[allow(clippy::question_mark)] // the three-way source choice reads clearer as if/else
    pub fn motion(&mut self, s: &MotionSample, calib: &Calibration, cfg: &Thresholds) -> Option<Frame> {
        let long = calib.long_axis;
        let up = calib.up_axis;
        let right = calib.right_axis();
        let (aim, twist) = if let Some(q) = s.quat {
            let neutral = calib.neutral.unwrap_or(Quat::IDENTITY);
            let rel = neutral.relative_to(q);
            let f = rel.rotate(long);
            let yaw = f.dot(right).atan2(f.dot(long)).to_degrees();
            let pitch = f.dot(up).clamp(-1.0, 1.0).asin().to_degrees();
            let twist = rel.twist_angle(long).to_degrees() * calib.twist_sign;
            (
                Vec2::new(yaw / cfg.deg_per_sh, -pitch / cfg.deg_per_sh),
                Some(twist),
            )
        } else if let Some(g) = s.gyro {
            // Gyro-only: integrate short-term orientation, leaking back to zero so drift
            // never accumulates into a long-term position estimate.
            let dt = self
                .last_t
                .filter(|l| s.t_us > *l)
                .map(|l| ((s.t_us - l) as f32 / 1e6).min(0.1))
                .unwrap_or(0.0);
            let yaw_rate = -g.dot(up).to_degrees();
            let pitch_rate = g.dot(right).to_degrees();
            let twist_rate = g.dot(long).to_degrees() * calib.twist_sign;
            let leak = (-dt / 1.5).exp();
            self.gyro_yaw = (self.gyro_yaw + yaw_rate * dt) * leak;
            self.gyro_pitch = (self.gyro_pitch + pitch_rate * dt) * leak;
            self.gyro_twist = (self.gyro_twist + twist_rate * dt) * leak;
            (
                Vec2::new(self.gyro_yaw / cfg.deg_per_sh, -self.gyro_pitch / cfg.deg_per_sh),
                Some(self.gyro_twist),
            )
        } else {
            return None;
        };
        let (dt, gap) = self.advance(s.t_us, aim, cfg, AimSource::Sensor);
        let twist_rate = match (s.gyro, twist, self.last_twist) {
            (Some(g), _, _) => Some(g.dot(long).to_degrees() * calib.twist_sign),
            (None, Some(t), Some(l)) if dt > 0.0 => Some(wrap_deg(t - l) / dt),
            _ => None,
        };
        self.last_twist = twist;
        Some(Frame {
            t_us: s.t_us,
            dt_s: dt,
            aim,
            vel: self.vel,
            twist_deg: twist,
            twist_rate_dps: twist_rate,
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
            source: AimSource::Pointer,
            gap,
        }
    }
}
