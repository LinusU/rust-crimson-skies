//! The capital-ship lowering boundary (F35-A; F35-B lowers the turret
//! boresight and the section integrity pools).
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stages `### F35-A` and `### F35-B`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! This module sits between the declared capital-ship schema
//! ([`cs_content::capital`]) and the runtime aggregate
//! ([`cs_sim::capital`]), which cannot see each other — `cs_sim` must not
//! depend on `cs_content` (`docs/01-ARCHITECTURE.md`). It contains:
//!
//! * [`lower_capital_ship`] — the conversion boundary: a validated
//!   [`cs_content::capital::DeclaredCapitalShip`] becomes a
//!   [`cs_sim::capital::CapitalShip`] with every subsystem, engine, bay,
//!   turret (weapon, traverse and boresight), anchor, the section
//!   integrity pools, the cargo pool and the authored trajectory mapped
//!   field-wise. An unmeasured value (a turret's weapon binding, an
//!   engine's thrust, a section's integrity) lowers as
//!   [`Resolved::Unknown`] and stays unknown. Two declared fields have no
//!   F35-B runtime counterpart and stay parked on the declared record: a
//!   launch bay's `socket_offset_m` and `capacity`. The boundary does not
//!   invent runtime state for them; their consumption is F35-C
//!   (launch/cargo wiring). See
//!   `docs/findings/2026-10-01-f35-a-capital-subsystems-and-bays.md` and
//!   `docs/findings/2026-10-04-f35-b-movement-weakpoints-and-turrets.md`.
//! * [`CapitalLowerError::UnknownOwnership`] — the one mandatory value: a
//!   ship whose owner is unresolved cannot have its guns, targeting and
//!   docking eligibility switched coherently, so the boundary refuses
//!   instead of guessing an owner.
//! * [`CapitalActorBinding`] — the ECS record tying an entity to its actor
//!   and catalog subject, generation-stamped like
//!   [`crate::scene::SceneNodeBinding`] and
//!   [`cs_app::damage::DamageActorBinding`] so a reload can never leave a
//!   stale binding looking live.
//!
//! Nothing here owns runtime behavior: the subsystem state machine,
//! propulsion and bay cycles are `cs_sim::capital`'s; these are the
//! conversion and binding records the ECS wiring consumes (F35-B/C).

use bevy::ecs::component::Component;
use cs_content::capital::{
    CapitalSubsystemEffect as DeclaredEffect, CapitalSubsystemKind as DeclaredKind,
    DeclaredCapitalShip, DeclaredExposure, DeclaredTrajectory,
};
use cs_script::ir::ActorId;
use cs_sim::capital::{
    Bay, BayKind, CapitalError, CapitalParts, CapitalShip, DockingAnchor, EngineSpec,
    ExposureError, ExposureWindow, IntegrityPool, Ownership, PropulsionError, Subsystem,
    SubsystemEffect, SubsystemGraph, SubsystemGraphError, SubsystemKey, SubsystemKeyError,
    TurretMount,
};
use cs_sim::world_actors::Quat;
use cs_sim::world_actors::trajectory::{Keyframe, Trajectory, TrajectoryError};
use cs_types::Tick;
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;

use crate::scene::SceneGeneration;

/// Why a declared capital ship could not be lowered to the runtime records.
#[derive(Clone, Debug, PartialEq)]
pub enum CapitalLowerError {
    /// A declared subsystem key could not form a runtime key — unreachable
    /// while both crates apply the same grammar, kept so the boundary stays
    /// honest if they ever diverge.
    SubsystemKey {
        /// The declared key text.
        key: String,
        /// Why the runtime refused it.
        source: SubsystemKeyError,
    },
    /// A declared exposure window could not form a runtime window.
    Exposure {
        /// The declared subsystem key text.
        key: String,
        /// Why the runtime refused it.
        source: ExposureError,
    },
    /// The runtime refused an engine record.
    Propulsion(PropulsionError),
    /// The runtime refused the assembled subsystem graph.
    Graph(SubsystemGraphError),
    /// The runtime refused the assembled ship.
    Ship(CapitalError),
    /// The runtime refused the authored trajectory.
    Trajectory(TrajectoryError),
    /// The initial owner is `Resolved::Unknown`: no session may switch guns,
    /// targeting or docking eligibility under a guessed owner.
    UnknownOwnership {
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the owner is unknown.
        reason: String,
    },
}

