//! The pilot-roster application boundary (F33-A, F33-B).
//!
//! Spec: `specs/F33-wingmates-factions-neutral-traffic-and-pilot-identity.md`,
//! stages `### F33-A` and `### F33-B`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! This module sits between the declared roster schema
//! ([`cs_content::pilots`]) and the per-session identity store
//! ([`cs_sim::allies`]), which cannot see each other — `cs_sim` must not
//! depend on `cs_content` (`docs/01-ARCHITECTURE.md`). It contains:
//!
//! * [`lower_roster`] — the conversion boundary: a validated
//!   [`cs_content::pilots::DeclaredRoster`] becomes the
//!   [`LoweredRoster`] a session registers actors and wingmate assignments
//!   from. Every `Resolved::Unknown` **refuses** rather than guessing: an
//!   unmeasured pilot voice never becomes a random line (F33 non-negotiable
//!   5) and an unmeasured survivability never becomes a silent mortal.
//! * [`open_roster`] — the F33-B session entry: it opens an
//!   [`AlliesRoster`] from a lowered roster and the player's [`BriefingPlan`],
//!   committing the player faction and deriving the wingmate assignments from
//!   the authored records every time, so a retry rebuilds rather than
//!   carrying the failed world's state.
//! * [`RosterBinding`] — the ECS record tying an entity to its
//!   session-qualified [`cs_sim::damage::ActorId`] and the declared roster
//!   subject it was spawned under, generation-stamped like
//!   [`crate::scene::SceneNodeBinding`] so a reload can never leave a stale
//!   binding looking live.
//!
//! Nothing here owns identity state: the pilot, faction, geometry, voice and
//! survivability records are the `AlliesRoster`'s; these are the conversion
//! and binding records the ECS wiring consumes (F33-B/C).

use bevy::ecs::component::Component;
use cs_content::pilots::{DeclaredPilot, DeclaredRoster, DeclaredSurvivability, DeclaredWingmate};
use cs_sim::allies::{
    AlliesRoster, BriefingError, BriefingPlan, FactionId, GeometryId, IdentityError, PilotId,
    SurvivabilityPolicy, WingmateAssignment, WingmateSlot,
};
use cs_sim::damage::ActorId;
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;

use crate::scene::SceneGeneration;

/// One lowered pilot: its identity and the voice it speaks through.
///
/// The voice is a catalog id, resolved from the declared record's
/// `Resolved<ContentId>` — an unknown voice refuses at [`lower_roster`], so
/// the lowered record never carries a random substitute (F33 non-negotiable
/// 5).
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredPilot {
    /// The pilot's identity.
    pub id: PilotId,
    /// The voice catalog id the pilot speaks through.
    pub voice: ContentId,
}

/// One lowered neutral actor.
#[derive(Clone, Debug, PartialEq)]
pub struct NeutralTraffic {
    /// The mission-authored traffic index.
    pub traffic: u32,
    /// The pilot that flies it.
    pub pilot: PilotId,
    /// The geometry it is built from.
    pub geometry: GeometryId,
    /// The faction it starts on.
    pub faction: FactionId,
    /// The declared survivability.
    pub survivability: SurvivabilityPolicy,
}

/// What [`lower_roster`] produces: the session's player faction, its lowered
/// pilots, its wingmate assignments and its authored neutral traffic.
///
/// An omitted declared list lowers to an **empty** list, never a global
/// population default (F33 AC04's contract half): the session can only spawn
/// the neutral actors the mission authored.
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredRoster {
    /// The faction the mission's player and wingmates fly for.
    pub player_faction: FactionId,
    /// The lowered pilots, in authored order.
    pub pilots: Vec<LoweredPilot>,
    /// The lowered wingmate assignments, in authored order.
    pub wingmates: Vec<WingmateAssignment>,
    /// The lowered neutral actors, in authored order.
    pub neutral_traffic: Vec<NeutralTraffic>,
}

