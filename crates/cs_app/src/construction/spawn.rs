//! The spawned-aircraft equality harness (F44-D, AC04).
//!
//! Spec: `specs/F44-aircraft-construction-budgets-loadouts-and-paint-editor.md`,
//! stage `### F44-D`; contract `docs/contracts/STATE-TRANSACTIONS.md`
//! ("Outcome and economy transaction").
//!
//! AC04 says **"Preview and actual spawned aircraft have equal normalized mass,
//! weapons and paint"**. This module is the half that did not exist: before it,
//! a blueprint could be previewed and committed but nothing ever turned one
//! into an aircraft, so "preview" and "spawned" were two words with only one
//! path behind them.
//!
//! Two projections of one aircraft, normalized to one record:
//!
//! * [`preview_normalized`] reads the screen's own [`ConstructionView`] — the
//!   draft as the player sees it, judged by [`ConstructionRules::validate`].
//! * [`SpawnedAircraft::normalized`] reads the [`SpawnedBlueprint`] component
//!   back **out of the world**, so it reports what was actually spawned rather
//!   than what the caller asked for.
//!
//! The spawn path itself is the production one: [`spawn_blueprint`] runs the
//! shared validator first, then [`spawn_flight_body`] (the one-mass physics
//! spawn) and `WeaponSession::register` (the one place a gun becomes fireable).
//! It refuses instead of dropping anything: a blueprint that breaks a limit, a
//! gun no declared record covers, a mount the session will not take — each
//! leaves no entity behind.
//!
//! # What is *not* claimed here
//!
//! The [`WeightUnits`] mass on both sides is this project's integer
//! game-weight total, exactly as [`BlueprintTotals`](cs_content::construction::BlueprintTotals)
//! computes it. Converting it to the SI kilograms the physics body carries
//! needs the original weight unit's scale, which is **unmeasured** — the F44-C
//! finding records it as F44-D's own unknown, and it stays unmeasured: no file
//! declares a component's mass, the numbers behind the purchase screen's
//! callbacks live in the executable, and inventing a conversion would be a
//! guess. AC04's equality is therefore stated in the unit both sides share,
//! and the spawned body's kilograms are asserted only against the one-mass
//! rule (tuning plus declared [`LoadoutMass`]), never against the game-weight
//! total.
//!
//! Paint travels as [`PaintSelection`] references — never as pixels — so the
//! spawned aircraft carries the same mask and decal ids the preview showed
//! (non-negotiable 5). Mapping those ids onto a `PaintChoice` the livery path
//! composes is unmeasured and is recorded, not guessed.

use bevy::prelude::{Component, Entity, World};

use cs_content::construction::{
    AircraftBlueprint, BlueprintVerdict, ConstructionPolicy, ConstructionRules, PaintSelection,
    PriceBook, ValidationRefusal, WeightUnits,
};
use cs_content::weapons::DeclaredGunDefinition;
use cs_sim::damage::ActorId;
use cs_sim::flight::FlightModel;
use cs_sim::weapons::{GunBank, GunBankError};
use cs_types::content::{ContentId, DamageNodeKey};

use crate::physics::{FlightSpawnError, FlightSpawnSpec, spawn_flight_body};
use crate::weapons::{RegisteredWeapon, WeaponRegistrationError, WeaponSession};

use super::screen::ConstructionView;

/// One aircraft as AC04 compares it: three projections in one canonical form.
///
/// The record is deliberately small — a mass, a weapon list and a paint
/// selection — because those are exactly the three the sheet names. Nothing
/// here is a performance bar or a float: the mass is the validator's own
/// integer total and the weapons and paint are catalog references.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedAircraft {
    mass: WeightUnits,
    weapons: Vec<(DamageNodeKey, ContentId)>,
    paint: PaintSelection,
}

impl NormalizedAircraft {
    /// Assembles the three projections.
    ///
    /// The weapon list is sorted by mount so a preview (which reads the
    /// blueprint's authored order) and a spawn (which reads the order the
    /// weapon session registered in) compare equal when they hold the same
    /// mounts — the order a session happened to register in is not a property
    /// of the aircraft.
    #[must_use]
    pub fn new(
        mass: WeightUnits,
        weapons: Vec<(DamageNodeKey, ContentId)>,
        paint: PaintSelection,
    ) -> Self {
        let mut weapons = weapons;
        weapons.sort();
        Self {
            mass,
            weapons,
            paint,
        }
    }

    /// The integer game-weight total both sides are compared in.
    #[must_use]
    pub const fn mass(&self) -> WeightUnits {
        self.mass
    }

