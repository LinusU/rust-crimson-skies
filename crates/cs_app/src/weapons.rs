//! The weapon application boundary (F27-A).
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, stage
//! `### F27-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! This module sits between the declared weapon schema
//! ([`cs_content::weapons`]) and the session resolver
//! ([`cs_sim::weapons`]), which cannot see each other — `cs_sim` must not
//! depend on `cs_content` (`docs/01-ARCHITECTURE.md`). It contains:
//!
//! * [`lower_gun`] — the conversion boundary: a validated
//!   [`cs_content::weapons::DeclaredGunDefinition`] becomes the runtime
//!   [`cs_sim::weapons::GunDefinition`] the [`FireResolver`] registers.
//!   Every `Resolved::Unknown` **refuses** rather than guessing: a session
//!   must not fire a gun whose muzzle velocity, cadence, spread or damage
//!   was invented, so the refusal names the field and carries the claim.
//! * [`lower_rules`] — the interaction-rules boundary: the declared
//!   self-hit, friendly-fire, penetration, ricochet and ammo-switching
//!   options become the runtime [`cs_sim::weapons::WeaponRules`], again
//!   refusing each unknown by name (F27 non-negotiable 4).
//! * [`WeaponActorBinding`] — the ECS record tying an entity to its
//!   session-qualified [`cs_sim::damage::ActorId`], its lowered guns and
//!   the declared loadout they came from, generation-stamped like
//!   [`crate::scene::SceneNodeBinding`] so a reload can never leave a stale
//!   binding looking live.
//! * [`resolve_swept_damage`] — the F27-C application consumer: it runs one
//!   accepted shot's swept contacts through the declared rules, the swept
//!   geometry and the [`GunHitRouter`], and hands the resulting
//!   [`HitEvent`]s to the authoritative [`DamageResolver`], so a gun's
//!   declared per-channel damage becomes applied damage through the one
//!   authority that owns it. Every refusal is returned, never swallowed.
//!
//! Nothing here owns weapon state: the selection, cooldowns, ammunition and
//! disabled mounts are the [`FireResolver`]'s; these are the conversion,
//! binding and application records the ECS wiring consumes (F27-B/C).
//!
//! [`FireResolver`]: cs_sim::weapons::FireResolver
//! [`GunHitRouter`]: cs_sim::weapons::GunHitRouter
//! [`DamageResolver`]: cs_sim::damage::DamageResolver
//! [`HitEvent`]: cs_sim::damage::HitEvent

use bevy::ecs::component::Component;
use cs_content::scene::SceneNodeId;
use cs_content::weapons::{
    AmmunitionId as DeclaredAmmunitionId, DeclaredAmmunition, DeclaredFriendlyFireRule,
    DeclaredGunDefinition, DeclaredGunMountKind, DeclaredInheritanceRule, DeclaredSelfHitRule,
    DeclaredSpreadCone, DeclaredWeaponDamage, InteractionRules,
};
use cs_sim::damage::{
    ActorId, DamageError, DamageNodeKey, DamageResolver, NodeKeyError, TickResolution,
};
use cs_sim::weapons::{
    AmmunitionId, AmmunitionIdError, FireEvent, FriendlyFireRule, GunDefinition,
    GunDefinitionError, GunHitRouter, GunMountKind, GunRate, InheritanceRule, ProjectileSegment,
    SelfHitRule, SpreadCone, SweepCandidate, SweepOutcome, WeaponDamage, WeaponRules,
};
use cs_types::Tick;
use cs_types::content::{ContentId, Known, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::Radians;

use crate::scene::SceneGeneration;

/// Why a declared weapon record could not be lowered to the runtime records.
#[derive(Clone, Debug, PartialEq)]
pub enum WeaponLowerError {
    /// A declared mount key could not form a runtime key. Retained for
    /// signature compatibility: declared and runtime keys are now the one
    /// shared [`cs_types::content::DamageNodeKey`], so this is unreachable.
    MountKey {
        /// The declared key text.
        key: String,
        /// Why the runtime refused it.
        source: NodeKeyError,
    },
    /// A declared ammunition id could not form a runtime id — a
    /// wrong-namespace id slipped through the declared schema.
    AmmunitionId {
        /// The declared id text.
        id: String,
        /// Why the runtime refused it.
        source: AmmunitionIdError,
    },
    /// A field of the declared gun is `Resolved::Unknown`: no session may
    /// fire a gun whose ballistic parameter was invented.
    UnknownField {
        /// Which field is unknown.
        field: &'static str,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the value is unknown.
        reason: String,
    },
    /// The runtime refused the assembled definition.
    Definition(GunDefinitionError),
}

impl std::fmt::Display for WeaponLowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MountKey { key, source } => {
                write!(f, "mount key {key:?} cannot be lowered: {source}")
            }
            Self::AmmunitionId { id, source } => {
                write!(f, "ammunition id {id:?} cannot be lowered: {source}")
            }
            Self::UnknownField {
                field,
                claim_id,
                reason,
            } => write!(
                f,
                "weapon field {field} is unknown ({}: {reason}) and cannot be lowered",
                claim_id.as_str()
            ),
            Self::Definition(source) => {
                write!(f, "the runtime refused the lowered gun: {source}")
            }
        }
    }
}