/// Why a declared roster could not be lowered to the runtime records.
#[derive(Clone, Debug, PartialEq)]
pub enum RosterLowerError {
    /// A pilot's voice is `Resolved::Unknown`: refused, because a random
    /// line must never stand in for a missing mission dialogue (F33
    /// non-negotiable 5).
    UnknownVoice {
        /// The pilot whose voice is unknown.
        pilot: ContentId,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the voice is unknown.
        reason: String,
    },
    /// A survivability is `Resolved::Unknown`: refused, because "unknown" is
    /// not "mortal" and an actor must not silently become killable.
    UnknownSurvivability {
        /// Which record list the value belongs to: `"wingmate"` or
        /// `"neutral_traffic"`.
        record: &'static str,
        /// The value's index in that list.
        index: usize,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the survivability is unknown.
        reason: String,
    },
    /// A referenced id was not in the namespace its field requires.
    Identity {
        /// Which record the id belongs to.
        record: &'static str,
        /// The value's index in that list.
        index: usize,
        /// The namespace error.
        error: IdentityError,
    },
    /// A wingmate names a pilot the lowered pilot list does not contain.
    /// Unreachable through `DeclaredRoster::try_new`, which refuses it
    /// earlier, but reported rather than silently skipped.
    UndeclaredPilot {
        /// Which record names the pilot.
        record: &'static str,
        /// The value's index in that list.
        index: usize,
        /// The undeclared pilot.
        pilot: ContentId,
    },
}

impl std::fmt::Display for RosterLowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownVoice {
                pilot,
                claim_id,
                reason,
            } => write!(
                f,
                "pilot {pilot} has no evidence-backed voice ({}: {reason})",
                claim_id.as_str()
            ),
            Self::UnknownSurvivability {
                record,
                index,
                claim_id,
                reason,
            } => write!(
                f,
                "{record} #{index} has an unknown survivability ({}: {reason})",
                claim_id.as_str()
            ),
            Self::Identity {
                record,
                index,
                error,
            } => write!(f, "{record} #{index} is invalid: {error}"),
            Self::UndeclaredPilot {
                record,
                index,
                pilot,
            } => write!(f, "{record} #{index} names undeclared pilot {pilot}"),
        }
    }
}

impl std::error::Error for RosterLowerError {}

fn lower_survivability(
    record: &'static str,
    index: usize,
    value: &Resolved<DeclaredSurvivability>,
) -> Result<SurvivabilityPolicy, RosterLowerError> {
    match value {
        Resolved::Known(known) => Ok(match known.value {
            DeclaredSurvivability::Mortal => SurvivabilityPolicy::Mortal,
            DeclaredSurvivability::ProtectedNeutral => SurvivabilityPolicy::ProtectedNeutral,
            DeclaredSurvivability::ScriptedInvulnerable => {
                SurvivabilityPolicy::ScriptedInvulnerable
            }
        }),
        Resolved::Unknown { claim_id, reason } => Err(RosterLowerError::UnknownSurvivability {
            record,
            index,
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        }),
    }
}

fn lower_pilot(declared: &DeclaredPilot, index: usize) -> Result<LoweredPilot, RosterLowerError> {
    let id =
        PilotId::try_new(declared.id().clone()).map_err(|error| RosterLowerError::Identity {
            record: "pilot",
            index,
            error,
        })?;
    let voice = match declared.voice() {
        Resolved::Known(known) => known.value.clone(),
        Resolved::Unknown { claim_id, reason } => {
            return Err(RosterLowerError::UnknownVoice {
                pilot: declared.id().clone(),
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            });
        }
    };
    Ok(LoweredPilot { id, voice })
}

fn lower_wingmate(
    declared: &DeclaredWingmate,
    index: usize,
    pilots: &[LoweredPilot],
) -> Result<WingmateAssignment, RosterLowerError> {
    let pilot =
        PilotId::try_new(declared.pilot().clone()).map_err(|error| RosterLowerError::Identity {
            record: "wingmate",
            index,
            error,
        })?;
    let aircraft = GeometryId::try_new(declared.airframe().clone()).map_err(|error| {
        RosterLowerError::Identity {
            record: "wingmate",
            index,
            error,
        }
    })?;
    let voice = pilots
        .iter()
        .find(|lowered| lowered.id.as_content() == declared.pilot())
        .map(|lowered| lowered.voice.clone())
        .ok_or_else(|| RosterLowerError::UndeclaredPilot {
            record: "wingmate",
            index,
            pilot: declared.pilot().clone(),
        })?;
    Ok(WingmateAssignment {
        slot: WingmateSlot(declared.slot().index()),
        pilot,
        aircraft,
        loadout: declared.loadout().clone(),
        voice,
        survivability: lower_survivability("wingmate", index, declared.survivability())?,
    })
}

