//! The capital-ship runtime aggregate: subsystems, propulsion, bays,
//! turrets, anchors, cargo, ownership and trajectory (F35-A), the F35-B
//! ship-level behavior — section damage pools, the weakpoint hit resolver and
//! turret aim/fire gating — and the F35-C accessors the wiring consumes.
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stages `### F35-A`, `### F35-B` and `### F35-C`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! [`CapitalShip`] ties the F35-A contracts to one actor. F35-B adds the
//! production behavior the aggregate itself owns:
//!
//! * [`CapitalShip::apply_hit`] is the weakpoint resolver a projectile path
//!   calls: a bay subsystem is only hittable while its authored
//!   [`ExposureWindow`](crate::capital::ExposureWindow) reports [`BayState::Exposed`], a gas/structural
//!   section absorbs damage into its declared [`IntegrityPool`] until the
//!   pool is depleted, and any other named part is destroyed by one landed
//!   damaging hit. A hit on a concealed, opening or closing bay does
//!   nothing — a closed bay is not an always-hittable health bar
//!   (non-negotiable 2).
//! * [`CapitalShip::propulsion_fraction`] is the share of the ship's
//!   declared thrust its intact engines still deliver; the
//!   [`crate::capital::runtime::CapitalShipSet`] couples it to course
//!   progress, so engine loss measurably slows the ship while the hull
//!   stays intact (non-negotiable 1).
//! * [`CapitalShip::aim_turret`] / [`CapitalShip::may_fire`] are the turret
//!   behavior: aiming is clamped to the mount's traverse cone about its
//!   boresight, a destroyed turret (or a destroyed ship) refuses, and an
//!   unresolved weapon binding refuses to fire by claim rather than
//!   inventing a gun.
//!
//! F35-C adds the three accessors the wiring consumes:
//! [`CapitalShip::launch_rig`] (the release socket and capacity a launch bay
//! carries), [`CapitalShip::docking_open`] (the docking eligibility a capture
//! latches through) and [`CapitalShip::adopt_ownership`] (the one place a
//! ship's owner changes).
//!
//! Runtime behavior is *designed*; no original coefficient is measured.

use std::collections::BTreeMap;

use cs_script::ir::ActorId;
use cs_types::Tick;
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;

use super::bay::{Bay, BayKind, BayState};
use super::capture::Ownership;
use super::motion::{EngineSpec, PropulsionError, acceleration_m_s2};
use super::parts::{DockingAnchor, IntegrityPool, LaunchBayRig, TurretMount};
use super::subsystem::{
    DisableOutcome, SubsystemGraph, SubsystemGraphError, SubsystemKey, SubsystemKind,
    SubsystemState,
};
use crate::world_actors::trajectory::Trajectory;

/// Why a [`CapitalShip`] was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum CapitalError {
    /// Two parts of one kind share a key.
    DuplicatePart {
        /// The kind.
        kind: SubsystemKind,
        /// The duplicated key.
        key: SubsystemKey,
    },
    /// A part key names no subsystem of the graph.
    UnknownPart {
        /// The kind.
        kind: SubsystemKind,
        /// The dangling key.
        key: SubsystemKey,
    },
    /// A part key names a subsystem of the wrong kind.
    PartKindMismatch {
        /// The offending key.
        key: SubsystemKey,
        /// The kind the part record requires.
        expected: SubsystemKind,
        /// The kind the subsystem declares.
        actual: SubsystemKind,
    },
    /// The cargo capacity was not finite.
    NonFiniteCargo,
    /// The cargo capacity was negative.
    NegativeCargo {
        /// The refused value.
        value: f64,
    },
    /// A turret's boresight direction was not finite.
    NonFiniteBoresight {
        /// The turret.
        key: SubsystemKey,
    },
    /// A turret's boresight direction was the zero vector: the mount cannot
    /// point nowhere.
    ZeroBoresight {
        /// The turret.
        key: SubsystemKey,
    },
    /// A turret's known traverse was not finite.
    NonFiniteTraverse {
        /// The turret.
        key: SubsystemKey,
    },
    /// A turret's known traverse was outside `[0, 360]` degrees — a mount
    /// cannot sweep more than the whole circle.
    TraverseOutOfRange {
        /// The turret.
        key: SubsystemKey,
        /// The refused arc.
        value: f64,
    },
    /// A section integrity pool's known value was not finite.
    NonFiniteIntegrity {
        /// The section.
        key: SubsystemKey,
    },
    /// A section integrity pool's known value was negative.
    NegativeIntegrity {
        /// The section.
        key: SubsystemKey,
        /// The refused value.
        value: f64,
    },
    /// A launch rig was attached to a bay that is not a launch bay: a weapon
    /// bay has no hangar to release aircraft from.
    LaunchRigOnWeaponBay {
        /// The offending bay.
        key: SubsystemKey,
    },
    /// A launch rig's socket offset was not finite.
    NonFiniteSocket {
        /// The offending bay.
        key: SubsystemKey,
    },
}