impl std::error::Error for WeaponLowerError {}

/// Lowers a declared gun definition into the runtime one a
/// [`cs_sim::weapons::FireResolver`] registers.
///
/// The conversion is field-wise and refuses every unknown by name. Mount
/// keys map by text, the mount kind and inheritance rule map variant-wise,
/// and a declared `SceneNodeId` binding is *not* part of the runtime gun:
/// the mount transform is read from the live aircraft hierarchy by F27-B, so
/// carrying the visual binding into the runtime record would smuggle a
/// presentation reference into gameplay state. It stays available on the
/// declared record for F27-B to read there.
pub fn lower_gun(gun: &DeclaredGunDefinition) -> Result<GunDefinition, WeaponLowerError> {
    let mount = lower_mount_key(gun.mount())?;
    let caliber = known_or_refuse("caliber", gun.caliber())?;
    let ammunition = lower_ammunition_id(gun.ammunition())?;
    let rate = known_or_refuse("rate", gun.rate())?;
    let muzzle_velocity = known_or_refuse("muzzle_velocity_mps", gun.muzzle_velocity_mps())?;
    let lifetime = known_or_refuse("lifetime_ticks", gun.lifetime_ticks())?;
    let spread = lower_spread(gun.spread())?;
    let damage = lower_damage(gun.damage())?;
    let inheritance = lower_inheritance(gun.inheritance())?;
    let effect = known_or_refuse("effect", gun.effect())?;
    let sound = known_or_refuse("sound", gun.sound())?;

    GunDefinition::try_new(
        mount,
        lower_mount_kind(gun.mount_kind()),
        caliber.as_str(),
        ammunition,
        GunRate::try_new(rate.ticks_between_shots).map_err(WeaponLowerError::Definition)?,
        muzzle_velocity,
        lifetime,
        spread,
        damage,
        inheritance,
        effect,
        sound,
    )
    .map_err(WeaponLowerError::Definition)
}

