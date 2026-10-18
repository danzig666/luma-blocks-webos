//! Small vector and quaternion helpers (f32). Quaternions are `(w, x, y, z)`, unit length,
//! Hamilton convention; `q.rotate(v)` maps a body-frame vector into the reference frame.

// Plain methods (not operator traits) keep the arithmetic explicit and readable here.
#![allow(clippy::should_implement_trait)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const X: Vec3 = Vec3 {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };
    pub const Y: Vec3 = Vec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    };
    pub const Z: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };
    pub fn new(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3 { x, y, z }
    }
    pub fn dot(self, o: Vec3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn scale(self, s: f32) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
    pub fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
    pub fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
    pub fn len(self) -> f32 {
        self.dot(self).sqrt()
    }
    pub fn norm(self) -> Vec3 {
        let l = self.len();
        if l > 1e-9 {
            self.scale(1.0 / l)
        } else {
            self
        }
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
    /// The coordinate axis (signed unit vector) this vector is closest to.
    pub fn dominant_axis(self) -> Vec3 {
        let (ax, ay, az) = (self.x.abs(), self.y.abs(), self.z.abs());
        if ax >= ay && ax >= az {
            Vec3::new(self.x.signum(), 0.0, 0.0)
        } else if ay >= az {
            Vec3::new(0.0, self.y.signum(), 0.0)
        } else {
            Vec3::new(0.0, 0.0, self.z.signum())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quat {
    pub w: f32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Default for Quat {
    fn default() -> Self {
        Quat::IDENTITY
    }
}

impl Quat {
    pub const IDENTITY: Quat = Quat {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    pub fn new(w: f32, x: f32, y: f32, z: f32) -> Quat {
        Quat { w, x, y, z }
    }
    pub fn from_axis_angle(axis: Vec3, rad: f32) -> Quat {
        let a = axis.norm();
        let (s, c) = (rad * 0.5).sin_cos();
        Quat::new(c, a.x * s, a.y * s, a.z * s)
    }
    pub fn vec(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
    pub fn len(self) -> f32 {
        (self.w * self.w + self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
    pub fn norm(self) -> Quat {
        let l = self.len();
        Quat::new(self.w / l, self.x / l, self.y / l, self.z / l)
    }
    pub fn conj(self) -> Quat {
        Quat::new(self.w, -self.x, -self.y, -self.z)
    }
    pub fn neg(self) -> Quat {
        Quat::new(-self.w, -self.x, -self.y, -self.z)
    }
    pub fn dot(self, o: Quat) -> f32 {
        self.w * o.w + self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn mul(self, o: Quat) -> Quat {
        Quat::new(
            self.w * o.w - self.x * o.x - self.y * o.y - self.z * o.z,
            self.w * o.x + self.x * o.w + self.y * o.z - self.z * o.y,
            self.w * o.y - self.x * o.z + self.y * o.w + self.z * o.x,
            self.w * o.z + self.x * o.y - self.y * o.x + self.z * o.w,
        )
    }
    pub fn rotate(self, v: Vec3) -> Vec3 {
        let p = Quat::new(0.0, v.x, v.y, v.z);
        self.mul(p).mul(self.conj()).vec()
    }
    /// Same rotation with a non-negative scalar part (q and -q are the same orientation).
    pub fn canonical(self) -> Quat {
        if self.w < 0.0 {
            self.neg()
        } else {
            self
        }
    }
    /// Rotation from `self` to `other` expressed in `self`'s body frame: `self^-1 * other`,
    /// sign-canonicalized so small motions give small angles.
    pub fn relative_to(self, other: Quat) -> Quat {
        self.conj().mul(other).canonical()
    }
    /// Signed angle (radians, in (-pi, pi]) of the twist component about unit `axis`
    /// (swing-twist decomposition).
    pub fn twist_angle(self, axis: Vec3) -> f32 {
        let q = self.canonical();
        let p = q.vec().dot(axis);
        wrap_pi(2.0 * p.atan2(q.w))
    }
    /// Rotation axis and angle (radians, 0..pi).
    pub fn axis_angle(self) -> (Vec3, f32) {
        let q = self.canonical();
        let s = q.vec().len();
        if s < 1e-7 {
            return (Vec3::Y, 0.0);
        }
        (q.vec().scale(1.0 / s), 2.0 * s.atan2(q.w))
    }
    pub fn is_finite(self) -> bool {
        self.w.is_finite() && self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

/// Wrap an angle to (-pi, pi].
pub fn wrap_pi(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let mut r = (a + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI;
    if r <= -std::f32::consts::PI {
        r += tau;
    }
    r
}

/// Wrap degrees to (-180, 180].
pub fn wrap_deg(a: f32) -> f32 {
    wrap_pi(a.to_radians()).to_degrees()
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub fn new(x: f32, y: f32) -> Vec2 {
        Vec2 { x, y }
    }
    pub fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
    pub fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
    pub fn scale(self, s: f32) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
    pub fn len(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn twist_angle_survives_sign_flip_and_wrap() {
        let q = Quat::from_axis_angle(Vec3::Y, 170f32.to_radians());
        assert!(close(q.twist_angle(Vec3::Y).to_degrees(), 170.0));
        assert!(close(q.neg().twist_angle(Vec3::Y).to_degrees(), 170.0));
        let q2 = Quat::from_axis_angle(Vec3::Y, 190f32.to_radians());
        assert!(close(q2.twist_angle(Vec3::Y).to_degrees(), -170.0));
    }

    #[test]
    fn twist_is_independent_of_swing() {
        let swing = Quat::from_axis_angle(Vec3::X, 0.4);
        let twist = Quat::from_axis_angle(Vec3::Y, 0.5);
        let q = swing.mul(twist);
        // Swing about a perpendicular axis adds little twist about Y.
        assert!((q.twist_angle(Vec3::Y) - 0.5).abs() < 0.05);
    }

    #[test]
    fn relative_rotation() {
        let a = Quat::from_axis_angle(Vec3::Z, 0.3);
        let b = a.mul(Quat::from_axis_angle(Vec3::Y, 0.2));
        let r = a.relative_to(b);
        let (axis, ang) = r.axis_angle();
        assert!(close(ang, 0.2));
        assert!(close(axis.y, 1.0));
    }

    #[test]
    fn wrap() {
        assert!(close(wrap_deg(190.0), -170.0));
        assert!(close(wrap_deg(-190.0), 170.0));
        assert!(close(wrap_deg(180.0), 180.0));
    }
}
