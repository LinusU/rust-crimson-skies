//! The declared-to-runtime AI combat lowering boundary (task #551).
//!
//! Spec: `specs/F32-ai-combat-formations-aces-and-difficulty.md`, stages
//! `### F32-A` to `### F32-C`. F32-A and F32-B defined the declared records
//! ([`cs_content::ai`]) and their runtime mirrors ([`cs_sim::ai::combat`]),
//! and `cs_sim` may not depend on `cs_content`, so the conversion lives here.
//!
//! The lowering is field-wise and refuses rather than guesses:
//!
//! * a [`Resolved::Unknown`] knob or weight is a [`LoweringError::UnknownValue`]
//!   naming the record, the field and the unknown's claim, never a zero;
//! * a role the rules never declared is [`LoweringError::NoProfileForRole`];
//! * an ace whose base role differs from the role it is assigned to is
//!   [`LoweringError::AceRoleMismatch`];
//! * a difficulty tier is composed onto each role profile here, once, and an
//!   ace's own overrides are applied on top of that tier's profile, so a tier
//!   never means two things (`cs_sim` never re-applies an override list).
//!
//! Everything lowered from the synthetic fixtures is newly authored project
//! design; the original's roles, knobs, tiers and formations are unmeasured
//! (F32-D).

use std::collections::BTreeMap;
use std::fmt;

use cs_content::ai as declared;
use cs_sim::ai::combat as runtime;
use cs_sim::damage::ActorId;
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;

/// Why a declared combat record could not be lowered.
#[derive(Clone, Debug, PartialEq)]
pub enum LoweringError {
    /// A needed value is [`Resolved::Unknown`].
    UnknownValue {
        /// The record carrying the value, e.g. `role escort`.
        record: String,
        /// The field, e.g. `reaction_ticks`.
        field: &'static str,
        /// The claim the unknown belongs to.
        claim_id: ClaimId,
        /// Why it is unknown.
        reason: String,
    },
    /// The rules declare no profile for a role that is needed.
    NoProfileForRole {
        /// The undeclared role.
        role: declared::DeclaredCombatRole,
    },
    /// An ace variant modifies another role than the one it is assigned to.
    AceRoleMismatch {
        /// The ace.
        ace: ContentId,
        /// The role the variant modifies.
        declared: declared::DeclaredCombatRole,
        /// The role the assignment gives the actor.
        assigned: declared::DeclaredCombatRole,
    },
    /// The rules declare no profile for a difficulty tier that was selected.
    UndeclaredTier {
        /// The tier.
        tier: declared::DifficultyTier,
    },
    /// A formation slot has no actor bound to it.
    UnboundFormationSlot {
        /// The formation.
        formation: declared::FormationId,
        /// The slot.
        slot: u32,
    },
    /// A declared override was refused when applied to a role profile.
    Override(declared::CombatSchemaError),
    /// The lowered record was refused by the runtime's own validation.
    Runtime(runtime::CombatError),
}

impl fmt::Display for LoweringError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownValue {
                record,
                field,
                claim_id,
                reason,
            } => write!(
                f,
                "{record}: `{field}` is unknown (claim {claim_id}): {reason}"
            ),
            Self::NoProfileForRole { role } => {
                write!(f, "the rules declare no profile for role {role}")
            }
            Self::AceRoleMismatch {
                ace,
                declared,
                assigned,
            } => write!(
                f,
                "ace {ace} modifies role {declared} but is assigned role {assigned}"
            ),
            Self::UndeclaredTier { tier } => {
                write!(f, "the rules declare no difficulty profile for tier {tier}")
            }
            Self::UnboundFormationSlot { formation, slot } => {
                write!(f, "formation {formation} slot {slot} has no actor bound")
            }
            Self::Override(error) => write!(f, "declared override refused: {error}"),
            Self::Runtime(error) => write!(f, "runtime refused the lowered record: {error}"),
        }
    }
}

impl std::error::Error for LoweringError {}

impl From<runtime::CombatError> for LoweringError {
    fn from(error: runtime::CombatError) -> Self {
        Self::Runtime(error)
    }
}

impl From<declared::CombatSchemaError> for LoweringError {
    fn from(error: declared::CombatSchemaError) -> Self {
        Self::Override(error)
    }
}

/// Lowers a declared role onto its runtime mirror.
#[must_use]
pub const fn lower_role(role: declared::DeclaredCombatRole) -> runtime::CombatRole {
    use declared::DeclaredCombatRole as D;
    use runtime::CombatRole as R;
    match role {
        D::FighterAttack => R::FighterAttack,
        D::BomberRun => R::BomberRun,
        D::TorpedoRun => R::TorpedoRun,
        D::Escort => R::Escort,
        D::Intercept => R::Intercept,
        D::Evade => R::Evade,
        D::Retreat => R::Retreat,
    }
}

