//! What a terrain contact does besides hurting the hull (PARITY4-CRASH-D.01, #1944).
//!
//! * **Wing zone.** The contact damage is also routed to one wing region node
//!   of the player's damage graph. Which zone the original picks (the routine
//!   at `0x4b3950`) is not decoded; the *side* of the contact point in the
//!   aircraft's local x is a [`Provenance::designed`] stand-in
//!   ([`CONTACT_WING_ZONE_CLAIM`]): `x < 0` is port ([`AirframeDamageRegion::LeftWing`]),
//!   the convention measured for the wingtip flares
//!   (`docs/findings/2026-10-19-parity-m01-player-view-c2-wing-fire-smoke-source.md`).
//!   Nose and tail are never chosen; the unknown is
//!   [`CONTACT_ZONE_UNKNOWN_CLAIM`].
//! * **Crash smoke.** A fatal contact queues one [`PuffBurst`] at the crash
//!   position, which [`super::burst_puffs`] draws as the measured
//!   `large_black_smokeball` puffs.

use bevy::prelude::Vec3;
use cs_sim::damage::DamageNodeKey;
use cs_types::Tick;
use cs_types::content::{Known, Provenance};
use cs_types::evidence::ClaimId;

use super::burst_puffs::{PendingOrdnanceBursts, PuffBurst};
use crate::airframe_damage::AirframeDamageRegion;

/// The claim the designed side-of-contact wing zone rule is filed under.
pub const CONTACT_WING_ZONE_CLAIM: &str = "m01-player-contact.designed-wing-zone-side";

/// The claim the unrecovered zone selection of the original is filed under.
pub const CONTACT_ZONE_UNKNOWN_CLAIM: &str = "m01-player-contact.zone-selection-unmeasured";

/// The wing region a contact at `local_point` (aircraft-local metres, +x
/// starboard) damages. Designed: port for `x < 0`, starboard otherwise.
#[must_use]
pub fn contact_wing_zone(local_point: Vec3) -> Known<AirframeDamageRegion> {
    let region = if local_point.x < 0.0 {
        AirframeDamageRegion::LeftWing
    } else {
        AirframeDamageRegion::RightWing
    };
    Known::new(
        region,
        Provenance::designed(
            ClaimId::new(CONTACT_WING_ZONE_CLAIM).expect("a claim id this module declares"),
        ),
    )
}

/// The damage node key of a wing region.
#[must_use]
pub fn wing_node(region: AirframeDamageRegion) -> DamageNodeKey {
    DamageNodeKey::new(region.node_name()).expect("a measured region node name is a valid key")
}

/// Queues the crash smoke: one burst at the crash position.
pub fn queue_crash_smoke(pending: &mut PendingOrdnanceBursts, tick: u64, position: Vec3) {
    pending.0.push(PuffBurst {
        at: Tick(tick),
        position,
    });
}