impl std::fmt::Display for CapitalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicatePart { kind, key } => {
                write!(f, "{kind} key {key:?} is used twice")
            }
            Self::UnknownPart { kind, key } => {
                write!(f, "{kind} {key} is not a subsystem of the ship")
            }
            Self::PartKindMismatch {
                key,
                expected,
                actual,
            } => write!(f, "part {key} is {actual}, expected {expected}"),
            Self::NonFiniteCargo => write!(f, "the cargo capacity is not finite"),
            Self::NegativeCargo { value } => {
                write!(f, "the cargo capacity {value} is negative")
            }
            Self::NonFiniteBoresight { key } => {
                write!(f, "turret {key} has a non-finite boresight")
            }
            Self::ZeroBoresight { key } => write!(f, "turret {key} has a zero boresight"),
            Self::NonFiniteTraverse { key } => {
                write!(f, "turret {key} has a non-finite traverse")
            }
            Self::TraverseOutOfRange { key, value } => {
                write!(
                    f,
                    "turret {key} traverses {value} degrees, outside [0, 360]"
                )
            }
            Self::NonFiniteIntegrity { key } => {
                write!(f, "section {key} has a non-finite integrity")
            }
            Self::NegativeIntegrity { key, value } => {
                write!(f, "section {key} has negative integrity {value}")
            }
            Self::LaunchRigOnWeaponBay { key } => {
                write!(f, "bay {key} carries a launch rig but is not a launch bay")
            }
            Self::NonFiniteSocket { key } => {
                write!(f, "launch bay {key} has a non-finite socket offset")
            }
        }
    }
}

impl std::error::Error for CapitalError {}

/// Everything [`CapitalShip::try_new`] assembles into one ship.
#[derive(Clone, Debug, PartialEq)]
pub struct CapitalParts {
    /// The subsystem graph.
    pub graph: SubsystemGraph,
    /// The engines.
    pub engines: Vec<EngineSpec>,
    /// The weapon and launch bays.
    pub bays: Vec<Bay>,
    /// The turret mounts.
    pub turrets: Vec<TurretMount>,
    /// The docking anchors.
    pub docking_anchors: Vec<DockingAnchor>,
    /// The gas/structural section integrity pools (F35-B).
    pub sections: Vec<IntegrityPool>,
    /// The cargo capacity, or an explicit unknown.
    pub cargo: Resolved<f64>,
    /// The initial ownership.
    pub ownership: Ownership,
    /// The authored trajectory, when the ship has one.
    pub trajectory: Option<Trajectory>,
}

/// The session state of one section's [`IntegrityPool`]: the declared pool
/// verbatim, plus what a known pool has left. `remaining` is `None` exactly
/// when the declared integrity is unresolved — an unknown pool never
/// silently absorbs a hit.
#[derive(Clone, Debug, PartialEq)]
struct PoolState {
    declared: Resolved<f64>,
    remaining: Option<f64>,
}

/// One capital ship's contract and behavior state.
#[derive(Clone, Debug, PartialEq)]
pub struct CapitalShip {
    subject: ContentId,
    actor: ActorId,
    graph: SubsystemGraph,
    engines: BTreeMap<SubsystemKey, EngineSpec>,
    bays: BTreeMap<SubsystemKey, Bay>,
    turrets: BTreeMap<SubsystemKey, TurretMount>,
    docking_anchors: BTreeMap<SubsystemKey, DockingAnchor>,
    /// Session damage pools for gas/structural sections (F35-B).
    pools: BTreeMap<SubsystemKey, PoolState>,
    /// The body-frame direction each turret currently bears, initialized to
    /// its boresight (F35-B).
    aims: BTreeMap<SubsystemKey, [f64; 3]>,
    cargo: Resolved<f64>,
    ownership: Ownership,
    trajectory: Option<Trajectory>,
}