/// Lowers the declared interaction rules into the runtime
/// [`WeaponRules`] a session filters a sweep's candidates with.
///
/// Every option refuses independently: an unmeasured self-hit rule does not
/// become "excluded" and an unmeasured penetration rule does not become
/// "false" — no session runs a gun under a guessed interaction rule
/// (F27 non-negotiable 4).
///
/// The three options that **no production code applies** — `penetration`,
/// `ricochet` and `ammo_switching` — are carried here too, because the
/// declared record is their only home and dropping them at the boundary would
/// make the declared schema and the runtime disagree about what a gun carries.
/// Their deferral to F27-D, and why none of them is applied, is reported by
/// `cs_content::weapons::InteractionRules::deferred` rather than only in a
/// findings file.
///
/// # Errors
///
/// [`WeaponLowerError::UnknownField`] naming the first unresolved option.
pub fn lower_rules(rules: &InteractionRules) -> Result<WeaponRules, WeaponLowerError> {
    let self_hit = known_or_refuse("rules.self_hit", &rules.self_hit)?;
    let friendly_fire = known_or_refuse("rules.friendly_fire", &rules.friendly_fire)?;
    let penetration = known_or_refuse("rules.penetration", &rules.penetration)?;
    let ricochet = known_or_refuse("rules.ricochet", &rules.ricochet)?;
    let ammo_switching = known_or_refuse("rules.ammo_switching", &rules.ammo_switching)?;
    Ok(WeaponRules {
        self_hit: match self_hit {
            DeclaredSelfHitRule::Excluded => SelfHitRule::Excluded,
            DeclaredSelfHitRule::Allowed => SelfHitRule::Allowed,
        },
        friendly_fire: match friendly_fire {
            DeclaredFriendlyFireRule::HostileOnly => FriendlyFireRule::HostileOnly,
            DeclaredFriendlyFireRule::NonFriendly => FriendlyFireRule::NonFriendly,
            DeclaredFriendlyFireRule::Everyone => FriendlyFireRule::Everyone,
        },
        penetration,
        ricochet,
        ammo_switching,
    })
}

/// Lowers a declared ammunition record into the runtime
/// [`AmmunitionId`] it names.
///
/// The id is the whole of what the runtime carries: a declared
/// ammunition's damage profile belongs to the gun that is loaded with it,
/// and F27 non-negotiable 1 forbids deriving one from a multiplier table.
/// The declared record stays where the audit reads it.
///
/// # Errors
///
/// [`WeaponLowerError::AmmunitionId`] when the declared id is not in the
/// `ammo` namespace.
pub fn lower_ammunition(ammunition: &DeclaredAmmunition) -> Result<AmmunitionId, WeaponLowerError> {
    AmmunitionId::try_new(ammunition.ammunition().id().clone()).map_err(|source| {
        WeaponLowerError::AmmunitionId {
            id: ammunition.ammunition().as_str().to_owned(),
            source,
        }
    })
}

/// A mount's live world transform, resolved from the aircraft hierarchy.
///
/// F27 non-negotiable 2 requires the mount transform to come from the live
/// hierarchy and damage state rather than a fixed center-screen origin.
/// This record is what F27-B's hierarchy walk produces: the muzzle's world
/// pose and the mount's forward axis at the tick being resolved, together
/// with the scene generation it was read under so a reloaded hierarchy's
/// stale poses are identifiable by mismatch.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct MountPoseBinding {
    /// The mount this pose belongs to, by damage-node key.
    pub mount: DamageNodeKey,
    /// The scene generation the hierarchy was read under.
    pub generation: SceneGeneration,
}

/// Component: marks an entity as the visual face of one weapon actor.
///
/// `actor` is the session-qualified [`ActorId`] the [`FireResolver`]
/// registered weapons for, `guns` the catalog ids the runtime definitions
/// were lowered from, `loadout` the declared loadout they came from, and
/// `generation` the scene generation that spawned the binding — so a reload
/// stamps new bindings and stale ones are identified by mismatch, never by
/// surviving pointers (the same rule
/// [`crate::damage::DamageActorBinding`] and
/// [`crate::targeting::TargetableBinding`] follow).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct WeaponActorBinding {
    /// The fire resolver actor this entity presents.
    pub actor: ActorId,
    /// The `weapon` catalog ids this actor's guns were lowered from.
    pub guns: Vec<ContentId>,
    /// The declared loadout the guns came from.
    pub loadout: ContentId,
    /// The scene generation that spawned the binding.
    pub generation: SceneGeneration,
}

