//! Synthetic remote motion for tests, the self-test and the simulator. Motion is described as
//! yaw / pitch / twist angles (degrees) of the remote over time in the documented body frame,
//! converted to orientation and body-rate samples exactly as the hardware adapter would
//! deliver them. Synthetic data is labelled as such everywhere; it is not evidence of how a
//! real remote behaves.

use crate::math::{Quat, Vec3};
use crate::sample::MotionSample;

/// Orientation for yaw (right +), pitch (up +) and clockwise twist, degrees, using the
/// documented body frame (X right, Y long axis, Z up).
pub fn pose(yaw: f32, pitch: f32, twist: f32) -> Quat {
    Quat::from_axis_angle(Vec3::Z, -yaw.to_radians())
        .mul(Quat::from_axis_angle(Vec3::X, pitch.to_radians()))
        .mul(Quat::from_axis_angle(Vec3::Y, twist.to_radians()))
}

/// Deterministic small pseudo-noise in -1..1.
fn noise(seed: u64, i: u64) -> f32 {
    let mut z = seed.wrapping_add(i.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f32 / (1u64 << 53) as f32 * 2.0 - 1.0
}

#[derive(Debug, Clone)]
pub struct Synth {
    pub hz: f32,
    pub t_us: u64,
    /// Hand tremor amplitude, degrees.
    pub tremor_deg: f32,
    pub seed: u64,
    i: u64,
    prev: Option<Quat>,
    /// Current base pose (yaw, pitch, twist), degrees.
    pub base: (f32, f32, f32),
}

impl Synth {
    pub fn new(hz: f32, seed: u64) -> Synth {
        Synth {
            hz,
            t_us: 1_000_000,
            tremor_deg: 0.15,
            seed,
            i: 0,
            prev: None,
            base: (0.0, 0.0, 0.0),
        }
    }

    fn step_us(&self) -> u64 {
        (1e6 / self.hz) as u64
    }

    /// Emit samples for `ms` milliseconds, with the pose offset given by `f(progress 0..1)`
    /// relative to the current base; the base moves to the final offset afterwards.
    pub fn motion(&mut self, ms: f32, f: impl Fn(f32) -> (f32, f32, f32)) -> Vec<MotionSample> {
        let n = ((ms / 1000.0) * self.hz).round().max(1.0) as usize;
        let mut out = Vec::with_capacity(n);
        let (by, bp, bt) = self.base;
        for k in 1..=n {
            let p = k as f32 / n as f32;
            let (dy, dp, dt) = f(p);
            out.push(self.sample(by + dy, bp + dp, bt + dt));
        }
        let (dy, dp, dt) = f(1.0);
        self.base = (by + dy, bp + dp, bt + dt);
        out
    }

    pub fn hold(&mut self, ms: f32) -> Vec<MotionSample> {
        self.motion(ms, |_| (0.0, 0.0, 0.0))
    }

    fn sample(&mut self, yaw: f32, pitch: f32, twist: f32) -> MotionSample {
        self.i += 1;
        self.t_us += self.step_us();
        let a = self.tremor_deg;
        let ph = self.t_us as f32 / 1e6 * std::f32::consts::TAU * 8.0;
        let ty = yaw + a * (ph.sin() * 0.6 + 0.4 * noise(self.seed, self.i * 3));
        let tp = pitch + a * ((ph * 1.3).cos() * 0.6 + 0.4 * noise(self.seed, self.i * 3 + 1));
        let tt = twist + a * 0.5 * noise(self.seed, self.i * 3 + 2);
        let q = pose(ty, tp, tt);
        let dt = self.step_us() as f32 / 1e6;
        let gyro = self.prev.map(|p| {
            let (axis, ang) = p.relative_to(q).axis_angle();
            axis.scale(ang / dt)
        });
        self.prev = Some(q);
        MotionSample {
            t_us: self.t_us,
            quat: Some(q),
            gyro: Some(gyro.unwrap_or_default()),
            accel: Some(q.conj().rotate(Vec3::new(0.0, 0.0, 9.81))),
            linear_accel: None,
            cursor: None,
            interval_ms: Some(dt * 1000.0),
        }
    }
}

/// Smooth 0..1 ease (cosine).
pub fn ease(p: f32) -> f32 {
    0.5 - 0.5 * (p.clamp(0.0, 1.0) * std::f32::consts::PI).cos()
}

/// Rise to `amp` and fall back to 0 over the progress range (a full there-and-back bump).
pub fn bump(p: f32) -> f32 {
    (p.clamp(0.0, 1.0) * std::f32::consts::PI).sin()
}

/// Ready-made gestures, angles in degrees, `dir` = +1 / -1.
pub mod gestures {
    use super::*;

    /// Twist out, hold, and return to neutral.
    pub fn twist(s: &mut Synth, dir: f32, amp: f32, out_ms: f32) -> Vec<MotionSample> {
        let mut v = s.motion(out_ms, |p| (0.0, 0.0, dir * amp * ease(p)));
        v.extend(s.hold(180.0));
        v.extend(s.motion(out_ms * 1.4, |p| (0.0, 0.0, -dir * amp * ease(p))));
        v.extend(s.hold(300.0));
        v
    }

    /// Circle of `radius` degrees drawn `turns` times; `dir` +1 = clockwise on screen.
    /// Starts and ends at the current aim point (the circle passes through it).
    pub fn circle(s: &mut Synth, dir: f32, radius: f32, period_ms: f32, turns: f32) -> Vec<MotionSample> {
        let ms = period_ms * turns;
        let mut v = s.motion(ms, |p| {
            let a = dir * p * turns * std::f32::consts::TAU;
            // Screen point (x, y-down) = r(cos a - 1, sin a); pitch = -y.
            let x = radius * (a.cos() - 1.0);
            let y = radius * a.sin();
            (x, -y, 0.0)
        });
        v.extend(s.hold(250.0));
        v
    }

    /// Quick yaw flick of `amp` degrees, partial recoil, settle.
    pub fn flick(s: &mut Synth, dir: f32, amp: f32, ms: f32) -> Vec<MotionSample> {
        let mut v = s.motion(ms, |p| (dir * amp * ease(p), 0.0, 0.0));
        v.extend(s.motion(160.0, |p| (-dir * amp * 0.35 * ease(p), 0.0, 0.0)));
        v.extend(s.hold(300.0));
        v
    }

    /// Sideways swing of `amp` degrees, a short pause, then the natural pull back to where
    /// it started (at `back_speed` times the outgoing speed), then rest.
    pub fn swing(s: &mut Synth, dir: f32, amp: f32, out_ms: f32, back_speed: f32) -> Vec<MotionSample> {
        let mut v = s.motion(out_ms, |p| (dir * amp * ease(p), 0.0, 0.0));
        v.extend(s.hold(120.0));
        v.extend(s.motion(out_ms / back_speed.max(0.1), |p| (-dir * amp * ease(p), 0.0, 0.0)));
        v.extend(s.hold(300.0));
        v
    }

    /// Quick downward wrist flick, upward recoil, settle.
    pub fn drop(s: &mut Synth, amp: f32, ms: f32) -> Vec<MotionSample> {
        let mut v = s.motion(ms, |p| (0.0, -amp * ease(p), 0.0));
        v.extend(s.motion(220.0, |p| (0.0, amp * 0.6 * ease(p), 0.0)));
        v.extend(s.hold(350.0));
        v
    }

    /// Idle noise with a stronger tremor.
    pub fn idle(s: &mut Synth, ms: f32) -> Vec<MotionSample> {
        let t = s.tremor_deg;
        s.tremor_deg = 0.6;
        let v = s.hold(ms);
        s.tremor_deg = t;
        v
    }
}
