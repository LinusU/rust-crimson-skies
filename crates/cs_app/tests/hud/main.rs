//! Acceptance stage F46-A: instrument values and display-unit policy
//! (`specs/F46-hud-instruments-mission-map-and-pause.md`, section `### F46-A`).
//!
//! The minimum scenario is AC01's shape: a known attitude quaternion produces
//! the expected horizon and heading. The rest covers the unit policy, the
//! low-altitude hysteresis, the empty-ammunition gauge (AC02's data half) and
//! rebinding on an aircraft swap (AC03's data half). Everything is authored
//! synthetic data; no original unit, datum or threshold is read, so this proves
//! the projection only, never original display behavior (F46-B/D).

use cs_app::ui::hud::{AircraftSample, WeaponSample};
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::{ActorId, SessionId};
use cs_types::space::{Quaternion, Radians, UnitVec3};

mod attitude;
mod binding;
mod units;

pub const EPS: f64 = 1e-9;

pub fn session(n: u64) -> SessionId {
    SessionId::new(n).expect("session")
}

pub fn actor(s: u64, serial: u64) -> ActorId {
    ActorId {
        session: session(s),
        serial,
    }
}

pub fn weapon_id(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Weapon, key).expect("weapon id")
}

/// `angle` radians about `axis` (canonical axes, right-hand rule).
pub fn rot(axis: [f64; 3], angle: f64) -> Quaternion {
    Quaternion::from_axis_angle(UnitVec3::try_new(axis).expect("axis"), Radians(angle))
        .expect("rotation")
}

pub fn level_sample(s: u64, serial: u64) -> AircraftSample {
    AircraftSample {
        session: session(s),
        actor: actor(s, serial),
        attitude: Quaternion::IDENTITY,
        velocity_mps: [0.0, 0.0, -50.0],
        wind_mps: [0.0; 3],
        height_m: 500.0,
        ground_height_m: Some(0.0),
        weapon: WeaponSample {
            selected: Some(weapon_id("synthetic-gun")),
            ammunition: 100,
        },
    }
}