fn lower_mount_key(
    key: &cs_content::damage::DamageNodeKey,
) -> Result<DamageNodeKey, WeaponLowerError> {
    // The declared and runtime mount keys are now one shared
    // `cs_types::content::DamageNodeKey`, so lowering is an identity map.
    // The `Result` is kept for the boundary's existing signatures; this
    // cannot fail.
    Ok(key.clone())
}

fn lower_mount_kind(kind: DeclaredGunMountKind) -> GunMountKind {
    match kind {
        DeclaredGunMountKind::Nose => GunMountKind::Nose,
        DeclaredGunMountKind::WingLeft => GunMountKind::WingLeft,
        DeclaredGunMountKind::WingRight => GunMountKind::WingRight,
        DeclaredGunMountKind::Tail => GunMountKind::Tail,
        DeclaredGunMountKind::Gondola => GunMountKind::Gondola,
    }
}

fn lower_ammunition_id(
    resolved: &Resolved<DeclaredAmmunitionId>,
) -> Result<AmmunitionId, WeaponLowerError> {
    let declared = known_or_refuse("ammunition", resolved)?;
    AmmunitionId::try_new(declared.id().clone()).map_err(|source| WeaponLowerError::AmmunitionId {
        id: declared.as_str().to_owned(),
        source,
    })
}

fn lower_spread(spread: &DeclaredSpreadCone) -> Result<SpreadCone, WeaponLowerError> {
    let half_angle: Radians = known_or_refuse("spread.half_angle", &spread.half_angle)?;
    SpreadCone::try_new(half_angle.0).map_err(WeaponLowerError::Definition)
}

fn lower_damage(damage: &DeclaredWeaponDamage) -> Result<WeaponDamage, WeaponLowerError> {
    let armor = known_or_refuse("damage.armor", &damage.armor)?;
    let internal = known_or_refuse("damage.internal", &damage.internal)?;
    WeaponDamage::try_new(armor, internal).map_err(WeaponLowerError::Definition)
}

fn lower_inheritance(
    inheritance: &Resolved<DeclaredInheritanceRule>,
) -> Result<InheritanceRule, WeaponLowerError> {
    let declared = known_or_refuse("inheritance", inheritance)?;
    Ok(match declared {
        DeclaredInheritanceRule::Full => InheritanceRule::Full,
        DeclaredInheritanceRule::Fraction { share } => InheritanceRule::Fraction { share },
        DeclaredInheritanceRule::None => InheritanceRule::None,
    })
}

/// Unwraps a declared [`Resolved`] value, refusing an unknown with its
/// claim and reason rather than substituting a default.
fn known_or_refuse<T: Clone>(
    field: &'static str,
    resolved: &Resolved<T>,
) -> Result<T, WeaponLowerError> {
    match resolved {
        Resolved::Known(Known { value, .. }) => Ok(value.clone()),
        Resolved::Unknown { claim_id, reason } => Err(WeaponLowerError::UnknownField {
            field,
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        }),
    }
}

/// The [`SceneNodeId`] a declared gun's visual mount binding names, when it
/// declares one.
///
/// F27-B reads the live hierarchy through this id; the weapon runtime itself
/// never sees it, because gameplay state must not depend on a presentation
/// reference.
#[must_use]
pub fn declared_scene_binding(gun: &DeclaredGunDefinition) -> Option<&Resolved<SceneNodeId>> {
    gun.scene_binding()
}