    /// Every weapon the aircraft carries, as `(mount or hardpoint, component)`,
    /// sorted by mount: the guns plus the rocket hardpoints.
    #[must_use]
    pub fn weapons(&self) -> &[(DamageNodeKey, ContentId)] {
        &self.weapons
    }

    /// The paint and decal references — no pixel data, on either side.
    #[must_use]
    pub fn paint(&self) -> &PaintSelection {
        &self.paint
    }
}

/// Why a preview has no normalized aircraft.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewRefusal {
    /// The draft breaks a limit or a constraint, so there is no aircraft behind
    /// it to compare: an invalid build never flies, and giving it a normalized
    /// form would make it look spawnable.
    Invalid(Box<BlueprintVerdict>),
    /// The draft cannot be measured at all — an unmeasured limit, an unpriced
    /// component or an unmeasured pairing rule has no normalized form, and
    /// inventing one would be the guess AC04 exists to catch.
    Unmeasurable(ValidationRefusal),
}

impl std::fmt::Display for PreviewRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(verdict) => write!(
                f,
                "the draft is invalid: {} limit breach(es), {} violation(s)",
                verdict.assessment().breaches().len(),
                verdict.violations().len()
            ),
            Self::Unmeasurable(refusal) => write!(f, "{refusal}"),
        }
    }
}

impl std::error::Error for PreviewRefusal {}

impl From<ValidationRefusal> for PreviewRefusal {
    fn from(refusal: ValidationRefusal) -> Self {
        Self::Unmeasurable(refusal)
    }
}

/// The preview side of AC04: the screen's draft, normalized once.
///
/// The mass is read from the verdict [`ConstructionScreen`](super::ConstructionScreen)
/// produced — the same [`ConstructionRules::validate`] call the commit would
/// enforce — so a preview cannot show a number the commit would not charge.
///
/// # Errors
///
/// [`PreviewRefusal`] when there is no aircraft behind the draft: an invalid
/// build (which [`spawn_blueprint`] would refuse for the same reason) or an
/// unmeasured value that has no normalized form.
pub fn preview_normalized(view: &ConstructionView) -> Result<NormalizedAircraft, PreviewRefusal> {
    let verdict = view.verdict.clone()?;
    if !verdict.is_valid() {
        return Err(PreviewRefusal::Invalid(Box::new(verdict)));
    }
    Ok(NormalizedAircraft::new(
        verdict.assessment().totals().mass(),
        blueprint_weapons(&view.blueprint),
        view.blueprint.paint().clone(),
    ))
}

/// Every weapon a blueprint names, as the normalized projection spells them.
fn blueprint_weapons(blueprint: &AircraftBlueprint) -> Vec<(DamageNodeKey, ContentId)> {
    let mut out: Vec<(DamageNodeKey, ContentId)> = blueprint
        .guns()
        .iter()
        .map(|fitment| (fitment.mount().clone(), fitment.gun().clone()))
        .collect();
    out.extend(
        blueprint
            .ordnance()
            .iter()
            .map(|fitment| (fitment.hardpoint().clone(), fitment.ordnance().clone())),
    );
    out
}

/// What a spawn leaves on the aircraft it created.
///
/// This is a component so the projection is read back **from the world**: a
/// test that compared two values the caller passed in would prove nothing about
/// the spawned aircraft.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct SpawnedBlueprint {
    blueprint: ContentId,
    mass: WeightUnits,
    weapons: Vec<(DamageNodeKey, ContentId)>,
    paint: PaintSelection,
}

impl SpawnedBlueprint {
    /// The spawned aircraft's blueprint id.
    #[must_use]
    pub const fn blueprint(&self) -> &ContentId {
        &self.blueprint
    }

    /// The validator's integer game-weight total for the record that spawned.
    #[must_use]
    pub const fn mass(&self) -> WeightUnits {
        self.mass
    }

    /// The weapons the weapon session reported registering, as `(mount,
    /// component)`, plus the rocket hardpoints.
    #[must_use]
    pub fn weapons(&self) -> &[(DamageNodeKey, ContentId)] {
        &self.weapons
    }

    /// The paint references the spawned aircraft carries.
    #[must_use]
    pub fn paint(&self) -> &PaintSelection {
        &self.paint
    }

    /// The spawned side of AC04.
    #[must_use]
    pub fn normalized(&self) -> NormalizedAircraft {
        NormalizedAircraft::new(self.mass, self.weapons.clone(), self.paint.clone())
    }
}

