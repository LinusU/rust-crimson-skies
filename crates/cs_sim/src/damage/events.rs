//! Damage events: immutable hit inputs and ordered resolution outputs
//! (F29-A).
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-A`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! [`HitEvent`] is the immutable input a weapon, crash or script producer
//! hands the [`crate::damage::DamageResolver`]: who fired, which actor and
//! node it landed on, the [`DamageChannel`] it routes on and the raw damage
//! — finite and non-negative by construction, so a corrupt producer input
//! fails at its own boundary, never inside resolution.
//!
//! [`DamageEvent`] is the ordered output: one record per applied, refused
//! or blocked hit hop, per part transition, per disabled system, per
//! lifecycle transition and per kill award, each stamped with a
//! [`DamageEventId`].
//!
//! # Identity
//!
//! [`ActorId`] and [`EventId`] are the shared `IDENTITY-CONTENT` identity
//! types `cs_types::net` owns; [`HitEventId`] and [`DamageEventId`] are the
//! damage-facing names of the same [`EventId`] (task #442). Session
//! qualification is load-bearing
//! (`docs/contracts/STATE-TRANSACTIONS.md`: "Results, previous targets and
//! delayed callbacks are always generation-qualified"): a hit or event from
//! a previous session generation can never alias a live one, and the
//! nonzero [`cs_types::net::SessionId`] expresses "no session" in the type
//! rather than as a bare zero.
//!
//! # Lifecycle separation
//!
//! [`LifecycleKind`] is the vocabulary that keeps actor death, pilot
//! bailout, captured ownership, despawn and mission removal *separate*
//! (F29 non-negotiable behavior 3). They are five distinct transitions, not
//! five spellings of one "gone" flag: a bailed-out pilot did not die, a
//! captured ship changed ownership instead of being destroyed, and a
//! despawned or mission-removed actor left the world — none of them is
//! interchangeable as an objective event.

use std::fmt;

use cs_types::evidence::ClaimId;
pub use cs_types::net::{ActorId, EventId};

use super::graph::{DamageChannel, DamageNodeKey, PartState, SystemKind};

// Damage identity is the shared `IDENTITY-CONTENT` identity `cs_types::net`
// owns (task #442). `HitEventId` and `DamageEventId` are the damage-facing
// names of the one [`EventId`]: the resolver's hit inputs, its emitted
// events and any other consumer's events are the same type, so they
// compare, order and hash together and cross a crate boundary unchanged.
// The aliases keep the damage vocabulary without a second struct.
/// The identity of one [`HitEvent`]: the shared [`EventId`] applied to a
/// damage input.
///
/// `producer` is the serial of the system that emitted the hit (a weapon
/// mount, a collision reporter, a script) and `sequence` orders that
/// producer's own hits. Ordering by the full id — session, tick, producer,
/// sequence — is the declared deterministic hit order the resolver applies
/// within a tick.
pub type HitEventId = EventId;

/// The identity of one [`DamageEvent`]: the same shared [`EventId`] stamped
/// by the resolver. `producer` is the resolver's own serial, `sequence`
/// orders the events it emits.
pub type DamageEventId = EventId;

/// Why a [`HitEvent`] was rejected.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HitEventError {
    /// The raw damage was NaN or infinite.
    NonFiniteDamage,
    /// The raw damage was negative; a hit cannot heal.
    NegativeDamage {
        /// The rejected value.
        value: f64,
    },
}

impl fmt::Display for HitEventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteDamage => write!(f, "hit damage must be finite"),
            Self::NegativeDamage { value } => {
                write!(f, "hit damage {value} is negative")
            }
        }
    }
}

impl std::error::Error for HitEventError {}

/// One immutable damage input: a hit that landed on an actor's part.
///
/// The record is data only — the resolver never mutates it. `attacker` is
/// `None` for a hit with no attributable source (the world, an
/// environmental hazard); such a hit can still destroy, it simply credits
/// no one.
#[derive(Clone, Debug, PartialEq)]
pub struct HitEvent {
    /// The hit's stable identity and ordering key.
    pub id: HitEventId,
    /// The actor that caused the hit, when one is attributable.
    pub attacker: Option<ActorId>,
    /// The actor the hit landed on.
    pub target: ActorId,
    /// The damage-graph node the hit names.
    pub node: DamageNodeKey,
    /// The channel the hit routes on.
    pub channel: DamageChannel,
    /// The raw damage applied, in the graph's integrity units.
    pub damage: f64,
}

impl HitEvent {
    /// Builds a hit, refusing non-finite or negative damage.
    ///
    /// # Errors
    ///
    /// [`HitEventError::NonFiniteDamage`] or
    /// [`HitEventError::NegativeDamage`].
    pub fn try_new(
        id: HitEventId,
        attacker: Option<ActorId>,
        target: ActorId,
        node: DamageNodeKey,
        channel: DamageChannel,
        damage: f64,
    ) -> Result<Self, HitEventError> {
        if !damage.is_finite() {
            return Err(HitEventError::NonFiniteDamage);
        }
        if damage < 0.0 {
            return Err(HitEventError::NegativeDamage { value: damage });
        }
        Ok(Self {
            id,
            attacker,
            target,
            node,
            channel,
            damage,
        })
    }
}

/// The distinct lifecycle transitions an actor can go through (F29
/// non-negotiable behavior 3).
///
/// These are **not interchangeable**: destruction is a kill, bailout is a
/// live pilot leaving an airframe, capture is an ownership change, despawn
/// is the entity leaving the world and mission removal is leaving mission
/// accounting. Objectives, scoring and presentation must each consume the
/// transition they actually mean.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LifecycleKind {
    /// The actor was destroyed — the damage-caused or recorded death.
    Destroyed,
    /// The pilot bailed out; the airframe is not "dead" in the same sense
    /// and the pilot's survival is a separate question (non-negotiable 4).
    PilotBailout,
    /// The actor's ownership was captured rather than destroyed.
    OwnershipCaptured,
    /// The actor's entities left the world.
    Despawned,
    /// The actor left mission accounting.
    MissionRemoved,
}

impl LifecycleKind {
    /// Every kind, in a stable order.
    pub const ALL: &'static [LifecycleKind] = &[
        Self::Destroyed,
        Self::PilotBailout,
        Self::OwnershipCaptured,
        Self::Despawned,
        Self::MissionRemoved,
    ];

    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Destroyed => "destroyed",
            Self::PilotBailout => "pilot_bailout",
            Self::OwnershipCaptured => "ownership_captured",
            Self::Despawned => "despawned",
            Self::MissionRemoved => "mission_removed",
        }
    }

    /// Whether the transition is terminal for the actor's record: after a
    /// despawn or a mission removal nothing may be recorded for the actor
    /// again — a stale hit or a late lifecycle event of a previous
    /// generation can never resurrect it.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Despawned | Self::MissionRemoved)
    }
}

impl fmt::Display for LifecycleKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The declared rule that credits one attacker when several hits are
/// lethal in the same tick (F29 AC01: "a declared attribution rule").
///
/// The rule is *declared* — it travels in the graph's authored rules and
/// the resolver reports which rule it applied on the award — never an
/// ambient code convention. Both rules are fully deterministic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AttributionRule {
    /// Credit the attacker of the first hit, in resolved order, whose
    /// application depleted a lethal node — the killing blow.
    FirstLethalHit,
    /// Credit the attacker whose hits applied the most damage to the
    /// victim during the tick; a tie credits the attacker of the earliest
    /// contributing hit. Only attributable attackers are candidates.
    GreatestDamage,
}

impl AttributionRule {
    /// The stable label used in reports and on emitted awards.
    pub const fn label(self) -> &'static str {
        match self {
            Self::FirstLethalHit => "first_lethal_hit",
            Self::GreatestDamage => "greatest_damage",
        }
    }
}

/// Why a hit was refused inside a resolution.
///
/// A refused hit is an ordered output record, not a dropped input: the
/// resolution stays deterministic and the refusal names its reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusalReason {
    /// The hit names an actor no [`crate::damage::DamageResolver`] state
    /// was registered for.
    UnknownTargetActor,
    /// The hit names a node that does not exist in the target's graph.
    UnknownNode,
}

impl RefusalReason {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::UnknownTargetActor => "unknown_target_actor",
            Self::UnknownNode => "unknown_node",
        }
    }
}

/// What one [`DamageEvent`] records: the resolver's ordered output
/// vocabulary.
#[derive(Clone, Debug, PartialEq)]
pub enum DamageEventKind {
    /// A hit applied `applied` damage to `node`, leaving
    /// `remaining_integrity` in its pool. `applied` may be `0` when the
    /// node was already depleted — the hit landed and flowed on.
    HitApplied {
        /// The hit this hop belongs to.
        hit: HitEventId,
        /// The node that absorbed the damage.
        node: DamageNodeKey,
        /// Damage absorbed at this hop.
        applied: f64,
        /// The node's integrity after absorbing.
        remaining_integrity: f64,
    },
    /// A node's observable state changed (intact → damaged → destroyed).
    PartTransition {
        /// The node that changed.
        node: DamageNodeKey,
        /// The state before this resolution hop.
        from: PartState,
        /// The state after it.
        to: PartState,
    },
    /// A destroyed node disabled the system it carried (AC03's semantic
    /// half: the mount stops firing — the gate itself is F29-B/C).
    SystemDisabled {
        /// The node whose destruction disabled the system.
        node: DamageNodeKey,
        /// The capability that went down.
        system: SystemKind,
    },
    /// An actor lifecycle transition. Destruction is emitted by the
    /// resolver; bailout, capture, despawn and mission removal are recorded
    /// through [`crate::damage::DamageResolver::record_lifecycle`] by the
    /// session systems that own them — they are separate transitions, not
    /// aliases for death (non-negotiable 3).
    Lifecycle {
        /// The actor that transitioned.
        actor: ActorId,
        /// Which transition.
        kind: LifecycleKind,
    },
    /// The single scoring event of a destruction: one kill awarded under
    /// the victim's own declared [`AttributionRule`]. Emitted at most once
    /// per actor per session.
    KillAwarded {
        /// The destroyed actor.
        victim: ActorId,
        /// The attacker the declared rule credits — `None` when the
        /// killing hit had no attributable attacker.
        credited: Option<ActorId>,
        /// The rule the award was computed under.
        rule: AttributionRule,
        /// The hit that depleted the lethal node — the causal blow,
        /// recorded even when another attacker is credited.
        blow: HitEventId,
    },
    /// A hit reached a node whose integrity is [`Resolved`](cs_types::content::Resolved)::Unknown:
    /// nothing was applied, nothing was guessed, and the claim id and
    /// reason are surfaced so the blocked resolution is visible.
    HitBlocked {
        /// The blocked hit.
        hit: HitEventId,
        /// The claim the unknown integrity is recorded under.
        claim_id: ClaimId,
        /// Why the integrity is unknown.
        reason: String,
    },
    /// A hit was refused before any application, with its reason.
    HitRefused {
        /// The refused hit.
        hit: HitEventId,
        /// Why it was refused.
        reason: RefusalReason,
    },
}

/// One ordered resolution output, stamped with its identity.
#[derive(Clone, Debug, PartialEq)]
pub struct DamageEvent {
    /// The event's stable identity.
    pub id: DamageEventId,
    /// What happened.
    pub kind: DamageEventKind,
}