impl CapitalShip {
    /// Assembles a ship from its validated parts.
    ///
    /// Every typed part key must be a subsystem of `graph` of the matching
    /// kind, and no key may repeat within a kind. A section integrity pool
    /// may sit only on a gas cell or structural section; a turret's
    /// boresight is normalized here and a known traverse must lie in
    /// `[0, 360]` degrees.
    ///
    /// # Errors
    ///
    /// [`CapitalError`] naming the duplicate, unknown or mistyped key, a
    /// corrupt cargo capacity, a bad turret mount or a corrupt section
    /// pool.
    pub fn try_new(
        subject: ContentId,
        actor: ActorId,
        parts: CapitalParts,
    ) -> Result<Self, CapitalError> {
        if let Resolved::Known(known) = &parts.cargo {
            if !known.value.is_finite() {
                return Err(CapitalError::NonFiniteCargo);
            }
            if known.value < 0.0 {
                return Err(CapitalError::NegativeCargo { value: known.value });
            }
        }

        let engines = collect(
            SubsystemKind::Engine,
            parts.engines,
            &parts.graph,
            |engine| engine.key.clone(),
        )?;
        let mut bays: BTreeMap<SubsystemKey, Bay> = BTreeMap::new();
        for bay in parts.bays {
            let kind = bay.kind.subsystem_kind();
            check_part(kind, bay.key.clone(), &parts.graph)?;
            // F35-C: a release rig is hangar wiring, so it may only sit on a
            // launch bay, and a known socket offset must be a real transform.
            if let Some(rig) = &bay.rig {
                if bay.kind != BayKind::Launch {
                    return Err(CapitalError::LaunchRigOnWeaponBay {
                        key: bay.key.clone(),
                    });
                }
                if let Resolved::Known(known) = &rig.offset_m
                    && !known.value.iter().all(|value| value.is_finite())
                {
                    return Err(CapitalError::NonFiniteSocket {
                        key: bay.key.clone(),
                    });
                }
            }
            let key = bay.key.clone();
            if bays.insert(key.clone(), bay).is_some() {
                return Err(CapitalError::DuplicatePart { kind, key });
            }
        }
        let mut turrets = collect(
            SubsystemKind::Turret,
            parts.turrets,
            &parts.graph,
            |turret| turret.key.clone(),
        )?;
        let docking_anchors = collect(
            SubsystemKind::DockingAnchor,
            parts.docking_anchors,
            &parts.graph,
            |anchor| anchor.key.clone(),
        )?;

        // A turret's aim starts on its boresight; boresights are normalized
        // so the traverse math can treat them as unit vectors.
        let mut aims = BTreeMap::new();
        for turret in turrets.values_mut() {
            let key = turret.key.clone();
            turret.boresight = validated_unit(&key, turret.boresight)?;
            if let Resolved::Known(known) = &turret.traverse_deg {
                if !known.value.is_finite() {
                    return Err(CapitalError::NonFiniteTraverse { key });
                }
                if !(0.0..=360.0).contains(&known.value) {
                    return Err(CapitalError::TraverseOutOfRange {
                        key,
                        value: known.value,
                    });
                }
            }
            aims.insert(key, turret.boresight);
        }

        // A section pool may sit only on a gas cell or structural section —
        // the same restriction the declared schema applies.
        let mut pools = BTreeMap::new();
        for pool in parts.sections {
            let key = pool.key.clone();
            let Some(subsystem) = parts.graph.subsystem(&key) else {
                return Err(CapitalError::UnknownPart {
                    kind: SubsystemKind::StructuralSection,
                    key,
                });
            };
            if !matches!(
                subsystem.kind,
                SubsystemKind::GasCell | SubsystemKind::StructuralSection
            ) {
                return Err(CapitalError::PartKindMismatch {
                    key,
                    expected: SubsystemKind::StructuralSection,
                    actual: subsystem.kind,
                });
            }
            if pools.contains_key(&key) {
                return Err(CapitalError::DuplicatePart {
                    kind: subsystem.kind,
                    key,
                });
            }
            let remaining = match &pool.integrity {
                Resolved::Known(known) => {
                    if !known.value.is_finite() {
                        return Err(CapitalError::NonFiniteIntegrity { key });
                    }
                    if known.value < 0.0 {
                        return Err(CapitalError::NegativeIntegrity {
                            key,
                            value: known.value,
                        });
                    }
                    Some(known.value)
                }
                Resolved::Unknown { .. } => None,
            };
            pools.insert(
                key,
                PoolState {
                    declared: pool.integrity,
                    remaining,
                },
            );
        }

        Ok(Self {
            subject,
            actor,
            graph: parts.graph,
            engines,
            bays,
            turrets,
            docking_anchors,
            pools,
            aims,
            cargo: parts.cargo,
            ownership: parts.ownership,
            trajectory: parts.trajectory,
        })
    }

    /// The catalog subject the ship was defined under.
    #[must_use]
    pub fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// The actor this ship is.
    #[must_use]
    pub fn actor(&self) -> ActorId {
        self.actor
    }

    /// The subsystem graph.
    #[must_use]
    pub fn graph(&self) -> &SubsystemGraph {
        &self.graph
    }

    /// The current ownership.
    #[must_use]
    pub fn ownership(&self) -> &Ownership {
        &self.ownership
    }

    /// Takes `ownership` as the ship's (F35-C).
    ///
    /// This is the one place a ship's owner changes: the capture transaction
    /// produces the record and the session set applies it here, in the same
    /// step as the control switch, so no consumer can read a new owner beside
    /// the previous owner's guns. The transaction decides *whether* ownership
    /// moves; it does not move it itself.
    pub fn adopt_ownership(&mut self, ownership: Ownership) {
        self.ownership = ownership;
    }

    /// A launch bay's release wiring, or `None` for a weapon bay, a bay that
    /// declared none, or a key that names no bay.
    #[must_use]
    pub fn launch_rig(&self, key: &SubsystemKey) -> Option<&LaunchBayRig> {
        self.bays.get(key)?.launch_rig()
    }

    /// The launch bays, in key order: the bays that can carry a rig.
    pub fn launch_bays(&self) -> impl Iterator<Item = &Bay> {
        self.bays.values().filter(|bay| bay.kind == BayKind::Launch)
    }

    /// How many launch bays carry release wiring. The count is a diagnostic for
    /// a lowered ship: a declared launch bay that lowered no rig would have
    /// nowhere to release aircraft from.
    #[must_use]
    pub fn launch_rigs(&self) -> usize {
        self.bays
            .values()
            .filter(|bay| bay.kind == BayKind::Launch && bay.rig.is_some())
            .count()
    }

