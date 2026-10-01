//! Minimal unit-quaternion math (`[x, y, z, w]`), kept local so this module
//! has no renderer dependency.

/// A unit quaternion, `[x, y, z, w]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat(pub [f64; 4]);

impl Quat {
    pub const IDENTITY: Self = Self([0.0, 0.0, 0.0, 1.0]);

    /// Whether finite and of unit length within 1e-6.
    #[must_use]
    pub fn is_unit(&self) -> bool {
        self.0.iter().all(|v| v.is_finite()) && (self.dot(*self) - 1.0).abs() < 1e-6
    }

    #[must_use]
    pub fn dot(self, o: Self) -> f64 {
        (0..4).map(|i| self.0[i] * o.0[i]).sum()
    }

    #[must_use]
    pub fn compose(self, o: Self) -> Self {
        let [ax, ay, az, aw] = self.0;
        let [bx, by, bz, bw] = o.0;
        Self([
            aw * bx + ax * bw + ay * bz - az * by,
            aw * by - ax * bz + ay * bw + az * bx,
            aw * bz + ax * by - ay * bx + az * bw,
            aw * bw - ax * bx - ay * by - az * bz,
        ])
    }

    #[must_use]
    pub const fn conjugate(self) -> Self {
        let [x, y, z, w] = self.0;
        Self([-x, -y, -z, w])
    }

    /// Rotates a vector.
    #[must_use]
    pub fn rotate(self, v: [f64; 3]) -> [f64; 3] {
        let p = Self([v[0], v[1], v[2], 0.0]);
        let r = self.compose(p).compose(self.conjugate()).0;
        [r[0], r[1], r[2]]
    }

    /// Shortest-arc spherical interpolation.
    #[must_use]
    pub fn slerp(self, to: Self, t: f64) -> Self {
        let mut b = to.0;
        let mut d = self.dot(to);
        if d < 0.0 {
            d = -d;
            b = b.map(|v| -v);
        }
        let a = self.0;
        if d > 1.0 - 1e-9 {
            let mut r = [0.0; 4];
            for i in 0..4 {
                r[i] = a[i] + (b[i] - a[i]) * t;
            }
            let n = r.iter().map(|v| v * v).sum::<f64>().sqrt();
            return Self(r.map(|v| v / n));
        }
        let theta = d.acos();
        let s = theta.sin();
        let (wa, wb) = (((1.0 - t) * theta).sin() / s, (t * theta).sin() / s);
        let mut r = [0.0; 4];
        for i in 0..4 {
            r[i] = a[i] * wa + b[i] * wb;
        }
        Self(r)
    }

    /// The constant world-frame angular velocity (rad per unit of `t`) of the
    /// shortest-arc slerp from `self` to `to` over `t in [0, 1]`.
    #[must_use]
    pub fn slerp_angular_velocity(self, to: Self) -> [f64; 3] {
        let mut d = to.compose(self.conjugate()).0;
        if d[3] < 0.0 {
            d = d.map(|v| -v);
        }
        let s = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if s < 1e-12 {
            return [0.0; 3];
        }
        let angle = 2.0 * s.atan2(d[3]);
        [d[0] / s * angle, d[1] / s * angle, d[2] / s * angle]
    }
}

pub fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