impl std::fmt::Display for CapitalLowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SubsystemKey { key, source } => {
                write!(f, "subsystem key {key:?} cannot be lowered: {source}")
            }
            Self::Exposure { key, source } => {
                write!(
                    f,
                    "subsystem {key} has an unloweable exposure window: {source:?}"
                )
            }
            Self::Propulsion(source) => {
                write!(f, "the runtime refused an engine record: {source}")
            }
            Self::Graph(source) => {
                write!(
                    f,
                    "the runtime refused the lowered subsystem graph: {source}"
                )
            }
            Self::Ship(source) => write!(f, "the runtime refused the lowered ship: {source}"),
            Self::Trajectory(source) => {
                write!(f, "the runtime refused the authored trajectory: {source:?}")
            }
            Self::UnknownOwnership { claim_id, reason } => write!(
                f,
                "the initial owner is unknown ({}: {reason}) and cannot be lowered",
                claim_id.as_str()
            ),
        }
    }
}

impl std::error::Error for CapitalLowerError {}

/// Lowers a declared capital ship into the runtime aggregate the ECS binds.
///
/// Subsystem identity maps by text, kinds, effects and lethality field-wise,
/// engines, bays, turrets, anchors and cargo lower with every
/// [`Resolved::Unknown`] carried through verbatim, and an unknown owner is
/// refused by claim.
///
/// # Errors
///
/// [`CapitalLowerError`] naming the first refused key, window, engine, graph,
/// trajectory or ownership value.
pub fn lower_capital_ship(
    actor: ActorId,
    declared: &DeclaredCapitalShip,
) -> Result<CapitalShip, CapitalLowerError> {
    let mut subsystems = Vec::with_capacity(declared.subsystems().len());
    for subsystem in declared.subsystems() {
        let mut lowered = Subsystem::new(lower_key(&subsystem.key)?, lower_kind(subsystem.kind));
        if let Some(effect) = subsystem.effect {
            lowered = lowered.with_effect(lower_effect(effect));
        }
        lowered = lowered.with_lethal(subsystem.lethal);
        subsystems.push(lowered);
    }
    let graph = SubsystemGraph::try_new(subsystems).map_err(CapitalLowerError::Graph)?;

    let engines = declared
        .engines()
        .iter()
        .map(|engine| {
            let lowered = EngineSpec::try_new(
                lower_key(&engine.key)?,
                engine.thrust_n.clone(),
                engine.axis,
            )
            .map_err(CapitalLowerError::Propulsion)?;
            Ok(lowered)
        })
        .collect::<Result<Vec<_>, CapitalLowerError>>()?;

    let mut bays = Vec::new();
    for bay in declared.weapon_bays() {
        bays.push(Bay::new(
            lower_key(&bay.key)?,
            BayKind::Weapon,
            lower_exposure(&bay.key, bay.exposure)?,
        ));
    }
    for bay in declared.launch_bays() {
        bays.push(Bay::new(
            lower_key(&bay.key)?,
            BayKind::Launch,
            lower_exposure(&bay.key, bay.exposure)?,
        ));
    }

    let turrets = declared
        .turrets()
        .iter()
        .map(|turret| {
            Ok(TurretMount {
                key: lower_key(&turret.key)?,
                weapon: turret.weapon.clone(),
                traverse_deg: turret.traverse_deg.clone(),
                boresight: turret.boresight,
            })
        })
        .collect::<Result<Vec<_>, CapitalLowerError>>()?;

    let docking_anchors = declared
        .docking_anchors()
        .iter()
        .map(|anchor| {
            Ok(DockingAnchor {
                key: lower_key(&anchor.key)?,
                offset_m: anchor.offset_m.clone(),
            })
        })
        .collect::<Result<Vec<_>, CapitalLowerError>>()?;

    // F35-B: the section integrity pools lower verbatim — an unmeasured
    // pool stays unknown and blocks hits by claim at runtime.
    let sections = declared
        .sections()
        .iter()
        .map(|section| {
            Ok(IntegrityPool {
                key: lower_key(&section.key)?,
                integrity: section.integrity.clone(),
            })
        })
        .collect::<Result<Vec<_>, CapitalLowerError>>()?;

    let ownership = match declared.ownership() {
        Resolved::Known(known) => Ownership {
            owner: known.value.clone(),
            captured: false,
        },
        Resolved::Unknown { claim_id, reason } => {
            return Err(CapitalLowerError::UnknownOwnership {
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            });
        }
    };

    let trajectory = declared.trajectory().map(lower_trajectory).transpose()?;

    CapitalShip::try_new(
        declared.subject().clone(),
        actor,
        CapitalParts {
            graph,
            engines,
            bays,
            turrets,
            docking_anchors,
            sections,
            cargo: declared.cargo().clone(),
            ownership,
            trajectory,
        },
    )
    .map_err(CapitalLowerError::Ship)
}

