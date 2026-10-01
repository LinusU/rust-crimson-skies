//! Detached payloads: inherit source motion, keep identity.

use cs_script::ir::{ActorId, SymbolId};
use cs_types::Tick;
use cs_types::content::ContentId;

use super::anchor::AnchorSample;
use super::math::add;

/// What a carrier hands over at release.
#[derive(Clone, Debug, PartialEq)]
pub struct PayloadSpec {
    /// A fresh id: never the carrier's, never reused within a session.
    pub actor: ActorId,
    pub faction: ContentId,
    /// The objective this payload counts for, if any; kept across release.
    pub objective: Option<SymbolId>,
    /// Designed extra ejection velocity in the carrier frame, m/s.
    pub eject_m_s: [f64; 3],
}

/// The detached payload's initial kinematic state.
#[derive(Clone, Debug, PartialEq)]
pub struct ReleasedPayload {
    pub actor: ActorId,
    pub faction: ContentId,
    pub objective: Option<SymbolId>,
    pub tick: Tick,
    pub position_m: [f64; 3],
    pub velocity_m_s: [f64; 3],
}

/// Releases `spec` from `anchor`: it starts at the anchor with the anchor's
/// velocity (rotational part included) plus the ejection.
#[must_use]
pub fn release_payload(anchor: &AnchorSample, spec: PayloadSpec) -> ReleasedPayload {
    let eject = anchor.orientation.rotate(spec.eject_m_s);
    ReleasedPayload {
        actor: spec.actor,
        faction: spec.faction,
        objective: spec.objective,
        tick: anchor.tick,
        position_m: anchor.position_m,
        velocity_m_s: add(anchor.velocity_m_s, eject),
    }
}