/// Lowers a declared difficulty tier onto its runtime mirror.
#[must_use]
pub const fn lower_tier(tier: declared::DifficultyTier) -> runtime::DifficultyTier {
    use declared::DifficultyTier as D;
    use runtime::DifficultyTier as R;
    match tier {
        D::Relaxed => R::Relaxed,
        D::Standard => R::Standard,
        D::Hard => R::Hard,
        D::Elite => R::Elite,
    }
}

const fn lower_recovery(policy: declared::RecoveryPolicy) -> runtime::RecoveryAction {
    use declared::RecoveryPolicy as D;
    use runtime::RecoveryAction as R;
    match policy {
        D::Regroup => R::Regroup,
        D::ReassignLead => R::ReassignLead,
        D::HoldFormation => R::HoldFormation,
        D::Withdraw => R::Withdraw,
        D::ResumeRoute => R::ResumeRoute,
    }
}

fn need<T: Copy>(
    record: &str,
    field: &'static str,
    value: &Resolved<T>,
) -> Result<T, LoweringError> {
    match value {
        Resolved::Known(known) => Ok(known.value),
        Resolved::Unknown { claim_id, reason } => Err(LoweringError::UnknownValue {
            record: record.to_owned(),
            field,
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        }),
    }
}

/// Lowers one declared role profile, refusing every unknown value.
///
/// # Errors
///
/// [`LoweringError::UnknownValue`] naming the first unknown knob or weight,
/// and [`LoweringError::Runtime`] when the runtime rejects the lowered
/// numbers.
pub fn lower_profile(
    profile: &declared::DeclaredRoleProfile,
) -> Result<runtime::SkillProfile, LoweringError> {
    let record = format!("role {}", profile.role());
    let knobs = profile.knobs();
    let priority = profile.priority();
    let arsenal = profile.arsenal();
    Ok(runtime::SkillProfile::try_new(
        lower_role(profile.role()),
        runtime::RoleArsenal {
            gun: arsenal.gun,
            ordnance: arsenal.ordnance,
        },
        runtime::SkillKnobs {
            reaction_ticks: need(&record, "reaction_ticks", &knobs.reaction_ticks)?,
            aim_error_rad: need(&record, "aim_error_rad", &knobs.aim_error_rad)?.0,
            engagement_range_m: need(&record, "engagement_range_m", &knobs.engagement_range_m)?.0,
            fire_discipline_ticks: need(
                &record,
                "fire_discipline_ticks",
                &knobs.fire_discipline_ticks,
            )?,
        },
        runtime::PriorityPolicy {
            protected_actor_weight: need(
                &record,
                "protected_actor_weight",
                &priority.protected_actor_weight,
            )?,
            objective_weight: need(&record, "objective_weight", &priority.objective_weight)?,
            self_defense_weight: need(
                &record,
                "self_defense_weight",
                &priority.self_defense_weight,
            )?,
            proximity_weight: need(&record, "proximity_weight", &priority.proximity_weight)?,
            threat_window_ticks: need(
                &record,
                "threat_window_ticks",
                &priority.threat_window_ticks,
            )?,
        },
    )?)
}

/// Lowers every role the rules declare, with no difficulty tier composed.
///
/// # Errors
///
/// The first error of [`lower_profile`].
pub fn lower_profiles(
    rules: &declared::DeclaredCombatRules,
) -> Result<Vec<runtime::SkillProfile>, LoweringError> {
    rules.roles().iter().map(lower_profile).collect()
}

fn declared_role(
    rules: &declared::DeclaredCombatRules,
    role: declared::DeclaredCombatRole,
) -> Result<&declared::DeclaredRoleProfile, LoweringError> {
    rules
        .role(role)
        .ok_or(LoweringError::NoProfileForRole { role })
}

fn tier_profile(
    rules: &declared::DeclaredCombatRules,
    tier: declared::DifficultyTier,
) -> Result<&declared::DifficultyProfile, LoweringError> {
    rules
        .difficulty(tier)
        .ok_or(LoweringError::UndeclaredTier { tier })
}

/// The role's declared profile with a tier's overrides applied.
fn role_at_tier(
    rules: &declared::DeclaredCombatRules,
    role: declared::DeclaredCombatRole,
    tier: declared::DifficultyTier,
) -> Result<declared::DeclaredRoleProfile, LoweringError> {
    let mut profile = declared_role(rules, role)?.clone();
    for change in tier_profile(rules, tier)?.overrides() {
        profile.apply(change)?;
    }
    Ok(profile)
}

/// Lowers the difficulty roster: every declared tier, each declaring every
/// declared role with the tier's overrides composed onto it.
///
/// # Errors
///
/// [`LoweringError::Override`] for a refused override, the errors of
/// [`lower_profile`], and [`LoweringError::Runtime`] for a duplicate tier.
pub fn lower_difficulty(
    rules: &declared::DeclaredCombatRules,
) -> Result<runtime::DifficultyRoster, LoweringError> {
    let mut roster = runtime::DifficultyRoster::new();
    for tier in rules.difficulties() {
        let profiles = rules
            .roles()
            .iter()
            .map(|role| lower_profile(&role_at_tier(rules, role.role(), tier.tier())?))
            .collect::<Result<Vec<_>, _>>()?;
        roster = roster.with_tier(lower_tier(tier.tier()), &profiles)?;
    }
    Ok(roster)
}