/// Lowers declared roster records into the runtime records a session
/// registers actors and wingmate assignments from.
///
/// Every `Resolved::Unknown` refuses naming the field and the claim it is
/// recorded under; no value is defaulted, guessed or repaired here. The
/// neutral-traffic list lowers verbatim, so an omitted list becomes an empty
/// list rather than a population default.
///
/// # Errors
///
/// [`RosterLowerError`] on any unresolved declared value or a referenced id
/// outside its namespace.
pub fn lower_roster(declared: &DeclaredRoster) -> Result<LoweredRoster, RosterLowerError> {
    let player_faction =
        FactionId::try_new(declared.player_faction().clone()).map_err(|error| {
            RosterLowerError::Identity {
                record: "player_faction",
                index: 0,
                error,
            }
        })?;

    let mut pilots = Vec::with_capacity(declared.pilots().len());
    for (index, pilot) in declared.pilots().iter().enumerate() {
        pilots.push(lower_pilot(pilot, index)?);
    }

    let mut wingmates = Vec::with_capacity(declared.wingmates().len());
    for (index, wingmate) in declared.wingmates().iter().enumerate() {
        wingmates.push(lower_wingmate(wingmate, index, &pilots)?);
    }

    let mut neutral_traffic = Vec::with_capacity(declared.neutral_traffic().len());
    for (index, neutral) in declared.neutral_traffic().iter().enumerate() {
        let pilot = PilotId::try_new(neutral.pilot().clone()).map_err(|error| {
            RosterLowerError::Identity {
                record: "neutral_traffic",
                index,
                error,
            }
        })?;
        let geometry = GeometryId::try_new(neutral.airframe().clone()).map_err(|error| {
            RosterLowerError::Identity {
                record: "neutral_traffic",
                index,
                error,
            }
        })?;
        let faction = FactionId::try_new(neutral.faction().clone()).map_err(|error| {
            RosterLowerError::Identity {
                record: "neutral_traffic",
                index,
                error,
            }
        })?;
        neutral_traffic.push(NeutralTraffic {
            traffic: neutral.traffic(),
            pilot,
            geometry,
            faction,
            survivability: lower_survivability("neutral_traffic", index, neutral.survivability())?,
        });
    }

    Ok(LoweredRoster {
        player_faction,
        pilots,
        wingmates,
        neutral_traffic,
    })
}

/// Opens a session's ally roster from a lowered mission roster and the
/// player's briefing selection.
///
/// The roster is built from the **authored** lowered records every time: the
/// player faction is committed once, and the wingmate assignments are derived
/// from the authored assignments plus the briefing plan
/// ([`AlliesRoster::reset_wingmates`]). A retry calls this again with a new
/// `session` and the same lowered roster, so the session starts from the
/// authored state and the briefing choice rather than the failed world's
/// rearm or captures (`docs/contracts/STATE-TRANSACTIONS.md`: "Retry restores
/// the authored initial state, not a mutated copy of the just-failed world").
/// The briefing selection is a session input, so it is re-applied identically.
///
/// # Errors
///
/// [`BriefingError`] when the plan selects a slot the mission never assigned.
pub fn open_roster(
    session: u64,
    lowered: &LoweredRoster,
    plan: &BriefingPlan,
) -> Result<AlliesRoster, BriefingError> {
    let mut roster = AlliesRoster::new(session);
    roster.set_player_faction(lowered.player_faction.clone());
    roster.reset_wingmates(&lowered.wingmates, plan)?;
    Ok(roster)
}

