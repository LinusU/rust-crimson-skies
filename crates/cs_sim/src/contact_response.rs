//! The original's contact response and crash damage as pure functions
//! (task #1849, part of #1752).
//!
//! Source: `docs/findings/2026-10-21-m01-collision-response.md`, a static
//! reading of the decrypted image (`0x48d7f0` response, `0x48d2c0` damage).
//! The provenance is `OWNER-STATIC`; nothing here is `verified_original` and
//! no measured trajectory was used to tune it.
//!
//! The response changes only the normal component: there is no friction and no
//! tangential impulse. Which contact of a sweep is answered (`0x48dd0a..`) and
//! the gate on `V+0x384` are the caller's business; so is not calling the
//! response for a body that is already separating (the image's caller acts on
//! `severity > 0`, `0x48ed7e`).
//!
//! Vectors are world-space `[x, y, z]` in SI units. `body_to_world` is the
//! row-major 3x3 whose columns are the body axes in world coordinates.

/// The image's linear-share scale, `0x608108`.
pub const LINEAR_SHARE_SCALE: f64 = 2.25;

/// The factor applied to the angular gain, `0x48e4b9..0x48e4f9`.
pub const ANGULAR_GAIN_FACTOR: f64 = 0.5;

/// The factor on both damages when the player's own vehicle is hit, `0x6034fc`
/// (`0x48d517`).
pub const PLAYER_DAMAGE_FACTOR: f64 = 2.0;

/// One contact to answer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    /// Unit contact normal (`n`), pointing out of the surface.
    pub normal: [f64; 3],
    /// Contact point relative to the vehicle's reference point (`r`).
    pub offset: [f64; 3],
}

/// The vehicle state the response reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactBody {
    /// Linear velocity (`v`).
    pub velocity: [f64; 3],
    /// Angular velocity (`w`).
    pub angular_velocity: [f64; 3],
    /// Row-major body-to-world rotation.
    pub body_to_world: [[f64; 3]; 3],
    /// Inertia about the three body axes.
    pub inertia: [f64; 3],
}