    /// Whether the ship's docking anchors are all intact: the docking
    /// eligibility a capture latches through (F35-C). A ship with no declared
    /// anchor cannot be boarded.
    #[must_use]
    pub fn docking_open(&self) -> bool {
        !self.docking_anchors.is_empty()
            && self
                .docking_anchors
                .keys()
                .all(|key| self.graph.state(key) == Some(SubsystemState::Intact))
    }

    /// The cargo capacity, or an explicit unknown.
    #[must_use]
    pub const fn cargo(&self) -> &Resolved<f64> {
        &self.cargo
    }

    /// The authored trajectory, when the ship has one.
    #[must_use]
    pub fn trajectory(&self) -> Option<&Trajectory> {
        self.trajectory.as_ref()
    }

    /// A subsystem's state.
    #[must_use]
    pub fn subsystem_state(&self, key: &SubsystemKey) -> Option<SubsystemState> {
        self.graph.state(key)
    }

    /// Whether a lethal subsystem has destroyed the ship.
    #[must_use]
    pub fn is_destroyed(&self) -> bool {
        self.graph.is_destroyed()
    }

    /// Disables one subsystem and reports the behavior its destruction
    /// applies. Only a lethal subsystem destroys the ship.
    ///
    /// # Errors
    ///
    /// [`SubsystemGraphError::UnknownSubsystem`] when the key names no part.
    pub fn disable(&mut self, key: &SubsystemKey) -> Result<DisableOutcome, SubsystemGraphError> {
        self.graph.disable(key)
    }

    /// The engines, in key order.
    pub fn engines(&self) -> impl Iterator<Item = &EngineSpec> {
        self.engines.values()
    }

    /// The turret mounts, in key order.
    pub fn turrets(&self) -> impl Iterator<Item = &TurretMount> {
        self.turrets.values()
    }

    /// The docking anchors, in key order.
    pub fn docking_anchors(&self) -> impl Iterator<Item = &DockingAnchor> {
        self.docking_anchors.values()
    }

    /// A bay's state at `tick`: [`BayState::Destroyed`] once its subsystem
    /// is disabled, otherwise its authored exposure cycle.
    #[must_use]
    pub fn bay_state(&self, key: &SubsystemKey, tick: Tick) -> Option<BayState> {
        let bay = self.bays.get(key)?;
        if self.graph.state(key) == Some(SubsystemState::Disabled) {
            return Some(BayState::Destroyed);
        }
        Some(bay.exposure.state_at(tick))
    }

    /// The sum of the thrust of every engine that is still intact.
    ///
    /// # Errors
    ///
    /// [`PropulsionError::UnknownThrust`] when an intact engine's thrust is
    /// unresolved — the total is refused, never approximated.
    pub fn engine_thrust_n(&self) -> Result<f64, PropulsionError> {
        let mut total = 0.0_f64;
        for engine in self.engines.values() {
            if self.graph.state(&engine.key) == Some(SubsystemState::Disabled) {
                continue;
            }
            total += engine.known_thrust_n()?;
        }
        Ok(total)
    }

    /// The summed body-frame force of every intact engine at `throttle`.
    ///
    /// # Errors
    ///
    /// [`PropulsionError`] for an unresolved thrust or a non-finite
    /// throttle.
    pub fn propulsive_force_n(&self, throttle: f64) -> Result<[f64; 3], PropulsionError> {
        let mut force = [0.0_f64; 3];
        for engine in self.engines.values() {
            if self.graph.state(&engine.key) == Some(SubsystemState::Disabled) {
                continue;
            }
            let engine_force = engine.force_n(throttle)?;
            force[0] += engine_force[0];
            force[1] += engine_force[1];
            force[2] += engine_force[2];
        }
        Ok(force)
    }

    /// The body-frame acceleration of every intact engine at `throttle` for
    /// a mass of `mass_kg`. Disabling engines lowers this; disabling all of
    /// them yields zero, and none of it touches the hull's subsystem state.
    ///
    /// # Errors
    ///
    /// [`PropulsionError`] for an unresolved thrust, a non-finite throttle
    /// or a non-positive mass.
    pub fn propulsive_acceleration_m_s2(
        &self,
        throttle: f64,
        mass_kg: f64,
    ) -> Result<[f64; 3], PropulsionError> {
        acceleration_m_s2(self.propulsive_force_n(throttle)?, mass_kg)
    }

    /// The sum of every engine's declared thrust, intact or disabled: the
    /// denominator [`Self::propulsion_fraction`] divides by.
    ///
    /// # Errors
    ///
    /// [`PropulsionError::UnknownThrust`] when any engine's thrust is
    /// unresolved — the designed total is refused, never approximated, so a
    /// ship carrying an unmeasured engine cannot have its motion computed.
    pub fn declared_thrust_n(&self) -> Result<f64, PropulsionError> {
        let mut total = 0.0_f64;
        for engine in self.engines.values() {
            total += engine.known_thrust_n()?;
        }
        Ok(total)
    }

    /// The fraction of designed thrust the intact engines still deliver, in
    /// `[0, 1]` — the motion response non-negotiable 1 asks for. A ship
    /// with no engine subsystem has no propulsion to lose: its authored
    /// course is unpowered and it returns `1.0`. A ship whose declared
    /// engines deliver nothing returns `0.0`.
    ///
    /// # Errors
    ///
    /// [`PropulsionError::UnknownThrust`] when any declared engine's thrust
    /// is unresolved.
    pub fn propulsion_fraction(&self) -> Result<f64, PropulsionError> {
        if self.engines.is_empty() {
            return Ok(1.0);
        }
        let declared = self.declared_thrust_n()?;
        if declared <= 0.0 {
            return Ok(0.0);
        }
        Ok((self.engine_thrust_n()? / declared).clamp(0.0, 1.0))
    }

