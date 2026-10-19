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
use crate::math::{wrap_deg, Quat, Vec2, Vec3};
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
    /// Signs that align the orientation's yaw and pitch with the gyro's (+1 or -1; 0 = not
    /// yet known, treated as +1). Firmware may report the rotation vector in the opposite
    /// convention, which mirrors absolute pointing while gyro-based swings stay right.
    pub orient_yaw_sign: f32,
    pub orient_pitch_sign: f32,
    orient_prev: Option<(f32, f32)>,
    yaw_agree: Agreement,
    pitch_agree: Agreement,
    /// How far the remote is rolled about its length relative to the room, from gravity
    /// (radians, smoothed); None until gravity has been seen clearly.
    pub roll_rad: Option<f32>,
    /// +1 if the accelerometer reads +g along "up" when held normally (LG's example data and
    /// most sensors), -1 if the firmware reports the opposite sign; learned from the first
    /// clear readings (players start with the remote held normally).
    accel_up_sign: Option<f32>,
    /// Rotation (radians, screen plane) that turns our gyro motion into the way LG's own
    /// pointer moves, learned from the pointer's movement. The firmware's pointer already
    /// follows the room however the remote is held, so this is the most direct reference.
    pub cursor_roll_rad: Option<f32>,
    cursor_prev: Option<(f32, f32)>,
    /// Running sum of pointer step x conj(gyro step) (complex, decaying) and of the step
    /// magnitudes, for the fit above.
    cursor_fit: (f32, f32, f32),
    cursor_fit_us: u64,
}

/// LG reports its pointer in 1920 x 1080 pixels; steps at the edges are clamped.
const CURSOR_W: f32 = 1920.0;
const CURSOR_H: f32 = 1080.0;

/// Degrees of gyro-measured motion needed before deciding an orientation sign.
const ORIENT_CHECK_DEG: f32 = 25.0;

/// Running comparison of orientation steps with gyro steps about one axis, degrees.
#[derive(Debug, Clone, Copy, Default)]
struct Agreement {
    og: f32,
    oo: f32,
    gg: f32,
    travel: f32,
}

impl Agreement {
    /// Add one step; returns the sign once there is enough motion and a clear relation.
    fn learn(&mut self, o: f32, g: f32) -> Option<f32> {
        self.og += o * g;
        self.oo += o * o;
        self.gg += g * g;
        self.travel += g.abs();
        let corr = self.og / (self.oo * self.gg).sqrt().max(1e-9);
        (self.travel >= ORIENT_CHECK_DEG && corr.abs() > 0.5).then(|| corr.signum())
    }
}

/// LG documents the gyroscope in rad/s.
pub const RAD_TO_DEG: f32 = 57.295_78;

impl AimTracker {
    pub fn reset(&mut self) {
        // The unit check survives resets: it describes the hardware, not the gesture.
        // So do the orientation signs.
        let (g, checked, accel_sign, cursor_roll) = (
            self.gyro_to_dps,
            self.gyro_units_checked,
            self.accel_up_sign,
            self.cursor_roll_rad,
        );
        let (ys, ps, ya, pa) = (
            self.orient_yaw_sign,
            self.orient_pitch_sign,
            self.yaw_agree,
            self.pitch_agree,
        );
        *self = AimTracker::default();
        self.gyro_to_dps = g;
        self.gyro_units_checked = checked;
        self.accel_up_sign = accel_sign;
        self.cursor_roll_rad = cursor_roll;
        self.orient_yaw_sign = ys;
        self.orient_pitch_sign = ps;
        self.yaw_agree = ya;
        self.pitch_agree = pa;
    }

    /// Yaw and pitch rates (deg/s) in room directions: the body-frame rates turned by the
    /// remote's roll, so a remote held rolled still moves left/right and up/down the way the
    /// player sees it.
    fn room_rates(&self, g: Vec3, up: Vec3, right: Vec3, calib: &Calibration) -> (f32, f32) {
        // Screen-plane rates (x right, y down) as calibrated for the neutral hold...
        let (bx, by) = (-g.dot(up) * calib.yaw_sign, -g.dot(right) * calib.pitch_sign);
        // ...turned by how the remote is rolled.
        let (sn, cs) = self.screen_roll(calib).sin_cos();
        let (x, y) = (cs * bx - sn * by, sn * bx + cs * by);
        (x, -y)
    }

    /// The rotation in the screen plane that compensates the remote's roll: learned from
    /// LG's pointer when it has moved recently, else from gravity, else none.
    fn screen_roll(&self, calib: &Calibration) -> f32 {
        if let Some(t) = self.cursor_roll_rad {
            return t;
        }
        match self.roll_rad {
            // Gravity gives the roll in the body frame; in the screen plane it turns the
            // other way, and a calibrated mirror flips it again.
            Some(r) => -r * calib.yaw_sign * calib.pitch_sign,
            None => 0.0,
        }
    }

