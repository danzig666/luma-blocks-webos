//! The common, backend-independent motion sample. Every backend (webOS sensor2, the legacy
//! sensor API, the desktop simulator and recorded traces) is normalized into this type.
//!
//! Units are explicit and fixed: angular velocity in **rad/s**, acceleration in **m/s²**,
//! orientation as a unit quaternion (w, x, y, z), cursor in **screen pixels** of the
//! reporting service. Fields the backend did not deliver are `None`; absence is never
//! turned into zero. Remote body axes (LG documentation): X to the right, Y along the remote
//! toward its top (the long axis, pointing at the screen when aiming), Z out of the front face.

use crate::math::{Quat, Vec3};
use serde::{Deserialize, Serialize};

/// Where a sample came from. Shown in diagnostics and written into traces so recorded or
/// simulated data is never mistaken for live hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum Origin {
    /// `luna://com.webos.service.mrcu/sensor2/getSensorEventData`.
    Sensor2,
    /// `luna://com.webos.service.mrcu/sensor/getSensorData` (compatibility fallback).
    Legacy,
    /// Desktop simulator (mouse/keyboard driven).
    Simulated,
    /// Played back from a recorded trace file.
    Trace,
    /// Synthetic test generator.
    Synthetic,
}

impl Origin {
    pub fn label(self) -> &'static str {
        match self {
            Origin::Sensor2 => "Remote sensor (sensor2)",
            Origin::Legacy => "Remote sensor (legacy)",
            Origin::Simulated => "Simulated",
            Origin::Trace => "Recorded trace",
            Origin::Synthetic => "Synthetic",
        }
    }
    pub fn is_hardware(self) -> bool {
        matches!(self, Origin::Sensor2 | Origin::Legacy)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct MotionSample {
    /// Monotonic arrival time, microseconds.
    pub t_us: u64,
    /// Orientation, remote body to reference frame. Heading drifts (no magnetometer).
    pub quat: Option<Quat>,
    /// Angular velocity, body frame, rad/s (positive = counter-clockwise about the axis).
    pub gyro: Option<Vec3>,
    /// Acceleration including gravity, body frame, m/s².
    pub accel: Option<Vec3>,
    /// Acceleration with gravity removed, body frame, m/s².
    pub linear_accel: Option<Vec3>,
    /// Cursor position reported by the sensor service, screen pixels.
    pub cursor: Option<(f32, f32)>,
    /// Service-reported time since its previous callback, milliseconds.
    pub interval_ms: Option<f32>,
}

impl MotionSample {
    /// Drop fields that are not finite or physically implausible instead of passing them on.
    pub fn sanitized(mut self) -> MotionSample {
        if let Some(q) = self.quat {
            let l = q.len();
            self.quat = if q.is_finite() && (0.8..1.2).contains(&l) {
                Some(q.norm())
            } else {
                None
            };
        }
        let ok = |v: Option<Vec3>, max: f32| v.filter(|v| v.is_finite() && v.len() < max);
        self.gyro = ok(self.gyro, 100.0); // rad/s (≈5700°/s)
        self.accel = ok(self.accel, 200.0);
        self.linear_accel = ok(self.linear_accel, 200.0);
        self.cursor = self.cursor.filter(|c| c.0.is_finite() && c.1.is_finite());
        self
    }

    pub fn has_motion(&self) -> bool {
        self.quat.is_some() || self.gyro.is_some()
    }
}

/// Which fields a stream has actually delivered (cumulative), for diagnostics and for
/// deciding which gestures are honestly available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Fields {
    pub quat: bool,
    pub gyro: bool,
    pub accel: bool,
    pub linear_accel: bool,
    pub cursor: bool,
}

impl Fields {
    pub fn observe(&mut self, s: &MotionSample) {
        self.quat |= s.quat.is_some();
        self.gyro |= s.gyro.is_some();
        self.accel |= s.accel.is_some();
        self.linear_accel |= s.linear_accel.is_some();
        self.cursor |= s.cursor.is_some();
    }
    pub fn describe(&self) -> String {
        let mut v = Vec::new();
        if self.quat {
            v.push("orientation");
        }
        if self.gyro {
            v.push("gyro");
        }
        if self.accel {
            v.push("accel");
        }
        if self.linear_accel {
            v.push("linear accel");
        }
        if self.cursor {
            v.push("cursor");
        }
        if v.is_empty() {
            "none".into()
        } else {
            v.join(", ")
        }
    }
}

/// Gestures that are genuinely available given the delivered data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    /// A real wrist twist about the long axis needs orientation or gyro data.
    pub twist: bool,
    /// Circle, flick and drop can run on orientation data or on the pointer.
    pub trajectory: bool,
    /// Absolute pointing (pointer or orientation).
    pub pointing: bool,
}

/// Rolling measurement of the delivered sample rate and freshness.
#[derive(Debug, Clone, Default)]
pub struct RateMeter {
    times: std::collections::VecDeque<u64>,
    pub last_us: Option<u64>,
    pub count: u64,
    pub max_gap_us: u64,
}

impl RateMeter {
    pub fn push(&mut self, t_us: u64) {
        if let Some(l) = self.last_us {
            self.max_gap_us = self.max_gap_us.max(t_us.saturating_sub(l));
        }
        self.last_us = Some(t_us);
        self.count += 1;
        self.times.push_back(t_us);
        while self.times.len() > 2 && t_us - self.times[0] > 2_000_000 {
            self.times.pop_front();
        }
    }
    /// Samples per second over the last ~2 s.
    pub fn hz(&self, now_us: u64) -> f32 {
        if self.times.len() < 2 {
            return 0.0;
        }
        let first = self.times[0];
        let span = now_us.max(*self.times.back().unwrap()).saturating_sub(first);
        if span == 0 {
            return 0.0;
        }
        (self.times.len() - 1) as f32 * 1e6 / span as f32
    }
    /// Age of the newest sample, ms.
    pub fn age_ms(&self, now_us: u64) -> Option<f32> {
        self.last_us.map(|l| now_us.saturating_sub(l) as f32 / 1000.0)
    }
    pub fn reset(&mut self) {
        *self = RateMeter::default();
    }
}
