//! Player calibration: the comfortable seated neutral pose, the remote's body axes as
//! verified by guided motions, and the horizontal pointing range.
//!
//! Body axes default to LG's documented frame (Y = long axis toward the screen, Z = out of
//! the front face, X = right). The guided steps measure them instead of trusting a field
//! name: "twist clockwise" finds the long axis and the clockwise sign, "tilt up" finds the
//! pitch axis. A player holding the remote in either hand performs the same physical
//! motions, so the result is hand-independent.

use crate::math::{Quat, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AimSource {
    /// Orientation from the motion sensor projected to yaw/pitch.
    #[default]
    Sensor,
    /// The system pointer (SDL mouse position).
    Pointer,
}

/// Aim x positions (SH) the player uses for the left edge, centre and right edge of the board.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PointRange {
    pub left: f32,
    pub center: f32,
    pub right: f32,
}

impl PointRange {
    pub fn is_valid(&self) -> bool {
        self.left.is_finite()
            && self.right.is_finite()
            && self.center.is_finite()
            && self.left + 0.02 < self.center
            && self.center + 0.02 < self.right
    }
    /// Continuous column (0..cols) for an aim x, piecewise linear through the centre.
    pub fn column(&self, aim_x: f32, cols: f32) -> f32 {
        let half = cols / 2.0;
        let u = if aim_x <= self.center {
            half * (aim_x - self.left) / (self.center - self.left)
        } else {
            half + half * (aim_x - self.center) / (self.right - self.center)
        };
        u.clamp(0.0, cols - 1e-3)
    }
    /// Scale the range around its centre (higher sensitivity = narrower range).
    pub fn scaled(&self, sensitivity: f32) -> PointRange {
        let s = 1.0 / sensitivity.clamp(0.5, 2.0);
        PointRange {
            left: self.center - (self.center - self.left) * s,
            center: self.center,
            right: self.center + (self.right - self.center) * s,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Calibration {
    pub version: u32,
    /// Neutral orientation (comfortable aim at the centre of the board).
    pub neutral: Option<Quat>,
    /// Long axis in the remote's body frame (pointing toward the screen).
    pub long_axis: Vec3,
    /// Up axis in the body frame (out of the front face when held normally).
    pub up_axis: Vec3,
    /// +1 when a positive rotation about `long_axis` is clockwise as the player sees it.
    pub twist_sign: f32,
    /// Whether the axes above were measured by the guided steps (vs documented defaults).
    pub axes_measured: bool,
    /// Player overrides from the calibration test ("feels reversed"): +1 or -1.
    pub yaw_sign: f32,
    pub pitch_sign: f32,
    pub sensor_range: Option<PointRange>,
    pub pointer_range: Option<PointRange>,
}

impl Default for Calibration {
    fn default() -> Self {
        Calibration {
            version: 1,
            neutral: None,
            long_axis: Vec3::Y,
            up_axis: Vec3::Z,
            // Right-hand rotation about an axis pointing away from the viewer appears
            // clockwise to that viewer.
            twist_sign: 1.0,
            axes_measured: false,
            yaw_sign: 1.0,
            pitch_sign: 1.0,
            sensor_range: None,
            pointer_range: None,
        }
    }
}

impl Calibration {
    pub fn right_axis(&self) -> Vec3 {
        self.long_axis.cross(self.up_axis)
    }

    /// Reject inconsistent axes from a corrupt file (they must be orthogonal unit axes).
    pub fn sanitize(&mut self) {
        let ok = |v: Vec3| v.is_finite() && (v.len() - 1.0).abs() < 0.05;
        if !ok(self.long_axis) || !ok(self.up_axis) || self.long_axis.dot(self.up_axis).abs() > 0.1 {
            let d = Calibration::default();
            self.long_axis = d.long_axis;
            self.up_axis = d.up_axis;
            self.twist_sign = d.twist_sign;
            self.axes_measured = false;
        }
        for s in [&mut self.twist_sign, &mut self.yaw_sign, &mut self.pitch_sign] {
            if *s != 1.0 && *s != -1.0 {
                *s = 1.0;
            }
        }
        if let Some(q) = self.neutral {
            if !q.is_finite() || (q.len() - 1.0).abs() > 0.1 {
                self.neutral = None;
            }
        }
        if self.sensor_range.is_some_and(|r| !r.is_valid()) {
            self.sensor_range = None;
        }
        if self.pointer_range.is_some_and(|r| !r.is_valid()) {
            self.pointer_range = None;
        }
    }
}

/// Accumulates one guided rotation step (from a start pose to the furthest pose reached)
/// and reports the body-frame rotation axis.
#[derive(Debug, Clone, Default)]
pub struct AxisProbe {
    start: Option<Quat>,
    best: Option<(Quat, f32)>,
}

impl AxisProbe {
    pub fn push(&mut self, q: Quat) {
        let start = *self.start.get_or_insert(q);
        let (_, ang) = start.relative_to(q).axis_angle();
        if self.best.is_none_or(|(_, a)| ang > a) {
            self.best = Some((q, ang));
        }
    }
    /// Degrees of rotation reached so far.
    pub fn angle_deg(&self) -> f32 {
        self.best.map(|b| b.1.to_degrees()).unwrap_or(0.0)
    }
    /// Signed dominant body axis of the rotation, if the motion was large and clean enough:
    /// at least 25 degrees, with at least 75% of it about a single body axis.
    pub fn axis(&self) -> Option<Vec3> {
        self.axis_min(25.0)
    }

    pub fn axis_min(&self, min_deg: f32) -> Option<Vec3> {
        let (q, ang) = self.best?;
        if ang.to_degrees() < min_deg {
            return None;
        }
        let (axis, _) = self.start?.relative_to(q).axis_angle();
        let d = axis.dominant_axis();
        if axis.dot(d) < 0.75 {
            return None;
        }
        Some(d)
    }
}

/// One guided calibration motion, measured from the gyroscope when available (a body-frame
/// rate, independent of the orientation's reference convention) and from the orientation
/// otherwise. The rotation axis is taken at the moment of largest excursion, so an
/// out-and-back motion still reads correctly.
#[derive(Debug, Clone, Default)]
pub struct Probe {
    acc: Vec3,
    best: Vec3,
    quat: AxisProbe,
    gyro_seen: bool,
}

impl Probe {
    /// `gyro_dps`: body rates in degrees per second.
    pub fn push(&mut self, gyro_dps: Option<Vec3>, quat: Option<Quat>, dt: f32) {
        if let Some(g) = gyro_dps {
            self.gyro_seen = true;
            self.acc = self.acc.add(g.scale(dt));
            if self.acc.len() > self.best.len() {
                self.best = self.acc;
            }
        }
        if let Some(q) = quat {
            self.quat.push(q);
        }
    }
    /// Largest rotation reached so far, degrees.
    pub fn angle_deg(&self) -> f32 {
        if self.gyro_seen {
            self.best.len()
        } else {
            self.quat.angle_deg()
        }
    }
    /// Current excursion along the best axis, degrees (falls back as the motion returns).
    pub fn current_deg(&self) -> f32 {
        if self.gyro_seen && self.best.len() > 1e-3 {
            self.acc.dot(self.best.norm())
        } else {
            self.quat.angle_deg()
        }
    }
    /// Signed dominant body axis, if the motion was at least `min_deg` and clean (>= 75%
    /// about one axis).
    pub fn axis(&self, min_deg: f32) -> Option<Vec3> {
        if !self.gyro_seen {
            return self.quat.axis_min(min_deg);
        }
        if self.best.len() < min_deg {
            return None;
        }
        let a = self.best.norm();
        let d = a.dominant_axis();
        (a.dot(d) >= 0.75).then_some(d)
    }
}

/// Apply the "swing right" step: swinging right is a negative rotation about the up axis
/// (right-hand rule), so up = -measured axis. Rejected if the player twisted instead.
pub fn apply_swing_probe(c: &mut Calibration, signed_axis: Vec3) -> bool {
    if signed_axis.dot(c.long_axis).abs() > 0.5 {
        return false;
    }
    c.up_axis = signed_axis.scale(-1.0).dominant_axis();
    c.axes_measured = true;
    true
}

/// Apply the "twist clockwise" step: the dominant axis is the long axis, and its sign says
/// which direction is clockwise for this player.
pub fn apply_twist_probe(c: &mut Calibration, signed_axis: Vec3) {
    let unsigned = Vec3::new(signed_axis.x.abs(), signed_axis.y.abs(), signed_axis.z.abs());
    let sign = signed_axis.x + signed_axis.y + signed_axis.z;
    c.long_axis = unsigned;
    c.twist_sign = if sign >= 0.0 { 1.0 } else { -1.0 };
}

/// Apply the "tilt the tip up" step: the measured axis is the right axis (raising the tip
/// is a positive rotation about it), giving up = right x long.
pub fn apply_pitch_probe(c: &mut Calibration, signed_axis: Vec3) -> bool {
    if signed_axis.dot(c.long_axis).abs() > 0.5 {
        return false; // the player twisted instead of tilting
    }
    let up = signed_axis.cross(c.long_axis).norm();
    c.up_axis = up.dominant_axis();
    c.axes_measured = true;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_finds_documented_axes() {
        let mut c = Calibration::default();
        let base = Quat::from_axis_angle(Vec3::new(0.3, 0.1, 1.0), 0.7);
        // Clockwise twist as seen by the player = positive about +Y.
        let mut p = AxisProbe::default();
        for i in 0..=20 {
            p.push(base.mul(Quat::from_axis_angle(Vec3::Y, (i as f32 * 2.5).to_radians())));
        }
        apply_twist_probe(&mut c, p.axis().unwrap());
        assert_eq!(c.long_axis, Vec3::Y);
        assert_eq!(c.twist_sign, 1.0);
        // Tip up = positive about +X.
        let mut p = AxisProbe::default();
        for i in 0..=20 {
            p.push(base.mul(Quat::from_axis_angle(Vec3::X, (i as f32 * 2.0).to_radians())));
        }
        assert!(apply_pitch_probe(&mut c, p.axis().unwrap()));
        assert_eq!(c.up_axis, Vec3::Z);
    }

    #[test]
    fn gyro_probe_finds_twist_and_swing_axes() {
        let mut c = Calibration::default();
        // Clockwise twist (+Y at 200 dps for 0.2 s) and back.
        let mut p = Probe::default();
        for i in 0..40 {
            let w = if i < 20 { 200.0 } else { -200.0 };
            p.push(Some(Vec3::new(3.0, w, -4.0)), None, 0.01);
        }
        assert!(p.angle_deg() > 35.0);
        apply_twist_probe(&mut c, p.axis(25.0).unwrap());
        assert_eq!(c.long_axis, Vec3::Y);
        assert_eq!(c.twist_sign, 1.0);
        // Swing right: negative rotation about +Z (up).
        let mut p = Probe::default();
        for _ in 0..20 {
            p.push(Some(Vec3::new(0.0, 5.0, -180.0)), None, 0.01);
        }
        assert!(apply_swing_probe(&mut c, p.axis(6.0).unwrap()));
        assert_eq!(c.up_axis, Vec3::Z);
        // A twist during the swing step is rejected.
        let mut p = Probe::default();
        for _ in 0..20 {
            p.push(Some(Vec3::new(0.0, 180.0, 0.0)), None, 0.01);
        }
        assert!(!apply_swing_probe(&mut c, p.axis(6.0).unwrap()));
    }

    #[test]
    fn mirrored_hardware_axes_are_learned() {
        // A firmware that reports the long axis as -Y still yields a correct mapping.
        let mut c = Calibration::default();
        let mut p = AxisProbe::default();
        for i in 0..=20 {
            p.push(Quat::from_axis_angle(
                Vec3::new(0.0, -1.0, 0.0),
                (i as f32 * 2.5).to_radians(),
            ));
        }
        apply_twist_probe(&mut c, p.axis().unwrap());
        assert_eq!(c.long_axis, Vec3::Y);
        assert_eq!(c.twist_sign, -1.0);
    }

    #[test]
    fn small_or_mixed_motion_is_rejected() {
        let mut p = AxisProbe::default();
        for i in 0..=10 {
            p.push(Quat::from_axis_angle(Vec3::Y, (i as f32).to_radians()));
        }
        assert!(p.axis().is_none());
        let mut p = AxisProbe::default();
        for i in 0..=20 {
            p.push(Quat::from_axis_angle(
                Vec3::new(1.0, 1.0, 0.0),
                (i as f32 * 2.5).to_radians(),
            ));
        }
        assert!(p.axis().is_none());
    }

    #[test]
    fn range_mapping_and_sanitize() {
        let r = PointRange {
            left: -0.3,
            center: 0.0,
            right: 0.5,
        };
        assert!((r.column(0.0, 10.0) - 5.0).abs() < 1e-4);
        assert!((r.column(-0.3, 10.0)).abs() < 1e-4);
        assert!((r.column(0.25, 10.0) - 7.5).abs() < 1e-4);
        assert!(r.column(9.0, 10.0) < 10.0);
        let mut c = Calibration {
            long_axis: Vec3::new(5.0, 0.0, 0.0),
            sensor_range: Some(PointRange {
                left: 1.0,
                center: 0.0,
                right: -1.0,
            }),
            ..Default::default()
        };
        c.sanitize();
        assert_eq!(c.long_axis, Vec3::Y);
        assert!(c.sensor_range.is_none());
    }
}