    /// A section's declared integrity pool, verbatim. `None` when the part
    /// declared none.
    #[must_use]
    pub fn integrity_pool(&self, key: &SubsystemKey) -> Option<&Resolved<f64>> {
        self.pools.get(key).map(|pool| &pool.declared)
    }

    /// What a section's known integrity pool has left after applied hits.
    /// `None` for a part with no pool or an unresolved one.
    #[must_use]
    pub fn remaining_integrity(&self, key: &SubsystemKey) -> Option<f64> {
        self.pools.get(key).and_then(|pool| pool.remaining)
    }

    /// Applies one hit to the named subsystem at tick `at`.
    ///
    /// This is the weakpoint resolver (non-negotiable 2):
    ///
    /// * A weapon or launch bay is only damaged while its authored
    ///   [`ExposureWindow`](crate::capital::ExposureWindow) reports [`BayState::Exposed`] at `at` —
    ///   [`HitOutcome::NotExposed`] names the observed state otherwise and
    ///   the subsystem stays intact.
    /// * A gas cell or structural section with a declared
    ///   [`IntegrityPool`] absorbs damage until the pool is depleted, then
    ///   transitions; an unresolved pool blocks the hit by claim instead of
    ///   absorbing it silently.
    /// * Any other named part — an engine, turret, docking anchor or a bay
    ///   subsystem with no declared window — is destroyed by one landed
    ///   damaging hit.
    ///
    /// A zero-damage hit lands nothing ([`HitOutcome::Ineffective`]), a hit
    /// on an already-disabled part applies nothing twice
    /// ([`HitOutcome::AlreadyDisabled`]), and a wrecked ship still takes
    /// part damage — destruction is monotonic, but parts aboard a wreck may
    /// still be destroyed (the F29 wreck convention).
    ///
    /// # Errors
    ///
    /// [`HitError`] for a non-finite or negative damage, or a subsystem key
    /// that names no part of the ship.
    pub fn apply_hit(
        &mut self,
        subsystem: &SubsystemKey,
        damage: f64,
        at: Tick,
    ) -> Result<HitOutcome, HitError> {
        if !damage.is_finite() {
            return Err(HitError::NonFiniteDamage {
                subsystem: subsystem.clone(),
            });
        }
        if damage < 0.0 {
            return Err(HitError::NegativeDamage {
                subsystem: subsystem.clone(),
                value: damage,
            });
        }
        let kind = self
            .graph
            .subsystem(subsystem)
            .map(|part| part.kind)
            .ok_or_else(|| HitError::UnknownSubsystem(subsystem.clone()))?;
        if self.graph.state(subsystem) == Some(SubsystemState::Disabled) {
            return Ok(HitOutcome::AlreadyDisabled {
                subsystem: subsystem.clone(),
            });
        }
        if damage == 0.0 {
            return Ok(HitOutcome::Ineffective {
                subsystem: subsystem.clone(),
            });
        }
        match kind {
            SubsystemKind::WeaponBay | SubsystemKind::LaunchBay => {
                // A bay with a declared window is a timed weakpoint; a bay
                // subsystem with no window behaves like any other part.
                if let Some(bay) = self.bays.get(subsystem) {
                    let state = bay.exposure.state_at(at);
                    if state != BayState::Exposed {
                        return Ok(HitOutcome::NotExposed {
                            subsystem: subsystem.clone(),
                            state,
                        });
                    }
                }
                self.destroy_part(subsystem)
            }
            SubsystemKind::GasCell | SubsystemKind::StructuralSection => {
                // Decide against the pool first, then act, so the pools
                // borrow never overlaps the destruction.
                enum PoolAction {
                    Destroy,
                    Damage(f64),
                    Blocked(ClaimId, String),
                }
                let action = match self.pools.get(subsystem) {
                    Some(pool) => match &pool.declared {
                        Resolved::Known(_) => {
                            let remaining =
                                pool.remaining.expect("a known pool always has a remainder")
                                    - damage;
                            if remaining <= 0.0 {
                                PoolAction::Destroy
                            } else {
                                PoolAction::Damage(remaining)
                            }
                        }
                        Resolved::Unknown { claim_id, reason } => {
                            PoolAction::Blocked(claim_id.clone(), reason.clone())
                        }
                    },
                    // A section with no declared pool has nothing to
                    // absorb into: a landed hit destroys it.
                    None => PoolAction::Destroy,
                };
                match action {
                    PoolAction::Destroy => self.destroy_part(subsystem),
                    PoolAction::Damage(remaining) => {
                        self.pools
                            .get_mut(subsystem)
                            .expect("the pool was checked above")
                            .remaining = Some(remaining);
                        Ok(HitOutcome::Damaged {
                            subsystem: subsystem.clone(),
                            remaining_integrity: remaining,
                        })
                    }
                    PoolAction::Blocked(claim_id, reason) => Ok(HitOutcome::Blocked {
                        subsystem: subsystem.clone(),
                        claim_id,
                        reason,
                    }),
                }
            }
            _ => self.destroy_part(subsystem),
        }
    }