    /// Fit the rotation between our (calibrated, unrotated) gyro motion and LG's pointer
    /// motion. Steps where the pointer sits at a screen edge are skipped.
    fn learn_cursor(&mut self, t_us: u64, b: (f32, f32), dt: f32, cursor: Option<(f32, f32)>) {
        let Some((x, y)) = cursor else { return };
        let prev = self.cursor_prev.replace((x, y));
        let Some((px, py)) = prev else { return };
        if dt <= 0.0 {
            return;
        }
        let edge = |v: f32, max: f32| v <= 1.0 || v >= max - 2.0;
        if edge(x, CURSOR_W) || edge(y, CURSOR_H) || edge(px, CURSOR_W) || edge(py, CURSOR_H) {
            return;
        }
        let (cx, cy) = (x - px, y - py);
        let (bx, by) = (b.0 * dt, b.1 * dt);
        let (cl, bl) = ((cx * cx + cy * cy).sqrt(), (bx * bx + by * by).sqrt());
        if cl < 1.0 || bl < 0.05 {
            return;
        }
        // Forget old evidence over about a second of movement.
        let decay = (-dt / 1.0).exp();
        let (re, im, mag) = self.cursor_fit;
        let (re, im, mag) = (
            re * decay + cx * bx + cy * by,
            im * decay + cy * bx - cx * by,
            mag * decay + cl * bl,
        );
        self.cursor_fit = (re, im, mag);
        self.cursor_fit_us = t_us;
        // Enough movement and a consistent relation: adopt it.
        let coherence = (re * re + im * im).sqrt() / mag.max(1e-6);
        if mag > 40.0 && coherence > 0.8 {
            self.cursor_roll_rad = Some(im.atan2(re));
        }
    }

    /// Learn whether the orientation turns the same way as the gyro, from steps of both.
    fn check_orientation(&mut self, yaw: f32, pitch: f32, gyro_step: Option<(f32, f32)>) {
        if let (Some((py, pp)), Some((gy, gp))) = (self.orient_prev, gyro_step) {
            let (dy, dp) = (wrap_deg(yaw - py), pitch - pp);
            // Ignore jumps (re-centring, wrap-around noise); learn only from real motion.
            if dy.abs() < 15.0 && dp.abs() < 15.0 {
                if self.orient_yaw_sign == 0.0 && gy.abs() > 0.05 {
                    if let Some(s) = self.yaw_agree.learn(dy, gy) {
                        self.orient_yaw_sign = s;
                    }
                }
                if self.orient_pitch_sign == 0.0 && gp.abs() > 0.05 {
                    if let Some(s) = self.pitch_agree.learn(dp, gp) {
                        self.orient_pitch_sign = s;
                    }
                }
            }
        }
        self.orient_prev = Some((yaw, pitch));
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
        // Roll about the long axis, from gravity: while the remote rests or moves gently the
        // accelerometer points up in the body frame.
        if let Some(a) = s.accel {
            let (ar, au) = (a.dot(right), a.dot(up));
            if self.accel_up_sign.is_none() && au.abs() > 7.0 && (a.len() - 9.81).abs() < 1.5 {
                self.accel_up_sign = Some(au.signum());
                if au < 0.0 {
                    self.roll_rad = None; // was learned with the wrong sign
                }
            }
            let sg = self.accel_up_sign.unwrap_or(1.0);
            let (ar, au) = (ar * sg, au * sg);
            // Until a clear reading says otherwise, assume the documented sign.
            if (a.len() - 9.81).abs() < 2.5 && (ar * ar + au * au).sqrt() > 4.0 {
                let phi = ar.atan2(au);
                let k = if dt_int > 0.0 {
                    dt_int / (0.12 + dt_int)
                } else {
                    1.0
                };
                self.roll_rad = Some(match self.roll_rad {
                    Some(r) => r + wrap_rad(phi - r) * k,
                    None => phi,
                });
            }
        }
        let raw_orient = s.quat.map(|q| {
            let neutral = calib.neutral.unwrap_or(Quat::IDENTITY);
            let rel = neutral.relative_to(q);
            let f = rel.rotate(long);
            let yaw = f.dot(right).atan2(f.dot(long)).to_degrees() * calib.yaw_sign;
            let pitch = f.dot(up).clamp(-1.0, 1.0).asin().to_degrees() * calib.pitch_sign;
            let twist = rel.twist_angle(long).to_degrees() * calib.twist_sign;
            (yaw, pitch, twist)
        });
        if let Some((yaw, pitch, _)) = raw_orient {
            let gyro_step = s.gyro.filter(|_| dt_int > 0.0).map(|g| {
                let (y, p) = self.room_rates(g.scale(self.scale()), up, right, calib);
                (y * dt_int, p * dt_int)
            });
            self.check_orientation(yaw, pitch, gyro_step);
        }
        let sy = if self.orient_yaw_sign < 0.0 { -1.0 } else { 1.0 };
        let sp = if self.orient_pitch_sign < 0.0 { -1.0 } else { 1.0 };
        let orient = raw_orient.map(|(yaw, pitch, twist)| {
            (
                Vec2::new(sy * yaw / cfg.deg_per_sh, -sp * pitch / cfg.deg_per_sh),
                twist,
            )
        });
        let cursor = s.cursor.map(|(x, y)| Vec2::new(x / 1080.0, y / 1080.0));
        if let Some(g) = s.gyro {
            let g = g.scale(self.scale());
            let b = (-g.dot(up) * calib.yaw_sign, -g.dot(right) * calib.pitch_sign);
            self.learn_cursor(s.t_us, b, dt_int, s.cursor);
        }
        let use_gyro = s.gyro.is_some() && (cfg.prefer_gyro || orient.is_none());
        let (aim, twist, twist_rate_g) = if use_gyro {
            let g = s.gyro.unwrap().scale(self.scale());
            let (yaw_rate, pitch_rate) = self.room_rates(g, up, right, calib);
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

/// Wrap an angle to -pi..pi.
fn wrap_rad(a: f32) -> f32 {
    let t = std::f32::consts::TAU;
    a - t * ((a + std::f32::consts::PI) / t).floor()
}