/// Lowers a declared subsystem key into the runtime key.
///
/// # Errors
///
/// [`CapitalLowerError::SubsystemKey`] when the grammar rejects the text.
pub fn lower_key(
    key: &cs_content::capital::CapitalSubsystemKey,
) -> Result<SubsystemKey, CapitalLowerError> {
    SubsystemKey::new(key.as_str()).map_err(|source| CapitalLowerError::SubsystemKey {
        key: key.as_str().to_owned(),
        source,
    })
}

/// Lowers a declared exposure window into the runtime window.
///
/// # Errors
///
/// [`CapitalLowerError::Exposure`] when the runtime rejects the cycle.
pub fn lower_exposure(
    key: &cs_content::capital::CapitalSubsystemKey,
    exposure: DeclaredExposure,
) -> Result<ExposureWindow, CapitalLowerError> {
    ExposureWindow::try_new(
        exposure.concealed_ticks,
        exposure.opening_ticks,
        exposure.exposed_ticks,
        exposure.closing_ticks,
    )
    .map_err(|source| CapitalLowerError::Exposure {
        key: key.as_str().to_owned(),
        source,
    })
}

fn lower_kind(kind: DeclaredKind) -> cs_sim::capital::SubsystemKind {
    match kind {
        DeclaredKind::Engine => cs_sim::capital::SubsystemKind::Engine,
        DeclaredKind::WeaponBay => cs_sim::capital::SubsystemKind::WeaponBay,
        DeclaredKind::LaunchBay => cs_sim::capital::SubsystemKind::LaunchBay,
        DeclaredKind::Turret => cs_sim::capital::SubsystemKind::Turret,
        DeclaredKind::DockingAnchor => cs_sim::capital::SubsystemKind::DockingAnchor,
        DeclaredKind::GasCell => cs_sim::capital::SubsystemKind::GasCell,
        DeclaredKind::StructuralSection => cs_sim::capital::SubsystemKind::StructuralSection,
    }
}

fn lower_effect(effect: DeclaredEffect) -> SubsystemEffect {
    match effect {
        DeclaredEffect::Propulsion => SubsystemEffect::Propulsion,
        DeclaredEffect::WeaponAccess => SubsystemEffect::WeaponAccess,
        DeclaredEffect::Launching => SubsystemEffect::Launching,
        DeclaredEffect::Docking => SubsystemEffect::Docking,
        DeclaredEffect::Vulnerability => SubsystemEffect::Vulnerability,
        DeclaredEffect::MissionCondition => SubsystemEffect::MissionCondition,
    }
}

fn lower_trajectory(trajectory: &DeclaredTrajectory) -> Result<Trajectory, CapitalLowerError> {
    let keyframes = trajectory
        .keyframes
        .iter()
        .map(|keyframe| Keyframe {
            tick: Tick(keyframe.tick),
            position_m: keyframe.position_m,
            orientation: Quat(keyframe.orientation),
        })
        .collect();
    Trajectory::new(keyframes, trajectory.ticks_per_second).map_err(CapitalLowerError::Trajectory)
}

/// Component: marks an entity as the visual/physical face of one capital
/// ship.
///
/// `actor` is the ship's [`ActorId`], `subject` the catalog subject the ship
/// was defined under and `generation` the scene generation that spawned the
/// binding — so a reload stamps new bindings and stale ones are identified
/// by mismatch, never by surviving pointers (the `STATE-TRANSACTIONS`
/// session-generation discipline).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct CapitalActorBinding {
    /// The actor this entity presents.
    pub actor: ActorId,
    /// The capital-ship catalog subject.
    pub subject: ContentId,
    /// The scene generation that spawned the binding.
    pub generation: SceneGeneration,
}