    /// Destroys `subsystem` through the graph — the one place a part
    /// transitions — and reports the outcome.
    fn destroy_part(&mut self, subsystem: &SubsystemKey) -> Result<HitOutcome, HitError> {
        let outcome = self.graph.disable(subsystem).map_err(HitError::Graph)?;
        Ok(HitOutcome::Destroyed {
            subsystem: subsystem.clone(),
            outcome,
        })
    }

    /// The body-frame unit direction `key`'s turret currently bears, `None`
    /// when the key names no turret. A destroyed turret keeps the bearing it
    /// died on.
    #[must_use]
    pub fn turret_aim(&self, key: &SubsystemKey) -> Option<[f64; 3]> {
        self.aims.get(key).copied()
    }

    /// Commands `key`'s turret to bear on `direction` (a body-frame vector).
    ///
    /// The mount's traverse is the cone of `traverse_deg` degrees about its
    /// boresight: a commanded direction inside the cone becomes the aim
    /// verbatim; one outside is clamped to the cone rim on the great circle
    /// from the boresight toward the target, and the returned
    /// [`TurretAim::within_arc`] is `false` — the turret turns as far as it
    /// can, it never snaps. (An antiparallel command has no preferred great
    /// circle; the clamp then bears on the least-aligned axis, a
    /// deterministic designed edge case.)
    ///
    /// # Errors
    ///
    /// [`TurretRefusal`] when the key names no turret, the turret or the
    /// ship is destroyed, the mount's traverse is unresolved, or the
    /// commanded direction is non-finite or zero.
    pub fn aim_turret(
        &mut self,
        key: &SubsystemKey,
        direction: [f64; 3],
    ) -> Result<TurretAim, TurretRefusal> {
        let turret = self
            .turrets
            .get(key)
            .ok_or_else(|| TurretRefusal::UnknownTurret { key: key.clone() })?;
        if self.is_destroyed() {
            return Err(TurretRefusal::ShipDestroyed { key: key.clone() });
        }
        if self.graph.state(key) == Some(SubsystemState::Disabled) {
            return Err(TurretRefusal::Destroyed { key: key.clone() });
        }
        let d = unit_direction(key, direction)?;
        let traverse_deg = match &turret.traverse_deg {
            Resolved::Known(known) => known.value,
            Resolved::Unknown { claim_id, reason } => {
                return Err(TurretRefusal::TraverseUnknown {
                    key: key.clone(),
                    claim_id: claim_id.clone(),
                    reason: reason.clone(),
                });
            }
        };
        let boresight = turret.boresight;
        let half_deg = traverse_deg * 0.5;
        let off_boresight_deg = angle_deg(boresight, d);
        let aim = if off_boresight_deg <= half_deg {
            d
        } else {
            clamp_to_cone(boresight, d, half_deg.to_radians())
        };
        self.aims.insert(key.clone(), aim);
        Ok(TurretAim {
            aim,
            within_arc: off_boresight_deg <= half_deg,
            off_boresight_deg,
        })
    }

    /// The body-frame direction `key`'s turret would fire along — its
    /// current aim — when the mount is live and its weapon is known.
    ///
    /// # Errors
    ///
    /// [`TurretRefusal`] when the key names no turret, the turret or the
    /// ship is destroyed, or the weapon binding is unresolved — an
    /// unmeasured gun never fires as a guessed one.
    pub fn may_fire(&self, key: &SubsystemKey) -> Result<[f64; 3], TurretRefusal> {
        let turret = self
            .turrets
            .get(key)
            .ok_or_else(|| TurretRefusal::UnknownTurret { key: key.clone() })?;
        if self.is_destroyed() {
            return Err(TurretRefusal::ShipDestroyed { key: key.clone() });
        }
        if self.graph.state(key) == Some(SubsystemState::Disabled) {
            return Err(TurretRefusal::Destroyed { key: key.clone() });
        }
        match &turret.weapon {
            Resolved::Known(_) => Ok(self.aims.get(key).copied().unwrap_or(turret.boresight)),
            Resolved::Unknown { claim_id, reason } => Err(TurretRefusal::WeaponUnknown {
                key: key.clone(),
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            }),
        }
    }
}

