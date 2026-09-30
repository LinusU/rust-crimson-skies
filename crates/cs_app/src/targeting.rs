//! The targeting application boundary (F30-A).
//!
//! Spec: `specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stage `### F30-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! This module sits between the declared targeting schema
//! ([`cs_content::target_rules`]) and the session store
//! ([`cs_sim::targeting`]), which cannot see each other — `cs_sim` must
//! not depend on `cs_content` (`docs/01-ARCHITECTURE.md`). It contains:
//!
//! * [`lower_rules`] — the conversion boundary: a validated
//!   [`cs_content::target_rules::DeclaredTargetRules`] becomes the
//!   [`cs_sim::targeting::TargetPolicy`] and
//!   [`cs_sim::targeting::AllegianceTable`] a `TargetStore` opens a
//!   session with. Every `Resolved::Unknown` **refuses** rather than
//!   guessing: an unevidenced relation is not the same statement as "no
//!   relation" (the runtime reports that as `None` already), and a
//!   session never runs under a guessed threat window or assistance flag.
//! * [`TargetableBinding`] — the ECS record tying an entity to its
//!   session-qualified [`cs_sim::damage::ActorId`] and the declared rules
//!   the binding was spawned under, generation-stamped like
//!   [`crate::scene::SceneNodeBinding`] so a reload can never leave a
//!   stale binding looking live.
//!
//! Nothing here owns targeting state: the roster, relation overrides and
//! the threat ledger are the store's; these are the conversion and
//! binding records the ECS wiring consumes (F30-B/C).

use bevy::ecs::component::Component;
use cs_content::target_rules::{DeclaredAllegiance, DeclaredTargetRules, TargetRuleSet};
use cs_sim::damage::ActorId;
use cs_sim::targeting::{Allegiance, AllegianceTable, TargetPolicy};
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;

use crate::scene::SceneGeneration;

/// What [`lower_rules`] produces: the runtime policy and the allegiance
/// table a `TargetStore` opens its session with, plus the two assistance
/// options its consumers own.
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredTargetRules {
    /// The lowered policy knobs.
    pub policy: TargetPolicy,
    /// The lowered declared relations.
    pub allegiance: AllegianceTable,
    /// Whether a lead indicator is offered — a display aid only, kept
    /// separate from aim assistance (F30 non-negotiable 3).
    pub lead_indicator: bool,
    /// Whether aim assistance is offered. An option with declared
    /// evidence, never an automatic hit correction.
    pub aim_assistance: bool,
}

/// Why declared target rules could not be lowered to the runtime records.
#[derive(Clone, Debug, PartialEq)]
pub enum TargetLowerError {
    /// A `TargetRuleSet` field is `Resolved::Unknown`: no session may run
    /// targeting under a guessed rule value.
    UnknownRule {
        /// Which field is unknown.
        field: &'static str,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the value is unknown.
        reason: String,
    },
    /// A declared relation's allegiance is `Resolved::Unknown`. Refused,
    /// because lowering it to "no relation" would silently reclassify a
    /// pair the importer flagged as unmeasured — exactly the silent
    /// friendly/enemy ambiguity F30 non-negotiable 1 forbids.
    UnknownRelation {
        /// The relation's index in the declared record's list.
        relation: usize,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the allegiance is unknown.
        reason: String,
    },
}

impl std::fmt::Display for TargetLowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownRule {
                field,
                claim_id,
                reason,
            } => write!(
                f,
                "target rule {field} is unknown ({}: {reason}) and cannot be lowered",
                claim_id.as_str()
            ),
            Self::UnknownRelation {
                relation,
                claim_id,
                reason,
            } => write!(
                f,
                "declared relation #{relation} is unknown ({}: {reason}) and cannot be lowered",
                claim_id.as_str()
            ),
        }
    }
}

impl std::error::Error for TargetLowerError {}

fn lower_allegiance(allegiance: DeclaredAllegiance) -> Allegiance {
    match allegiance {
        DeclaredAllegiance::Hostile => Allegiance::Hostile,
        DeclaredAllegiance::Neutral => Allegiance::Neutral,
        DeclaredAllegiance::Friendly => Allegiance::Friendly,
    }
}

fn lower_rule<T>(field: &'static str, value: &Resolved<T>) -> Result<T, TargetLowerError>
where
    T: Clone,
{
    match value {
        Resolved::Known(known) => Ok(known.value.clone()),
        Resolved::Unknown { claim_id, reason } => Err(TargetLowerError::UnknownRule {
            field,
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        }),
    }
}

fn lower_rule_set(rules: &TargetRuleSet) -> Result<TargetPolicy, TargetLowerError> {
    Ok(TargetPolicy {
        threat_window_ticks: lower_rule("threat_window", &rules.threat_window)?,
        crosshair_cone: lower_rule("crosshair_cone", &rules.crosshair_cone)?,
    })
}

/// Lowers declared target rules into the runtime policy and allegiance
/// table a `TargetStore` opens with.
///
/// Relations map field-wise — directed pairs stay directed, the declared
/// allegiance maps onto the runtime vocabulary — and every
/// `Resolved::Unknown` refuses, so nothing is resolved, guessed or
/// repaired at this boundary. The two assistance options lower onto the
/// record for the F30-B/C consumers that own the HUD and aim paths; an
/// unknown flag fails the lowering rather than defaulting at the consumer.
///
/// # Errors
///
/// [`TargetLowerError::UnknownRule`] or
/// [`TargetLowerError::UnknownRelation`] on any unresolved declared value.
pub fn lower_rules(rules: &DeclaredTargetRules) -> Result<LoweredTargetRules, TargetLowerError> {
    let policy = lower_rule_set(rules.rules())?;
    let lead_indicator = lower_rule("lead_indicator", &rules.rules().lead_indicator)?;
    let aim_assistance = lower_rule("aim_assistance", &rules.rules().aim_assistance)?;

    let mut allegiance = AllegianceTable::new();
    for (index, relation) in rules.relations().iter().enumerate() {
        let value = match &relation.allegiance {
            Resolved::Known(known) => known.value,
            Resolved::Unknown { claim_id, reason } => {
                return Err(TargetLowerError::UnknownRelation {
                    relation: index,
                    claim_id: claim_id.clone(),
                    reason: reason.clone(),
                });
            }
        };
        allegiance.declare(
            relation.from.clone(),
            relation.to.clone(),
            lower_allegiance(value),
        );
    }

    Ok(LoweredTargetRules {
        policy,
        allegiance,
        lead_indicator,
        aim_assistance,
    })
}

/// Component: marks an entity as targetable under one session's rules.
///
/// `actor` is the session-qualified [`ActorId`] the `TargetStore`
/// registered (its `session` is the session generation), `rules` the
/// catalog subject of the `DeclaredTargetRules` the session opened with,
/// and `generation` the scene generation the binding was spawned under —
/// so a reload stamps new bindings and stale ones are identified by
/// mismatch, never by surviving pointers (the `STATE-TRANSACTIONS`
/// session-generation discipline; the same rule
/// [`crate::scene::SceneNodeBinding`] and [`crate::damage::DamageActorBinding`]
/// follow).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct TargetableBinding {
    /// The targeting actor this entity presents.
    pub actor: ActorId,
    /// The catalog id of the declared rules the session runs under.
    pub rules: ContentId,
    /// The scene generation that spawned the binding.
    pub generation: SceneGeneration,
}