/// Lowers one ace variant for one tier: the base role's profile, the tier's
/// overrides, then the ace's own overrides.
///
/// # Errors
///
/// [`LoweringError::NoProfileForRole`], [`LoweringError::UndeclaredTier`],
/// [`LoweringError::Override`] and the errors of [`lower_profile`].
pub fn lower_ace(
    rules: &declared::DeclaredCombatRules,
    ace: &declared::DeclaredAceProfile,
    tier: declared::DifficultyTier,
) -> Result<runtime::AceVariant, LoweringError> {
    let mut profile = role_at_tier(rules, ace.base_role(), tier)?;
    for change in ace.overrides() {
        profile.apply(change)?;
    }
    Ok(runtime::AceVariant::try_new(
        runtime::AceId::try_new(ace.id().clone())?,
        lower_role(ace.base_role()),
        lower_tier(tier),
        lower_profile(&profile)?,
    )?)
}

/// Lowers a formation's membership, binding each declared slot to an actor.
///
/// # Errors
///
/// [`LoweringError::UnboundFormationSlot`] for a slot with no actor, and
/// [`LoweringError::Runtime`] for an invalid roster.
pub fn lower_formation_roster(
    formation: &declared::DeclaredFormation,
    actors: &BTreeMap<u32, ActorId>,
) -> Result<runtime::FormationRoster, LoweringError> {
    let members = formation
        .members()
        .iter()
        .map(|member| {
            actors
                .get(&member.slot)
                .map(|actor| runtime::FormationRosterMember {
                    slot: member.slot,
                    actor: *actor,
                })
                .ok_or(LoweringError::UnboundFormationSlot {
                    formation: formation.id(),
                    slot: member.slot,
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(runtime::FormationRoster::try_new(
        lower_formation_id(formation.id()),
        formation.leader_slot(),
        members,
    )?)
}

/// Lowers a declared formation identity.
#[must_use]
pub const fn lower_formation_id(id: declared::FormationId) -> runtime::FormationId {
    runtime::FormationId(id.index())
}

/// Lowers a formation's declared recovery paths.
#[must_use]
pub const fn lower_recovery_policies(
    formation: &declared::DeclaredFormation,
) -> runtime::RecoveryPolicySet {
    let recovery = formation.recovery();
    runtime::RecoveryPolicySet {
        leader_loss: lower_recovery(recovery.leader_loss),
        assigned_target_destroyed: lower_recovery(recovery.assigned_target_destroyed),
        route_interrupted: lower_recovery(recovery.route_interrupted),
        protected_actor_lost: lower_recovery(recovery.protected_actor_lost),
    }
}

/// Lowers a mission's role assignment, refusing an ace whose base role is not
/// the assigned role.
///
/// # Errors
///
/// [`LoweringError::AceRoleMismatch`], and [`LoweringError::Runtime`] when the
/// protected actor is the actor itself.
pub fn lower_assignment(
    actor: ActorId,
    role: declared::DeclaredCombatRole,
    ace: Option<&declared::DeclaredAceProfile>,
    protected: Option<ActorId>,
) -> Result<runtime::RoleAssignment, LoweringError> {
    if let Some(ace) = ace
        && ace.base_role() != role
    {
        return Err(LoweringError::AceRoleMismatch {
            ace: ace.id().clone(),
            declared: ace.base_role(),
            assigned: role,
        });
    }
    Ok(match protected {
        Some(protected) => runtime::RoleAssignment::protecting(actor, lower_role(role), protected)?,
        None => runtime::RoleAssignment::new(actor, lower_role(role)),
    })
}

/// Lowers a whole rule set into a [`runtime::CombatRuntime`] for one session
/// generation: base profiles, the difficulty roster and every declared ace
/// lowered for `ace_tier`.
///
/// Formations are registered separately, once the mission has bound actors
/// to slots ([`lower_formation_roster`]).
///
/// # Errors
///
/// Every error of [`lower_profiles`], [`lower_difficulty`] and [`lower_ace`].
pub fn lower_runtime(
    session: u64,
    rules: &declared::DeclaredCombatRules,
    ace_tier: declared::DifficultyTier,
) -> Result<runtime::CombatRuntime, LoweringError> {
    let aces = rules
        .aces()
        .iter()
        .map(|ace| lower_ace(rules, ace, ace_tier))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(runtime::CombatRuntime::new(
        session,
        &lower_profiles(rules)?,
        aces,
        lower_difficulty(rules)?,
    )?)
}