/// What one [`CapitalShip::apply_hit`] call produced.
#[derive(Clone, Debug, PartialEq)]
pub enum HitOutcome {
    /// The named part was already disabled: nothing applied twice.
    AlreadyDisabled {
        /// The subsystem struck.
        subsystem: SubsystemKey,
    },
    /// The hit named a bay that was not an exposed weakpoint at the hit's
    /// tick. The observed bay state is carried so the caller can
    /// distinguish concealed, opening and closing.
    NotExposed {
        /// The bay subsystem struck.
        subsystem: SubsystemKey,
        /// The bay's state at the hit's tick.
        state: BayState,
    },
    /// The hit landed on a section whose integrity pool is unresolved:
    /// nothing was absorbed and the block surfaces by claim, never
    /// silently.
    Blocked {
        /// The section struck.
        subsystem: SubsystemKey,
        /// The claim the unknown pool is recorded under.
        claim_id: ClaimId,
        /// Why the pool is unknown.
        reason: String,
    },
    /// The hit landed but applied nothing: a zero-damage hit destroys no
    /// part and drains no pool.
    Ineffective {
        /// The subsystem struck.
        subsystem: SubsystemKey,
    },
    /// A section's pool absorbed the hit and the part survives.
    Damaged {
        /// The section struck.
        subsystem: SubsystemKey,
        /// The pool's remaining integrity.
        remaining_integrity: f64,
    },
    /// The hit destroyed the subsystem; the outcome carries the applied
    /// effect and whether the actor was destroyed with it.
    Destroyed {
        /// The subsystem destroyed.
        subsystem: SubsystemKey,
        /// What the destruction applied.
        outcome: DisableOutcome,
    },
}

/// Why a [`CapitalShip::apply_hit`] was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum HitError {
    /// The damage was NaN or infinite.
    NonFiniteDamage {
        /// The subsystem named.
        subsystem: SubsystemKey,
    },
    /// The damage was negative: a hit cannot heal a part.
    NegativeDamage {
        /// The subsystem named.
        subsystem: SubsystemKey,
        /// The refused damage.
        value: f64,
    },
    /// The key names no part of the ship.
    UnknownSubsystem(SubsystemKey),
    /// The subsystem graph refused the destruction — unreachable for a key
    /// already validated present, kept so the refusal surfaces rather than
    /// being swallowed.
    Graph(SubsystemGraphError),
}

impl std::fmt::Display for HitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFiniteDamage { subsystem } => {
                write!(f, "hit on {subsystem} carries non-finite damage")
            }
            Self::NegativeDamage { subsystem, value } => {
                write!(f, "hit on {subsystem} carries negative damage {value}")
            }
            Self::UnknownSubsystem(subsystem) => {
                write!(f, "hit names {subsystem}, which is not part of the ship")
            }
            Self::Graph(source) => write!(f, "the subsystem graph refused the hit: {source}"),
        }
    }
}

impl std::error::Error for HitError {}

/// Where an [`CapitalShip::aim_turret`] command left a turret bearing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TurretAim {
    /// The body-frame unit direction the turret now bears on: the commanded
    /// direction verbatim when it lay inside the traverse cone, else the
    /// cone rim nearest it.
    pub aim: [f64; 3],
    /// Whether the commanded direction lay inside the traverse cone.
    pub within_arc: bool,
    /// The commanded direction's angle from the boresight, in degrees.
    pub off_boresight_deg: f64,
}

/// Why a turret refused an aim or fire command.
#[derive(Clone, Debug, PartialEq)]
pub enum TurretRefusal {
    /// The key names no turret mount of the ship.
    UnknownTurret {
        /// The refused key.
        key: SubsystemKey,
    },
    /// The turret subsystem is destroyed: its weapon access is gone and it
    /// can neither aim nor fire.
    Destroyed {
        /// The turret.
        key: SubsystemKey,
    },
    /// The ship itself is destroyed: nothing aboard it aims or fires.
    ShipDestroyed {
        /// The turret.
        key: SubsystemKey,
    },
    /// The mount's weapon binding is unresolved; an unmeasured gun never
    /// fires as a guessed one.
    WeaponUnknown {
        /// The turret.
        key: SubsystemKey,
        /// The claim the unknown binding is recorded under.
        claim_id: ClaimId,
        /// Why the weapon is unknown.
        reason: String,
    },
    /// The mount's traverse is unresolved; no arc can be judged.
    TraverseUnknown {
        /// The turret.
        key: SubsystemKey,
        /// The claim the unknown arc is recorded under.
        claim_id: ClaimId,
        /// Why the traverse is unknown.
        reason: String,
    },
    /// The commanded direction was not finite.
    NonFiniteDirection {
        /// The turret.
        key: SubsystemKey,
    },
    /// The commanded direction was the zero vector.
    ZeroDirection {
        /// The turret.
        key: SubsystemKey,
    },
}

impl std::fmt::Display for TurretRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTurret { key } => write!(f, "{key} is not a turret of this ship"),
            Self::Destroyed { key } => write!(f, "turret {key} is destroyed"),
            Self::ShipDestroyed { key } => {
                write!(f, "turret {key}'s ship is destroyed")
            }
            Self::WeaponUnknown {
                key,
                claim_id,
                reason,
            } => write!(
                f,
                "turret {key} has an unresolved weapon ({}: {reason})",
                claim_id.as_str()
            ),
            Self::TraverseUnknown {
                key,
                claim_id,
                reason,
            } => write!(
                f,
                "turret {key} has an unresolved traverse ({}: {reason})",
                claim_id.as_str()
            ),
            Self::NonFiniteDirection { key } => {
                write!(f, "turret {key} was commanded a non-finite direction")
            }
            Self::ZeroDirection { key } => {
                write!(f, "turret {key} was commanded a zero direction")
            }
        }
    }
}

impl std::error::Error for TurretRefusal {}