/// Component: ties an entity to one session-qualified actor and the declared
/// roster subject it was spawned under.
///
/// `actor` is the session-qualified [`ActorId`] the `AlliesRoster` registered
/// (its `session` is the session generation), `roster` the catalog subject of
/// the [`DeclaredRoster`] the session opened with, and `generation` the scene
/// generation the binding was spawned under — so a reload stamps new bindings
/// and stale ones are identified by mismatch, never by surviving pointers (the
/// `STATE-TRANSACTIONS` session-generation discipline; the same rule
/// [`crate::scene::SceneNodeBinding`] follows).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct RosterBinding {
    /// The actor this entity presents.
    pub actor: ActorId,
    /// The catalog id of the declared roster the session runs under.
    pub roster: ContentId,
    /// The scene generation that spawned the binding.
    pub generation: SceneGeneration,
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_content::pilots::{
        DeclaredPilot, DeclaredRoster, DeclaredWingmate, WingmateSlot as DeclaredSlot,
    };
    use cs_types::content::{ContentKind, Known, Origin, Provenance};
    use cs_types::evidence::ClaimId;
    use cs_types::net::SessionId;

    fn id(kind: ContentKind, key: &str) -> ContentId {
        ContentId::from_source(kind, key).expect("test id is valid")
    }

    fn claim(key: &str) -> ClaimId {
        ClaimId::new(key).expect("test claim id is valid")
    }

    fn known<T>(value: T) -> Resolved<T> {
        Resolved::Known(Known::new(value, Provenance::designed(claim("f33a.test"))))
    }

    /// The declared synthetic roster lowers field-wise: the player faction,
    /// the two pilots with their voices, the two wingmate slots and the one
    /// neutral actor all arrive unchanged.
    #[test]
    fn accept_f33_a_lower_roster_maps_the_declared_roster() {
        let lowered = lower_roster(&cs_content::pilots::declared_synthetic_roster())
            .expect("the fixture lowers");

        assert_eq!(
            lowered.player_faction.as_content().as_str(),
            "faction/synthetic.nathan"
        );
        assert_eq!(lowered.pilots.len(), 2);
        assert_eq!(
            lowered.pilots[0].id.as_content().as_str(),
            "pilot/synthetic.nathan"
        );
        assert_eq!(lowered.pilots[0].voice.as_str(), "voice/synthetic.nathan");

        assert_eq!(lowered.wingmates.len(), 2);
        assert_eq!(lowered.wingmates[0].slot, WingmateSlot(1));
        assert_eq!(
            lowered.wingmates[0].pilot.as_content().as_str(),
            "pilot/synthetic.betty"
        );
        assert_eq!(
            lowered.wingmates[0].aircraft.as_content().as_str(),
            "airframe/synthetic.devastator"
        );
        assert_eq!(
            lowered.wingmates[0].loadout.as_str(),
            "loadout/synthetic.escort"
        );
        assert_eq!(
            lowered.wingmates[0].voice.as_str(),
            "voice/synthetic.betty",
            "the wingmate speaks through its pilot's voice"
        );
        assert_eq!(
            lowered.wingmates[0].survivability,
            SurvivabilityPolicy::Mortal
        );
        assert_eq!(
            lowered.wingmates[1].survivability,
            SurvivabilityPolicy::ScriptedInvulnerable
        );

        assert_eq!(lowered.neutral_traffic.len(), 1);
        assert_eq!(lowered.neutral_traffic[0].traffic, 1);
        assert_eq!(
            lowered.neutral_traffic[0].faction.as_content().as_str(),
            "faction/synthetic.traders"
        );
        assert_eq!(
            lowered.neutral_traffic[0].survivability,
            SurvivabilityPolicy::ProtectedNeutral
        );
    }

    /// An unknown voice refuses to lower (never a random line), and an
    /// unknown survivability refuses too (never a silent mortal).
    #[test]
    fn accept_f33_a_unknowns_refuse_to_lower() {
        let unknown_voice = DeclaredPilot::try_new(
            id(ContentKind::Pilot, "synthetic.nathan"),
            Resolved::unknown(claim("f33a.test.voice"), "no observed voice").expect("reason"),
            Origin::SyntheticFixture,
            Provenance::designed(claim("f33a.test")),
        )
        .expect("an unknown voice is a valid declared record");
        let declared = DeclaredRoster::try_new(
            id(ContentKind::Mission, "m01"),
            Origin::SyntheticFixture,
            id(ContentKind::Faction, "synthetic.nathan"),
            vec![unknown_voice],
            Vec::new(),
            Vec::new(),
            Provenance::designed(claim("f33a.test")),
        )
        .expect("the roster is valid");
        assert_eq!(
            lower_roster(&declared),
            Err(RosterLowerError::UnknownVoice {
                pilot: id(ContentKind::Pilot, "synthetic.nathan"),
                claim_id: claim("f33a.test.voice"),
                reason: "no observed voice".to_owned(),
            })
        );

        let pilot = DeclaredPilot::try_new(
            id(ContentKind::Pilot, "synthetic.nathan"),
            known(id(ContentKind::Voice, "synthetic.nathan")),
            Origin::SyntheticFixture,
            Provenance::designed(claim("f33a.test")),
        )
        .expect("the pilot is valid");
        let wingmate = DeclaredWingmate::try_new(
            DeclaredSlot(1),
            id(ContentKind::Pilot, "synthetic.nathan"),
            id(ContentKind::Airframe, "synthetic.fury"),
            id(ContentKind::Loadout, "synthetic.interceptor"),
            Resolved::unknown(claim("f33a.test.survivability"), "unmeasured escort policy")
                .expect("reason"),
            Origin::SyntheticFixture,
            Provenance::designed(claim("f33a.test")),
        )
        .expect("an unknown survivability is a valid declared record");
        let declared = DeclaredRoster::try_new(
            id(ContentKind::Mission, "m01"),
            Origin::SyntheticFixture,
            id(ContentKind::Faction, "synthetic.nathan"),
            vec![pilot],
            vec![wingmate],
            Vec::new(),
            Provenance::designed(claim("f33a.test")),
        )
        .expect("the roster is valid");
        assert_eq!(
            lower_roster(&declared),
            Err(RosterLowerError::UnknownSurvivability {
                record: "wingmate",
                index: 0,
                claim_id: claim("f33a.test.survivability"),
                reason: "unmeasured escort policy".to_owned(),
            })
        );
    }

    /// A roster that authors no neutral traffic lowers to an empty list, never
    /// a population default (F33 AC04's contract half).
    #[test]
    fn accept_f33_a_omitted_neutral_traffic_lowers_to_nothing() {
        let pilot = DeclaredPilot::try_new(
            id(ContentKind::Pilot, "synthetic.nathan"),
            known(id(ContentKind::Voice, "synthetic.nathan")),
            Origin::SyntheticFixture,
            Provenance::designed(claim("f33a.test")),
        )
        .expect("the pilot is valid");
        let declared = DeclaredRoster::try_new(
            id(ContentKind::Mission, "m01"),
            Origin::SyntheticFixture,
            id(ContentKind::Faction, "synthetic.nathan"),
            vec![pilot],
            Vec::new(),
            Vec::new(),
            Provenance::designed(claim("f33a.test")),
        )
        .expect("the roster is valid");
        let lowered = lower_roster(&declared).expect("the roster lowers");
        assert!(
            lowered.neutral_traffic.is_empty(),
            "an omitted list stays empty"
        );
    }

    /// The binding ties an entity to its session actor and roster subject
    /// under the scene generation that spawned it.
    #[test]
    fn accept_f33_a_roster_binding_is_generation_stamped() {
        let binding = RosterBinding {
            actor: ActorId {
                session: SessionId::new(3).expect("a nonzero session generation"),
                serial: 2,
            },
            roster: id(ContentKind::IaScenario, "synthetic.roster"),
            generation: SceneGeneration(4),
        };
        assert_eq!(binding.actor.session.get(), 3);
        assert_eq!(binding.roster.as_str(), "ia_scenario/synthetic.roster");
        assert_eq!(binding.generation, SceneGeneration(4));

        let stale = RosterBinding {
            generation: SceneGeneration(3),
            ..binding.clone()
        };
        assert_ne!(binding, stale, "a reload cannot alias a stale binding");
    }
}