/// What the response yields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactOutcome {
    /// `-dot(normalize(v), n)`: 1 head-on, 0 grazing, negative when leaving;
    /// 0 for a body at rest.
    pub severity: f64,
    /// The new linear velocity, `v + Dv * (1 + bounce * linear_share)`.
    pub velocity: [f64; 3],
    /// What the angular accumulator gains,
    /// `Dw * (1 + bounce * angular_share) * 0.5`.
    pub angular_gain: [f64; 3],
    /// The linear share of the split.
    pub linear_share: f64,
    /// The angular share of the split.
    pub angular_share: f64,
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn scale(a: [f64; 3], factor: f64) -> [f64; 3] {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn length(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// The severity of a contact: `-dot(normalize(velocity), normal)`.
#[must_use]
pub fn severity(velocity: [f64; 3], normal: [f64; 3]) -> f64 {
    let speed = length(velocity);
    if speed == 0.0 {
        0.0
    } else {
        -dot(scale(velocity, 1.0 / speed), normal)
    }
}

/// Answers one contact with restitution `bounce_factor` (the `crash` record's).
#[must_use]
pub fn respond(body: &ContactBody, contact: &Contact, bounce_factor: f64) -> ContactOutcome {
    let n = contact.normal;
    let r = contact.offset;
    let severity = severity(body.velocity, n);

    // The point's response velocity: `v + 2 (r x w)` (`0x48e0be..0x48e18b`).
    let u = add(body.velocity, scale(cross(r, body.angular_velocity), 2.0));
    let approach = -dot(n, u);
    let linear_delta = scale(n, approach);

    let r_squared = dot(r, r);
    let angular_delta = if r_squared == 0.0 {
        [0.0; 3]
    } else {
        let world = scale(cross(r, n), approach / r_squared);
        let m = &body.body_to_world;
        // Body axes are the columns of `m`: body = mT * world.
        let mut local = [0.0; 3];
        for axis in 0..3 {
            local[axis] = (m[0][axis] * world[0] + m[1][axis] * world[1] + m[2][axis] * world[2])
                / body.inertia[axis];
        }
        [
            m[0][0] * local[0] + m[0][1] * local[1] + m[0][2] * local[2],
            m[1][0] * local[0] + m[1][1] * local[1] + m[1][2] * local[2],
            m[2][0] * local[0] + m[2][1] * local[1] + m[2][2] * local[2],
        ]
    };

    let k = LINEAR_SHARE_SCALE * length(linear_delta);
    let a = length(angular_delta);
    let (linear_share, angular_share) = if k == 0.0 {
        (0.0, 1.0)
    } else if a == 0.0 {
        (1.0, 0.0)
    } else {
        (k / (k + a), a / (k + a))
    };

    ContactOutcome {
        severity,
        velocity: add(
            body.velocity,
            scale(linear_delta, 1.0 + bounce_factor * linear_share),
        ),
        angular_gain: scale(
            angular_delta,
            (1.0 + bounce_factor * angular_share) * ANGULAR_GAIN_FACTOR,
        ),
        linear_share,
        angular_share,
    }
}

/// Crash damage of a contact of `severity`, from one `*_damage_range` of the
/// `crash` record: `max(range[1] * severity^3, range[0])`, doubled when the
/// player's vehicle is the one hit (`0x48d4c1..0x48d517`). What the armor and
/// health pair is subtracted from is not established.
#[must_use]
pub fn crash_damage(range: [f64; 2], severity: f64, is_player: bool) -> f64 {
    let damage = (range[1] * severity.powi(3)).max(range[0]);
    if is_player {
        damage * PLAYER_DAMAGE_FACTOR
    } else {
        damage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    const UP: [f64; 3] = [0.0, 1.0, 0.0];

    fn body(velocity: [f64; 3]) -> ContactBody {
        ContactBody {
            velocity,
            angular_velocity: [0.0; 3],
            body_to_world: IDENTITY,
            inertia: [1.0; 3],
        }
    }

    fn close(actual: [f64; 3], expected: [f64; 3]) {
        for axis in 0..3 {
            assert!(
                (actual[axis] - expected[axis]).abs() < 1e-12,
                "{actual:?} != {expected:?}"
            );
        }
    }

    #[test]
    fn accept_parity4_m01_contact_head_on_at_the_reference_point_is_point_mass_restitution() {
        // v = (0,-10,0), n = up: Dv = (0,10,0), no angular part, lin = 1,
        // v' = v + Dv * 1.6 = (0,6,0).
        let contact = Contact {
            normal: UP,
            offset: [0.0; 3],
        };
        let out = respond(&body([0.0, -10.0, 0.0]), &contact, 0.6);
        close(out.velocity, [0.0, 6.0, 0.0]);
        close(out.angular_gain, [0.0; 3]);
        assert_eq!(
            (out.severity, out.linear_share, out.angular_share),
            (1.0, 1.0, 0.0)
        );
    }

    #[test]
    fn accept_parity4_m01_contact_a_graze_keeps_the_tangent_and_reflects_the_normal() {
        // v = (10,-1,0): Dv = (0,1,0), v' = (10, -1 + 1.6, 0).
        let contact = Contact {
            normal: UP,
            offset: [0.0; 3],
        };
        let out = respond(&body([10.0, -1.0, 0.0]), &contact, 0.6);
        close(out.velocity, [10.0, 0.6, 0.0]);
        assert!((out.severity - 1.0 / 101.0_f64.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn accept_parity4_m01_contact_an_offset_point_splits_into_linear_and_angular_shares() {
        // r = (1,0,0), n = up, v = (0,-10,0): t = 10, r x n = (0,0,1),
        // Dw = (0,0,10), k = 22.5, a = 10, lin = 22.5/32.5 = 9/13,
        // v'y = 10 * 0.6 * 9/13 = 54/13, gain z = 5 * (1 + 0.6 * 4/13).
        let contact = Contact {
            normal: UP,
            offset: [1.0, 0.0, 0.0],
        };
        let out = respond(&body([0.0, -10.0, 0.0]), &contact, 0.6);
        close(out.velocity, [0.0, 54.0 / 13.0, 0.0]);
        close(out.angular_gain, [0.0, 0.0, 5.0 * (1.0 + 0.6 * 4.0 / 13.0)]);
    }

    #[test]
    fn accept_parity4_m01_contact_inertia_divides_the_angular_share_in_body_axes() {
        // Inertia z = 2: Dw = (0,0,5), k = 22.5, a = 5, lin = 9/11;
        // v'y = -10 + 10 * (1 + 0.6 * 9/11) = 54/11.
        let mut vehicle = body([0.0, -10.0, 0.0]);
        vehicle.inertia = [1.0, 1.0, 2.0];
        let contact = Contact {
            normal: UP,
            offset: [1.0, 0.0, 0.0],
        };
        let out = respond(&vehicle, &contact, 0.6);
        close(out.velocity, [0.0, 54.0 / 11.0, 0.0]);
        close(out.angular_gain, [0.0, 0.0, 2.5 * (1.0 + 0.6 * 2.0 / 11.0)]);
    }

    #[test]
    fn accept_parity4_m01_contact_the_rotation_carries_the_inertia_to_the_world_axes() {
        // A quarter turn about y: body x -> world -z, y -> y, z -> x. The
        // world torque (0,0,10) is body (-10,0,0); with inertia x = 2 it is
        // (-5,0,0), back in the world (0,0,5): the previous test's numbers.
        let mut vehicle = body([0.0, -10.0, 0.0]);
        vehicle.body_to_world = [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]];
        vehicle.inertia = [2.0, 1.0, 1.0];
        let contact = Contact {
            normal: UP,
            offset: [1.0, 0.0, 0.0],
        };
        let out = respond(&vehicle, &contact, 0.6);
        close(out.velocity, [0.0, 54.0 / 11.0, 0.0]);
        close(out.angular_gain, [0.0, 0.0, 2.5 * (1.0 + 0.6 * 2.0 / 11.0)]);
    }

    #[test]
    fn accept_parity4_m01_contact_a_spinning_point_adds_twice_its_surface_velocity() {
        // w = (0,0,1), r = (1,0,0): r x w = (0,-1,0), u = (0,-12,0), t = 12,
        // Dw = (0,0,12), k = 27, a = 12, lin = 9/13.
        let mut vehicle = body([0.0, -10.0, 0.0]);
        vehicle.angular_velocity = [0.0, 0.0, 1.0];
        let contact = Contact {
            normal: UP,
            offset: [1.0, 0.0, 0.0],
        };
        let out = respond(&vehicle, &contact, 0.6);
        close(out.velocity, [0.0, 2.0 + 12.0 * 0.6 * 9.0 / 13.0, 0.0]);
    }

    #[test]
    fn accept_parity4_m01_contact_severity_is_zero_at_rest_and_negative_when_leaving() {
        assert_eq!(severity([0.0; 3], UP), 0.0);
        assert_eq!(severity([0.0, 5.0, 0.0], UP), -1.0);
    }

    #[test]
    fn accept_parity4_m01_contact_crash_damage_has_a_floor_a_cubic_and_a_player_doubling() {
        let range = [50.0, 300.0];
        assert_eq!(crash_damage(range, 1.0, false), 300.0);
        assert_eq!(crash_damage(range, 1.0, true), 600.0);
        // 300 * 0.1^3 = 0.3 < the floor.
        assert_eq!(crash_damage(range, 0.1, false), 50.0);
        assert_eq!(crash_damage(range, 0.1, true), 100.0);
        assert_eq!(crash_damage(range, 0.5, false), 50.0_f64.max(37.5));
        assert_eq!(crash_damage(range, 0.8, false), 300.0 * 0.8_f64.powi(3));
    }
}