/// Normalizes a turret boresight to unit length at ship construction.
fn validated_unit(key: &SubsystemKey, axis: [f64; 3]) -> Result<[f64; 3], CapitalError> {
    if !axis.iter().all(|value| value.is_finite()) {
        return Err(CapitalError::NonFiniteBoresight { key: key.clone() });
    }
    let norm = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    if norm <= f64::EPSILON {
        return Err(CapitalError::ZeroBoresight { key: key.clone() });
    }
    Ok([axis[0] / norm, axis[1] / norm, axis[2] / norm])
}

/// Normalizes a commanded aim direction.
fn unit_direction(key: &SubsystemKey, direction: [f64; 3]) -> Result<[f64; 3], TurretRefusal> {
    if !direction.iter().all(|value| value.is_finite()) {
        return Err(TurretRefusal::NonFiniteDirection { key: key.clone() });
    }
    let norm =
        (direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2])
            .sqrt();
    if norm <= f64::EPSILON {
        return Err(TurretRefusal::ZeroDirection { key: key.clone() });
    }
    Ok([
        direction[0] / norm,
        direction[1] / norm,
        direction[2] / norm,
    ])
}

/// The angle between two unit vectors, in degrees.
fn angle_deg(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dot = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).clamp(-1.0, 1.0);
    dot.acos().to_degrees()
}

/// Clamps the unit direction `d` onto the rim of the cone of `half_rad`
/// radians about the unit boresight `b`, along the great circle toward `d`.
/// `half_rad` must be strictly less than `theta` (the caller checks), so
/// the rim exists; an antiparallel `d` degenerates the great circle and the
/// clamp bears on the axis least aligned with `b` instead.
fn clamp_to_cone(b: [f64; 3], d: [f64; 3], half_rad: f64) -> [f64; 3] {
    let cos = b[0] * d[0] + b[1] * d[1] + b[2] * d[2];
    let perp = [d[0] - b[0] * cos, d[1] - b[1] * cos, d[2] - b[2] * cos];
    let perp_norm = (perp[0] * perp[0] + perp[1] * perp[1] + perp[2] * perp[2]).sqrt();
    let perp = if perp_norm <= f64::EPSILON {
        // Antiparallel command: no great circle prefers one rim direction,
        // so pick the deterministic perpendicular nearest the axis `b`
        // aligns with least.
        let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let axis = axes
            .iter()
            .min_by(|a, c| {
                let da = (b[0] * a[0] + b[1] * a[1] + b[2] * a[2]).abs();
                let dc = (b[0] * c[0] + b[1] * c[1] + b[2] * c[2]).abs();
                da.partial_cmp(&dc).expect("unit inputs are finite")
            })
            .expect("three axes");
        let c = [
            b[1] * axis[2] - b[2] * axis[1],
            b[2] * axis[0] - b[0] * axis[2],
            b[0] * axis[1] - b[1] * axis[0],
        ];
        let n = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
        [c[0] / n, c[1] / n, c[2] / n]
    } else {
        [
            perp[0] / perp_norm,
            perp[1] / perp_norm,
            perp[2] / perp_norm,
        ]
    };
    let (sin, cos) = half_rad.sin_cos();
    [
        b[0] * cos + perp[0] * sin,
        b[1] * cos + perp[1] * sin,
        b[2] * cos + perp[2] * sin,
    ]
}

/// Checks that `key` is a subsystem of the expected kind.
fn check_part(
    expected: SubsystemKind,
    key: SubsystemKey,
    graph: &SubsystemGraph,
) -> Result<(), CapitalError> {
    match graph.subsystem(&key) {
        None => Err(CapitalError::UnknownPart {
            kind: expected,
            key,
        }),
        Some(subsystem) if subsystem.kind != expected => Err(CapitalError::PartKindMismatch {
            key: subsystem.key.clone(),
            expected,
            actual: subsystem.kind,
        }),
        Some(_) => Ok(()),
    }
}

/// Builds a keyed map from a typed part list, checking each key's kind and
/// refusing duplicates.
fn collect<T>(
    expected: SubsystemKind,
    items: Vec<T>,
    graph: &SubsystemGraph,
    key_of: impl Fn(&T) -> SubsystemKey,
) -> Result<BTreeMap<SubsystemKey, T>, CapitalError> {
    let mut map = BTreeMap::new();
    for item in items {
        let key = key_of(&item);
        check_part(expected, key.clone(), graph)?;
        if map.insert(key.clone(), item).is_some() {
            return Err(CapitalError::DuplicatePart {
                kind: expected,
                key,
            });
        }
    }
    Ok(map)
}

/// The synthetic ship's straight-line trajectory: 20 m/s along +X at
/// 10 ticks/s for 1000 m, then held. Designed, not measured.
#[must_use]
pub fn synthetic_capital_trajectory() -> Trajectory {
    use crate::world_actors::Quat;
    use crate::world_actors::trajectory::Keyframe;
    Trajectory::new(
        vec![
            Keyframe {
                tick: Tick(0),
                position_m: [0.0, 0.0, 0.0],
                orientation: Quat::IDENTITY,
            },
            Keyframe {
                tick: Tick(500),
                position_m: [1000.0, 0.0, 0.0],
                orientation: Quat::IDENTITY,
            },
        ],
        10,
    )
    .expect("the synthetic capital trajectory is valid")
}
