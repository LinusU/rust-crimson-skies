//! The `V+0xac` collision grace, written by an aircraft-into-aircraft contact
//! and fed to the ground blow's gate (#2251).
//!
//! The contact response (`0x48d31e`..`0x48d395`) sets `V+0xac = now + 1.0` on
//! **both** vehicles of a contact between two class 0/4 vehicles, and the
//! ground blow of a **non-player** is skipped while `now < V+0xac`
//! (`0x48c241`); `docs/findings/2026-11-01-groundblow-gates.md` has the bytes
//! and the law's side of the gate (`OriginalInput::collision_grace_until_s`,
//! #2222). Static claims only (`OWNER-STATIC-2026-10-29`), never
//! `verified_original`.
//!
//! The port's class 0/4 vehicles are the bodies on [`CollisionLayer::Aircraft`]
//! that carry a [`PlaytestOriginalFlight`] record: a contact report whose two
//! bodies both qualify writes [`CollisionGrace`] on both. The law keeps no
//! clock, so the driver hands it the session time and the deadline
//! ([`overlay_collision_grace`]).

use bevy::prelude::{Commands, Component, Fixed, Query, Res, Time, With};
use cs_sim::collision::{CollisionLayer, ContactKind};
use cs_sim::flight::original::OriginalInput;

use super::scene::PlaytestOriginalFlight;
use crate::physics::{ContactReports, PhysicsTickLedger};

/// How long after the contact both aircraft keep their ground blow off, in
/// seconds: the `1.0` the contact response adds to the clock (`0x48d383`,
/// `0x48d395`).
pub const COLLISION_GRACE_S: f64 = 1.0;

/// The vehicle's `V+0xac`: the session time, in seconds, until which a
/// non-player's ground blow is skipped. Absent means never set (`0.0`).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct CollisionGrace {
    /// `V+0xac`.
    pub until_s: f64,
}

/// The session clock the law reads as `[0x71c470]`: integer fixed ticks
/// converted once.
#[must_use]
pub fn session_time_s(ticks: u64, timestep_s: f64) -> f64 {
    ticks as f64 * timestep_s
}

/// Whether the vehicle is inside its grace, the `now < [V+0xac]` the contact
/// sweep opens with (`0x48d7f0` / `0x48d7fd`): it returns without searching,
/// so the vehicle's own contact reaction does not run. Symmetric: there is no
/// seat test, and a player in its grace skips its own sweep too.
#[must_use]
pub fn sweep_skipped(grace: Option<&CollisionGrace>, now_s: f64) -> bool {
    grace.is_some_and(|grace| now_s < grace.until_s)
}

/// Writes [`CollisionGrace`] on both aircraft of every aircraft-into-aircraft
/// contact reported this tick. A contact is found by either vehicle's own
/// sweep, so one whose two vehicles are both inside their grace is not seen
/// and does not renew it.
pub fn write_collision_grace(
    reports: Res<ContactReports>,
    ledger: Res<PhysicsTickLedger>,
    time: Res<Time<Fixed>>,
    flights: Query<(), With<PlaytestOriginalFlight>>,
    graces: Query<&CollisionGrace>,
    mut commands: Commands,
) {
    for report in reports.reports() {
        if report.tick != ledger.ticks
            || report.kind != ContactKind::SolidContact
            || report.layers != [CollisionLayer::Aircraft; 2]
            || !report.bodies.iter().all(|body| flights.contains(*body))
        {
            continue;
        }
        let now_s = session_time_s(report.tick, time.timestep().as_secs_f64());
        if report
            .bodies
            .iter()
            .all(|body| sweep_skipped(graces.get(*body).ok(), now_s))
        {
            continue;
        }
        let until_s = now_s + COLLISION_GRACE_S;
        for body in report.bodies {
            commands.entity(body).insert(CollisionGrace { until_s });
        }
    }
}

/// States the clock and the vehicle's grace in `input`, the two inputs of the
/// law's non-player ground-blow gate. The player is ungated by the law itself.
#[must_use]
pub fn overlay_collision_grace(
    mut input: OriginalInput,
    grace: Option<&CollisionGrace>,
    now_s: f64,
) -> OriginalInput {
    input.sim_time_s = now_s;
    input.collision_grace_until_s = grace.map_or(0.0, |grace| grace.until_s);
    input
}