/// A spawned aircraft, identified by the entity it lives on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpawnedAircraft {
    entity: Entity,
}

impl SpawnedAircraft {
    /// The entity the aircraft was spawned on.
    #[must_use]
    pub const fn entity(&self) -> Entity {
        self.entity
    }

    /// The spawned side of AC04, read back out of `world`.
    ///
    /// `None` means the entity is gone or never carried the record — a missing
    /// spawn is never reported as an equal one.
    #[must_use]
    pub fn normalized(&self, world: &World) -> Option<NormalizedAircraft> {
        world
            .get::<SpawnedBlueprint>(self.entity)
            .map(SpawnedBlueprint::normalized)
    }
}

/// Everything a blueprint needs to become an aircraft.
///
/// The caller supplies the declared gun catalogue and the rule profile the way
/// every other construction caller does: one policy value per caller, one
/// shared validator behind it.
#[derive(Clone, Debug)]
pub struct BlueprintSpawnRequest<'a> {
    /// The record to spawn — not necessarily the one a view is showing, which
    /// is exactly the divergence AC04 compares.
    pub blueprint: &'a AircraftBlueprint,
    /// The airframe's rule profile.
    pub rules: &'a ConstructionRules,
    /// Pairing, banned list and catalog availability.
    pub policy: &'a ConstructionPolicy,
    /// Declared masses and prices.
    pub book: &'a PriceBook,
    /// The declared gun records, one per `(gun, mount)` the blueprint fits.
    pub declared_guns: &'a [DeclaredGunDefinition],
    /// The airframe's flight model — the tuning the one-mass rule reads.
    pub model: FlightModel,
    /// Pose, velocity, environment and loadout mass for the spawn.
    pub spec: FlightSpawnSpec,
}

/// Why a blueprint could not become an aircraft.
///
/// Every variant is a refusal: nothing is dropped, defaulted or half-spawned,
/// so a spawn can never produce an aircraft that quietly carries less than the
/// preview showed.
#[derive(Debug)]
pub enum BlueprintSpawnError {
    /// The shared validator could not measure the blueprint at all.
    Refused(ValidationRefusal),
    /// The blueprint breaks a limit or a constraint, so it is not spawned.
    Invalid(Box<BlueprintVerdict>),
    /// No declared gun record covers this `(gun, mount)` fitment, so the
    /// aircraft's weapons could not be declared — refused by name rather than
    /// registered as something else.
    UndeclaredGun {
        /// The gun the blueprint fits.
        gun: ContentId,
        /// The mount it occupies.
        mount: DamageNodeKey,
    },
    /// The weapon session registered a mount the blueprint does not fit a gun
    /// to, so the spawned aircraft's weapon list cannot be attributed.
    UnmatchedMount {
        /// The mount the session reported.
        mount: DamageNodeKey,
    },
    /// The bank built from the blueprint's mounts was refused.
    Bank(GunBankError),
    /// The weapon session refused the registration.
    Register(WeaponRegistrationError),
    /// The physics spawn refused the aircraft; nothing was created.
    Flight(FlightSpawnError),
}

impl std::fmt::Display for BlueprintSpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(refusal) => write!(f, "the blueprint cannot be judged: {refusal}"),
            Self::Invalid(verdict) => write!(
                f,
                "the blueprint is invalid: {} limit breach(es), {} violation(s)",
                verdict.assessment().breaches().len(),
                verdict.violations().len()
            ),
            Self::UndeclaredGun { gun, mount } => write!(
                f,
                "no declared gun record covers {gun} at {mount}, so it cannot be spawned"
            ),
            Self::UnmatchedMount { mount } => {
                write!(
                    f,
                    "the weapon session registered {mount}, which fits no gun"
                )
            }
            Self::Bank(error) => write!(f, "the gun bank is not usable: {error}"),
            Self::Register(error) => write!(f, "the weapons would not register: {error}"),
            Self::Flight(error) => write!(f, "the aircraft would not spawn: {error}"),
        }
    }
}

impl std::error::Error for BlueprintSpawnError {}

impl From<ValidationRefusal> for BlueprintSpawnError {
    fn from(refusal: ValidationRefusal) -> Self {
        Self::Refused(refusal)
    }
}

impl From<FlightSpawnError> for BlueprintSpawnError {
    fn from(error: FlightSpawnError) -> Self {
        Self::Flight(error)
    }
}

impl From<WeaponRegistrationError> for BlueprintSpawnError {
    fn from(error: WeaponRegistrationError) -> Self {
        Self::Register(error)
    }
}