// ---------------------------------------------- the swept-hit → damage seam ---
//
// F27-C. The gun half (`cs_sim::weapons::guns`) owns the *shape* of a swept
// hit: [`GunHitRouter::route`] filters candidates by the declared rules,
// sweeps and converts each contact into one `HitEvent` per non-zero declared
// damage channel. It deliberately does not apply anything — `cs_sim::damage`
// is the only authority that may turn a hit into destroyed structure.
//
// This function is the production caller that closes the loop: it takes the
// routed hits and resolves them through the session's [`DamageResolver`].
// Nothing else in the codebase consumes a `RoutedHit`, so without this the
// sweep would compute a hit nobody could act on.
//
// # Ordering and failure
//
// The routing runs first and produces the whole batch, which is then handed
// to the resolver in one call. Routing first means the resolver sees a
// *complete* tick's damage from this shot, so its same-tick lethal and
// attribution rules apply across the whole batch rather than per shot.
//
// The resolver's error is returned, not swallowed: a `ForeignSession` or a
// `DuplicateHit` means the batch was wrong before anything was applied, and
// the routing outcome is still returned alongside so the caller can see which
// contacts produced it. A routing refusal does **not** stop the batch: the
// other contacts' damage is still the gun's declared damage and is still
// applied, because a refused contact is a reported defect in one candidate,
// not a licence to drop the rest of the round's damage.
//
// # What this does not do
//
// It does not select the bank, play the sound, spawn the muzzle effect or
// move a mount's transform: the accepted fire event already carries the sound
// and effect ids (F27-A) and F27-B owns the hierarchy walk. It does not decide
// eligibility either — the lowered [`WeaponRules`] do, by declaration.

/// What one shot's swept hits produced: the routing outcome and the damage
/// resolution, or the reason the damage could not be resolved.
///
/// Both are returned together, always: a caller must be able to see *which
/// contacts produced damage* and *which damage was refused* in the same pass,
/// so a lost hit is never invisible behind a successful resolution.
#[derive(Clone, Debug, PartialEq)]
pub struct SweptDamageOutcome {
    /// The routing: which candidates were admitted, which hits they produced
    /// and which contacts were refused by name.
    pub sweep: SweepOutcome,
    /// The resolver's output for the routed hits, when the batch resolved.
    pub damage: Result<TickResolution, DamageError>,
}

impl SweptDamageOutcome {
    /// Whether the shot's contacts produced no hit at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sweep.is_empty()
    }

    /// The contacts the routing refused, by name.
    #[must_use]
    pub fn refused_contacts(&self) -> &[cs_sim::weapons::SweepRefusal] {
        &self.sweep.refused
    }
}

/// Runs one accepted shot's swept contacts through the declared rules, the
/// swept geometry and the damage authority.
///
/// `candidates` are the world candidates the collision features reported for
/// this projectile's segment; each names the damage node a contact with it
/// lands on. `at` is the tick being resolved, and it is what the resulting
/// hits are stamped with — a round fired on one tick lands on another.
///
/// The damage resolver is the session's authority: nothing here bypasses it,
/// and the gun's declared per-channel damage amounts reach the graph only
/// through the [`cs_sim::damage::HitEvent`]s it resolves.
///
/// # Errors
///
/// The error is the [`DamageResolver`]'s own, returned inside
/// [`SweptDamageOutcome::damage`] rather than as this function's `Err`: the
/// routing outcome is still meaningful, and dropping it would hide which
/// contacts produced the batch that could not be applied. See the module
/// section above.
#[must_use]
pub fn resolve_swept_damage(
    router: &mut GunHitRouter,
    rules: &WeaponRules,
    damage: &mut DamageResolver,
    shot: &FireEvent,
    segment: &ProjectileSegment,
    candidates: impl IntoIterator<Item = SweepCandidate>,
    at: Tick,
) -> SweptDamageOutcome {
    let sweep = router.route(shot, segment, candidates, rules, at);
    let routed = sweep.damage();
    let resolved = if routed.is_empty() {
        // A round that crossed nothing resolves no batch: calling the resolver
        // with an empty slice would still advance nothing but would report a
        // `TickResolution` for a tick that had no weapon damage in it, which a
        // caller could mistake for "the round hit and did nothing".
        Ok(TickResolution {
            tick: at,
            events: Vec::new(),
        })
    } else {
        damage.resolve(at, &routed)
    };
    SweptDamageOutcome {
        sweep,
        damage: resolved,
    }
}