/// Spawns `request.blueprint` through the production paths, and leaves the
/// [`SpawnedBlueprint`] record AC04 reads back.
///
/// Order matters and every step before the world changes is a refusal:
///
/// 1. [`ConstructionRules::validate`] — the same validator the preview and the
///    commit run, so a spawn cannot meet a weaker rule.
/// 2. The declared gun catalogue is matched per fitment by `(gun, mount)`; an
///    uncovered fitment is refused by name.
/// 3. The gun bank is built from those mounts.
/// 4. [`spawn_flight_body`] creates the body (its own one-mass rule applies).
/// 5. `WeaponSession::register` makes the guns fireable. If it refuses, the
///    body is despawned again, so a failed spawn still leaves no entity.
/// 6. The record is written from what the session actually reported.
///
/// # Errors
///
/// [`BlueprintSpawnError`]; on any error no entity remains.
pub fn spawn_blueprint(
    world: &mut World,
    weapons: &mut WeaponSession,
    actor: ActorId,
    starting_rounds: u64,
    request: &BlueprintSpawnRequest<'_>,
) -> Result<SpawnedAircraft, BlueprintSpawnError> {
    // 1. One validator, before anything exists.
    let verdict = request
        .rules
        .validate(request.policy, request.blueprint, request.book)?;
    if !verdict.is_valid() {
        return Err(BlueprintSpawnError::Invalid(Box::new(verdict)));
    }
    let mass = verdict.assessment().totals().mass();

    // 2. Every fitted gun must be covered by a declared record that names the
    //    same gun *and* the same mount; nothing is substituted.
    let mut declared: Vec<DeclaredGunDefinition> =
        Vec::with_capacity(request.blueprint.guns().len());
    for fitment in request.blueprint.guns() {
        let record = request
            .declared_guns
            .iter()
            .find(|record| record.gun() == fitment.gun() && record.mount() == fitment.mount())
            .cloned()
            .ok_or_else(|| BlueprintSpawnError::UndeclaredGun {
                gun: fitment.gun().clone(),
                mount: fitment.mount().clone(),
            })?;
        declared.push(record);
    }

    // 3. The bank is the mounts the blueprint actually occupies.
    let bank = GunBank::try_new(declared.iter().map(|record| record.mount().clone()))
        .map_err(BlueprintSpawnError::Bank)?;

    // 4. The body enters the world through the production physics spawn.
    let entity = spawn_flight_body(world, request.model.clone(), &request.spec)?;

    // 5. The guns become fireable. A refusal here must not leave a body
    //    behind, or an aircraft could exist with no weapons the preview showed.
    let registered = match weapons.register(actor, &declared, bank, starting_rounds) {
        Ok(registered) => registered,
        Err(error) => {
            let _ = world.try_despawn(entity);
            return Err(BlueprintSpawnError::Register(error));
        }
    };

    // 6. The record reports what the session said it registered, attributed
    //    back to the blueprint's own fitments.
    let weapon_list = match materialized_weapons(request.blueprint, &registered) {
        Ok(weapons) => weapons,
        Err(error) => {
            let _ = world.try_despawn(entity);
            return Err(error);
        }
    };

    world.entity_mut(entity).insert(SpawnedBlueprint {
        blueprint: request.blueprint.id().clone(),
        mass,
        weapons: weapon_list,
        paint: request.blueprint.paint().clone(),
    });
    Ok(SpawnedAircraft { entity })
}

/// Attributes the session's registrations back to the blueprint's fitments,
/// and appends the rocket hardpoints the session does not carry.
fn materialized_weapons(
    blueprint: &AircraftBlueprint,
    registered: &[RegisteredWeapon],
) -> Result<Vec<(DamageNodeKey, ContentId)>, BlueprintSpawnError> {
    let mut out: Vec<(DamageNodeKey, ContentId)> = Vec::with_capacity(registered.len());
    for weapon in registered {
        let fitment = blueprint
            .guns()
            .iter()
            .find(|fitment| fitment.mount() == &weapon.mount)
            .ok_or_else(|| BlueprintSpawnError::UnmatchedMount {
                mount: weapon.mount.clone(),
            })?;
        out.push((weapon.mount.clone(), fitment.gun().clone()));
    }
    out.extend(
        blueprint
            .ordnance()
            .iter()
            .map(|fitment| (fitment.hardpoint().clone(), fitment.ordnance().clone())),
    );
    Ok(out)
}
