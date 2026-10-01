//! Stunt traversal predicates and reward identity (F42-A).
//!
//! Spec: `specs/F42-stunts-fame-photos-and-optional-achievement-events.md`,
//! stage `### F42-A`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! This module is the **runtime half** of the stunt contract. `cs_sim` cannot
//! depend on `cs_content`, so it re-declares and re-validates its own
//! vocabulary (as `cs_sim::campaign::graph` mirrors
//! `cs_content::campaign`) and the conversion boundary in `cs_app::stunts`
//! fills it. The declared, provenance-carrying record is
//! `cs_content::stunts`.
//!
//! # The predicate
//!
//! [`Gate::classify`] answers one question about one swept movement segment:
//! did this aircraft *fly through* the gate? It requires all of
//!
//! 1. a swept segment — a teleport has no path and never earns
//!    ([`StuntMovement::Teleport`]), while a rebase keeps the same physical
//!    flight and still counts ([`StuntMovement::Rebased`], sheet behavior 1);
//! 2. a crossing of the gate's mid-plane inside the aperture
//!    ([`PassRefusal::MissedGate`]). The segment must actually *reach* the
//!    plane: one that stops inside the slab, turns back before it or runs
//!    parallel to it did not fly through anything, however deep inside the
//!    hole its endpoints happen to sit;
//! 3. a travel direction at least as aligned with the gate normal as the
//!    authored rule ([`PassRefusal::WrongDirection`]); and
//! 4. the authored margin from the nearest rim
//!    ([`PassRefusal::InsufficientClearance`]). A margin wider than the hole
//!    itself is refused as an unusable rule
//!    ([`GateRefusal::UnsatisfiableClearance`]) rather than as a near miss.
//!
//! The gate's in-plane `right`/`up` axes are **derived** from its normal by
//! [`Gate::new`], deterministically, so a lowered record and a test agree
//! about which side of a gate is "right" without either of them choosing.
//!
//! # The identity
//!
//! [`StuntRewardKey`] is `(profile, mission, stunt)` — the identity the sheet
//! names for one-time photo rewards. [`StuntLedger`] holds the keys a session
//! has already paid and is *seeded* from the persisted record, so a mission
//! retry (a new [`SessionGeneration`] with the same identity) cannot re-pay.
//! A key carries no session, no tick and no attempt number on purpose: those
//! would make every retry look like a new reward.
//!
//! # Designed, not measured
//!
//! Nothing here is an original measurement. The original stunt encoding, the
//! gate shapes, the direction/clearance thresholds, the fame and cash amounts
//! and the one-time-versus-repeatable rules are all unmeasured; see
//! `docs/findings/2026-10-01-f42-a-traversal-predicates-and-reward-identity.md`.

use std::collections::BTreeSet;
use std::fmt;

use cs_script::ir::ActorId;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::space::{SpaceError, UnitVec3, WorldPosition};

use crate::campaign::{ProfileId, SessionGeneration};

/// Below this length a swept segment is treated as no movement at all: a
/// sample that does not move cannot be a traversal.
const MIN_SWEEP_M: f64 = 1.0e-9;

/// Whether a declared gate's geometry may be used as evidence of original
/// behavior (sheet behavior 5).
///
/// The runtime half keeps the marking so a completion record can report
/// whether it was earned on a measured volume or on a drawn one; only
/// [`GateEvidence::Measured`] supports an original-behavior statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GateEvidence {
    /// Measured from original data or observation.
    Measured,
    /// A manually drawn replacement volume.
    Reconstructed,
    /// No geometry was recovered.
    NotRecovered,
}

impl GateEvidence {
    /// Whether a gate in this state may be treated as measured.
    #[must_use]
    pub const fn is_measured(self) -> bool {
        matches!(self, Self::Measured)
    }
}

/// Whether completing this stunt may affect mission success (sheet
/// behavior 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StuntCriticality {
    /// Supplementary; cannot force mission success.
    Optional,
    /// The authored mission depends on it.
    CriticalPath,
}

impl StuntCriticality {
    /// Whether a completion may affect mission success.
    #[must_use]
    pub const fn affects_mission_success(self) -> bool {
        matches!(self, Self::CriticalPath)
    }
}

/// Whether a second traversal of the same stunt pays again (sheet
/// behavior 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StuntRepeat {
    /// One payout per [`StuntRewardKey`], ever.
    Once,
    /// Every traversal pays.
    Repeatable,
}

/// What a completed traversal pays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StuntReward {
    /// Fame points.
    pub fame: u32,
    /// Cash in integer minor game units.
    pub cash_minor: u64,
    /// The scrapbook item this completion unlocks, if any.
    pub media: Option<ContentId>,
}

impl StuntReward {
    /// A reward that pays nothing and unlocks nothing.
    #[must_use]
    pub const fn nothing() -> Self {
        Self {
            fame: 0,
            cash_minor: 0,
            media: None,
        }
    }

    /// Whether this reward would change anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fame == 0 && self.cash_minor == 0 && self.media.is_none()
    }
}

/// The lowered traversal aperture: a centre, a unit normal, a derived
/// in-plane basis and the authored extents and thickness.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gate {
    center: WorldPosition,
    normal: UnitVec3,
    right: UnitVec3,
    up: UnitVec3,
    right_half_extent_m: f64,
    up_half_extent_m: f64,
    half_depth_m: f64,
}

impl Gate {
    /// Builds a gate, deriving its in-plane basis from the normal.
    ///
    /// The basis is derived, not authored: `right` is `normal × seed`
    /// normalized, where `seed` is canonical up `+Y` unless the normal is
    /// within 20° of it, in which case canonical forward `-Z` is used; `up`
    /// is `right × normal`. For the synthetic fixture's `-Z` normal that
    /// yields `right = +X` and `up = +Y`, so a test can assert the frame
    /// instead of trusting it.
    ///
    /// # Errors
    ///
    /// [`StuntError::BadGeometry`] for a zero-length normal, a non-positive
    /// or non-finite extent, or a negative or non-finite half depth, and
    /// [`StuntError::BadSpace`] for a rejected position or normal.
    pub fn new(
        center_m: [f64; 3],
        normal: [f64; 3],
        right_half_extent_m: f64,
        up_half_extent_m: f64,
        half_depth_m: f64,
    ) -> Result<Self, StuntError> {
        let center = WorldPosition::try_new(center_m).map_err(StuntError::BadSpace)?;
        if !normal.iter().all(|value| value.is_finite()) {
            return Err(StuntError::BadGeometry { field: "normal" });
        }
        let length: f64 = normal.iter().map(|value| value * value).sum::<f64>().sqrt();
        if length <= 0.0 || !length.is_finite() {
            return Err(StuntError::BadGeometry { field: "normal" });
        }
        let unit_normal =
            UnitVec3::try_new(normal.map(|value| value / length)).map_err(StuntError::BadSpace)?;
        if !right_half_extent_m.is_finite() || right_half_extent_m <= 0.0 {
            return Err(StuntError::BadGeometry {
                field: "right_half_extent_m",
            });
        }
        if !up_half_extent_m.is_finite() || up_half_extent_m <= 0.0 {
            return Err(StuntError::BadGeometry {
                field: "up_half_extent_m",
            });
        }
        if !half_depth_m.is_finite() || half_depth_m < 0.0 {
            return Err(StuntError::BadGeometry {
                field: "half_depth_m",
            });
        }

        let seed: [f64; 3] = if unit_normal.y().abs() > 0.94 {
            [0.0, 0.0, -1.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        let right = normalize(cross(unit_normal.to_array(), seed))
            .ok_or(StuntError::BadGeometry { field: "normal" })?;
        let up = normalize(cross(right, unit_normal.to_array()))
            .ok_or(StuntError::BadGeometry { field: "normal" })?;
        Ok(Self {
            center,
            normal: unit_normal,
            right: UnitVec3::try_new(right).map_err(StuntError::BadSpace)?,
            up: UnitVec3::try_new(up).map_err(StuntError::BadSpace)?,
            right_half_extent_m,
            up_half_extent_m,
            half_depth_m,
        })
    }

    /// The gate's centre.
    #[must_use]
    pub const fn center_m(&self) -> WorldPosition {
        self.center
    }

    /// The gate's unit normal: the direction a valid passage travels.
    #[must_use]
    pub const fn normal(&self) -> UnitVec3 {
        self.normal
    }

    /// The gate's derived in-plane `right` axis.
    #[must_use]
    pub const fn right(&self) -> UnitVec3 {
        self.right
    }

    /// The gate's derived in-plane `up` axis.
    #[must_use]
    pub const fn up(&self) -> UnitVec3 {
        self.up
    }

    /// Half the hole's width, in meters.
    #[must_use]
    pub const fn right_half_extent_m(&self) -> f64 {
        self.right_half_extent_m
    }

    /// Half the hole's height, in meters.
    #[must_use]
    pub const fn up_half_extent_m(&self) -> f64 {
        self.up_half_extent_m
    }

    /// Half the aperture's thickness, in meters.
    ///
    /// A traversal is a crossing of the mid-plane, so this records how thick
    /// the gate was drawn rather than changing the predicate. A zero half
    /// depth is a legal plane gate.
    #[must_use]
    pub const fn half_depth_m(&self) -> f64 {
        self.half_depth_m
    }

    /// The gate's coordinates of `point`: `(right, up, normal)` offsets from
    /// the centre, in meters.
    #[must_use]
    pub fn gate_frame(&self, point: WorldPosition) -> [f64; 3] {
        let offset = [
            point.x() - self.center.x(),
            point.y() - self.center.y(),
            point.z() - self.center.z(),
        ];
        [
            dot(offset, self.right.to_array()),
            dot(offset, self.up.to_array()),
            dot(offset, self.normal.to_array()),
        ]
    }

    /// Classifies one swept movement segment against this gate and the
    /// authored direction/clearance rules.
    ///
    /// The refusal is a [`GateRefusal`], which knows nothing about which
    /// stunt the gate belongs to: the geometry is one predicate and the
    /// identity belongs to the rule that owns it.
    ///
    /// # Errors
    ///
    /// [`GateRefusal`] naming the first rule the passage fails. A
    /// non-finite endpoint cannot be built at all, so there is no NaN branch
    /// here.
    pub fn classify(
        &self,
        from_m: WorldPosition,
        to_m: WorldPosition,
        min_forward_cosine: f64,
        min_clearance_m: f64,
    ) -> Result<Passage, GateRefusal> {
        // A threshold outside its documented range is a corrupt record. It is
        // re-checked here because `classify` is public and takes its
        // thresholds as plain values, so a caller can hand it something no
        // `TraversalRule` would ever hold.
        if !min_forward_cosine.is_finite()
            || !(-1.0..=1.0).contains(&min_forward_cosine)
            || !min_clearance_m.is_finite()
            || min_clearance_m < 0.0
        {
            return Err(GateRefusal::BadRule);
        }
        let segment = [
            to_m.x() - from_m.x(),
            to_m.y() - from_m.y(),
            to_m.z() - from_m.z(),
        ];
        let length: f64 = dot(segment, segment).sqrt();
        if length <= MIN_SWEEP_M {
            // A stationary sample moved nowhere, so it traversed nothing.
            return Err(GateRefusal::NoSweep);
        }
        let travel = segment.map(|value| value / length);
        let forward_cosine = dot(travel, self.normal.to_array());
        if forward_cosine < min_forward_cosine {
            return Err(GateRefusal::WrongDirection {
                forward_cosine,
                required: min_forward_cosine,
            });
        }

        // The crossing point of the gate's mid-plane along the segment. A
        // segment that does not reach the plane at all — one that stops short
        // of it, turns back before it, or runs parallel to it inside the slab
        // — never flew through the gate, so it is refused here rather than
        // clamped to its nearer endpoint: an endpoint that happens to lie
        // inside the slab would otherwise be credited as a traversal.
        let from_gate = self.gate_frame(from_m);
        let to_gate = self.gate_frame(to_m);
        let spans_plane = (from_gate[2] <= 0.0 && to_gate[2] >= 0.0)
            || (from_gate[2] >= 0.0 && to_gate[2] <= 0.0);
        if !spans_plane {
            return Err(GateRefusal::MissedGate);
        }
        // A segment that lies wholly in the mid-plane touches it everywhere;
        // its crossing is its own start. Otherwise the plane is reached
        // between the two endpoints, so the crossing parameter is inside the
        // segment by construction.
        let depth_span = from_gate[2] - to_gate[2];
        let t = if depth_span.abs() <= f64::EPSILON {
            0.0
        } else {
            (from_gate[2] / depth_span).clamp(0.0, 1.0)
        };
        let crossing_gate = [
            from_gate[0] + (to_gate[0] - from_gate[0]) * t,
            from_gate[1] + (to_gate[1] - from_gate[1]) * t,
            from_gate[2] + (to_gate[2] - from_gate[2]) * t,
        ];
        // The crossing is on the mid-plane by construction, so only the
        // in-plane extents decide whether it fell inside the hole. The
        // authored half depth records how thick the gate was drawn and is not
        // tested here, so a legal zero-thickness (plane) gate never refuses a
        // genuine crossing on the float residue of the interpolation.
        if crossing_gate[0].abs() > self.right_half_extent_m
            || crossing_gate[1].abs() > self.up_half_extent_m
        {
            return Err(GateRefusal::MissedGate);
        }

        let clearance_m = (self.right_half_extent_m - crossing_gate[0].abs())
            .min(self.up_half_extent_m - crossing_gate[1].abs());
        // A margin wider than the hole can never be met, so it is a corrupt
        // rule rather than a refusal of this particular flight: reporting it as
        // `InsufficientClearance` would blame the aircraft for an authored
        // impossibility.
        let max_possible_m = self.right_half_extent_m.min(self.up_half_extent_m);
        if min_clearance_m > max_possible_m {
            return Err(GateRefusal::UnsatisfiableClearance {
                required_m: min_clearance_m,
                max_possible_m,
            });
        }
        if clearance_m < min_clearance_m {
            return Err(GateRefusal::InsufficientClearance {
                clearance_m,
                required_m: min_clearance_m,
            });
        }
        Ok(Passage {
            crossing_m: [
                from_m.x() + segment[0] * t,
                from_m.y() + segment[1] * t,
                from_m.z() + segment[2] * t,
            ],
            forward_cosine,
            clearance_m,
        })
    }
}

/// A measured traversal of a gate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Passage {
    /// Where the segment crossed the gate's mid-plane, in meters.
    pub crossing_m: [f64; 3],
    /// The cosine between the travel direction and the gate normal.
    pub forward_cosine: f64,
    /// The distance from the crossing point to the nearest aperture rim, in
    /// meters. It is at least the authored minimum on a passage.
    pub clearance_m: f64,
}

/// The lowered traversal rule: a gate plus its authored direction and
/// clearance thresholds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraversalRule {
    gate: Gate,
    min_forward_cosine: f64,
    min_clearance_m: f64,
}

impl TraversalRule {
    /// Builds a rule.
    ///
    /// # Errors
    ///
    /// [`StuntError::BadRule`] when the direction rule is outside `[-1, 1]`
    /// or not finite, or when the clearance rule is negative or not finite;
    /// [`StuntError::UnsatisfiableClearance`] when the clearance margin is
    /// wider than the gate's own hole could ever offer, so no crossing could
    /// satisfy it.
    pub fn new(
        gate: Gate,
        min_forward_cosine: f64,
        min_clearance_m: f64,
    ) -> Result<Self, StuntError> {
        if !min_forward_cosine.is_finite() || !(-1.0..=1.0).contains(&min_forward_cosine) {
            return Err(StuntError::BadRule {
                field: "min_forward_cosine",
            });
        }
        if !min_clearance_m.is_finite() || min_clearance_m < 0.0 {
            return Err(StuntError::BadRule {
                field: "min_clearance_m",
            });
        }
        let max_possible_m = gate.right_half_extent_m.min(gate.up_half_extent_m);
        if min_clearance_m > max_possible_m {
            return Err(StuntError::UnsatisfiableClearance {
                required_m: min_clearance_m,
                max_possible_m,
            });
        }
        Ok(Self {
            gate,
            min_forward_cosine,
            min_clearance_m,
        })
    }

    /// The gate this rule tests.
    #[must_use]
    pub const fn gate(&self) -> &Gate {
        &self.gate
    }

    /// The authored direction threshold.
    #[must_use]
    pub const fn min_forward_cosine(&self) -> f64 {
        self.min_forward_cosine
    }

    /// The authored rim margin, in meters.
    #[must_use]
    pub const fn min_clearance_m(&self) -> f64 {
        self.min_clearance_m
    }

    /// Classifies a swept segment against this rule.
    ///
    /// # Errors
    ///
    /// [`GateRefusal`] naming the first rule the passage fails.
    pub fn classify(
        &self,
        from_m: WorldPosition,
        to_m: WorldPosition,
    ) -> Result<Passage, GateRefusal> {
        self.gate
            .classify(from_m, to_m, self.min_forward_cosine, self.min_clearance_m)
    }
}

/// One lowered stunt: a rule, the missions that declare it, its criticality,
/// its repeat policy, its reward and its geometry marking.
///
/// It is `PartialEq` but not `Eq`: the rule holds authored floating-point
/// extents and thresholds, and `f64` has no total equality.
#[derive(Clone, Debug, PartialEq)]
pub struct StuntRule {
    id: ContentId,
    world: ContentId,
    rule: TraversalRule,
    missions: Vec<ContentId>,
    criticality: StuntCriticality,
    repeat: StuntRepeat,
    reward: StuntReward,
    evidence: GateEvidence,
}

/// The raw parts of a [`StuntRule`], collected so the validating constructor
/// takes one record instead of a long positional argument list.
#[derive(Clone, Debug, PartialEq)]
pub struct StuntRuleDraft {
    /// The `stunt` content id.
    pub id: ContentId,
    /// The `world` the gate lives in.
    pub world: ContentId,
    /// The traversal rule.
    pub rule: TraversalRule,
    /// The missions that declare the stunt; non-empty and unique.
    pub missions: Vec<ContentId>,
    /// Whether a completion may affect mission success.
    pub criticality: StuntCriticality,
    /// Whether a second traversal pays again.
    pub repeat: StuntRepeat,
    /// What a completion pays.
    pub reward: StuntReward,
    /// The geometry's evidence marking.
    pub evidence: GateEvidence,
}

impl StuntRule {
    /// Validates and assembles a lowered stunt.
    ///
    /// # Errors
    ///
    /// [`StuntError::NotAStunt`], [`StuntError::NotAWorld`],
    /// [`StuntError::NotAMission`], [`StuntError::EmptyMissionScope`] or
    /// [`StuntError::DuplicateMissionScope`].
    pub fn try_new(draft: StuntRuleDraft) -> Result<Self, StuntError> {
        let StuntRuleDraft {
            id,
            world,
            rule,
            missions,
            criticality,
            repeat,
            reward,
            evidence,
        } = draft;
        if id.kind() != ContentKind::Stunt {
            return Err(StuntError::NotAStunt { kind: id.kind() });
        }
        if world.kind() != ContentKind::World {
            return Err(StuntError::NotAWorld { kind: world.kind() });
        }
        if missions.is_empty() {
            return Err(StuntError::EmptyMissionScope);
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for mission in &missions {
            if mission.kind() != ContentKind::Mission {
                return Err(StuntError::NotAMission {
                    kind: mission.kind(),
                });
            }
            if !seen.insert(mission.as_str()) {
                return Err(StuntError::DuplicateMissionScope {
                    mission: mission.as_str().to_owned(),
                });
            }
        }
        Ok(Self {
            id,
            world,
            rule,
            missions,
            criticality,
            repeat,
            reward,
            evidence,
        })
    }

    /// The `stunt` content id.
    #[must_use]
    pub const fn id(&self) -> &ContentId {
        &self.id
    }

    /// The `world` the gate lives in.
    #[must_use]
    pub const fn world(&self) -> &ContentId {
        &self.world
    }

    /// The traversal rule.
    #[must_use]
    pub const fn rule(&self) -> &TraversalRule {
        &self.rule
    }

    /// The declared missions, in authored order.
    #[must_use]
    pub fn missions(&self) -> &[ContentId] {
        &self.missions
    }

    /// Whether a completion may affect mission success.
    #[must_use]
    pub const fn criticality(&self) -> StuntCriticality {
        self.criticality
    }

    /// Whether a second traversal pays again.
    #[must_use]
    pub const fn repeat(&self) -> StuntRepeat {
        self.repeat
    }

    /// What a completion pays.
    #[must_use]
    pub const fn reward(&self) -> &StuntReward {
        &self.reward
    }

    /// The geometry's evidence marking.
    #[must_use]
    pub const fn evidence(&self) -> GateEvidence {
        self.evidence
    }

    /// Whether `mission` declares this stunt.
    #[must_use]
    pub fn is_eligible_in(&self, mission: &ContentId) -> bool {
        self.missions.iter().any(|declared| declared == mission)
    }
}

/// How one tick's movement got from the previous position to the current
/// one.
///
/// [`Swept`](StuntMovement::Swept) and [`Rebased`](StuntMovement::Rebased)
/// are both continuous: a rebase moves the origin frame and keeps **world
/// identity** — both endpoints — unchanged (F16 `OriginChange::Rebase`
/// preserves swept continuity), so the same physical flight must still
/// traverse the same gate after the frame changed. The two variants are
/// distinct values rather than one boolean so a caller cannot invert the
/// distinction by accident, and so a report can say which it observed.
///
/// [`Teleport`](StuntMovement::Teleport) is discontinuous: it has no path
/// and can never earn, however far it jumps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StuntMovement {
    /// One tick of continuous flight.
    Swept {
        /// Where the aircraft was at the previous tick, in world identity.
        from_m: WorldPosition,
        /// Where it is now, in world identity.
        to_m: WorldPosition,
    },
    /// The same continuous flight, carried through an origin rebase. The two
    /// endpoints are the same world positions the previous sample carried:
    /// only the f32 local frame the physics and renderer work in moved, and
    /// the gate is a world-identity volume, so the traversal is unaffected.
    Rebased {
        /// The previous endpoint, in world identity.
        from_m: WorldPosition,
        /// The current endpoint, in world identity.
        to_m: WorldPosition,
    },
    /// A discontinuous move: no path exists, so no gate can be earned.
    Teleport {
        /// Where the aircraft is now.
        to_m: WorldPosition,
    },
}

impl StuntMovement {
    /// The swept endpoints, or [`None`] for a teleport.
    #[must_use]
    pub const fn swept(&self) -> Option<(WorldPosition, WorldPosition)> {
        match *self {
            Self::Swept { from_m, to_m } | Self::Rebased { from_m, to_m } => Some((from_m, to_m)),
            Self::Teleport { .. } => None,
        }
    }

    /// Whether this movement preserves a swept path.
    #[must_use]
    pub const fn is_continuous(&self) -> bool {
        self.swept().is_some()
    }
}

/// Which authority produced a movement sample (sheet behavior 1: a
/// teleport or developer camera movement cannot earn a stunt).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StuntAuthority {
    /// The aircraft the session bound to flight input.
    PlayerFlight,
    /// An AI-controlled aircraft. Its flight is real, but it is not the
    /// player-authored traversal the reward is for.
    AiFlight,
    /// A developer/free camera. Never earns.
    DeveloperCamera,
    /// A spectator or replay view. Never earns.
    Spectator,
}

impl StuntAuthority {
    /// Whether a sample from this authority can earn.
    #[must_use]
    pub const fn can_earn(self) -> bool {
        matches!(self, Self::PlayerFlight)
    }
}

/// One tick's traversal sample for one actor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraversalRequest {
    /// The session generation the sample belongs to. A delayed callback from
    /// a previous attempt carries an older generation and is refused.
    pub session: SessionGeneration,
    /// Which actor moved.
    pub actor: ActorId,
    /// The tick the sample was taken at.
    pub tick: Tick,
    /// How it moved.
    pub movement: StuntMovement,
    /// Which authority produced it.
    pub authority: StuntAuthority,
}

impl TraversalRequest {
    /// A player-flight sample for `actor` at `tick`.
    #[must_use]
    pub const fn player_flight(
        session: SessionGeneration,
        actor: ActorId,
        tick: Tick,
        movement: StuntMovement,
    ) -> Self {
        Self {
            session,
            actor,
            tick,
            movement,
            authority: StuntAuthority::PlayerFlight,
        }
    }
}

/// The stable identity of one stunt reward: `(profile, mission, stunt)`.
///
/// The session generation, the tick and the attempt number are deliberately
/// **not** part of the key. They would make a mission retry look like a new
/// reward, and a one-time photo would then pay again every attempt.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StuntRewardKey {
    /// The profile the reward belongs to.
    pub profile: ProfileId,
    /// The mission the stunt was earned in.
    pub mission: ContentId,
    /// The stunt that was earned.
    pub stunt: ContentId,
}

impl StuntRewardKey {
    /// Builds the key.
    #[must_use]
    pub const fn new(profile: ProfileId, mission: ContentId, stunt: ContentId) -> Self {
        Self {
            profile,
            mission,
            stunt,
        }
    }
}

/// Whether a reward was granted or refused as a duplicate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    /// The reward was granted for the first time.
    Admitted,
    /// The identity already paid; nothing was granted.
    AlreadyRewarded,
}

/// The per-session record of which reward identities have already paid.
///
/// A ledger is **seeded** with the identities the persisted profile already
/// holds, which is what makes a mission retry (a new
/// [`SessionGeneration`], the same identity) refuse to pay again.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StuntLedger {
    generation: Option<SessionGeneration>,
    awarded: BTreeSet<StuntRewardKey>,
}

impl StuntLedger {
    /// A ledger for one session, seeded with the identities the profile
    /// already holds.
    #[must_use]
    pub fn new(
        session: SessionGeneration,
        already_awarded: impl IntoIterator<Item = StuntRewardKey>,
    ) -> Self {
        Self {
            generation: Some(session),
            awarded: already_awarded.into_iter().collect(),
        }
    }

    /// The session this ledger belongs to.
    #[must_use]
    pub const fn generation(&self) -> Option<SessionGeneration> {
        self.generation
    }

    /// How many identities have paid.
    #[must_use]
    pub fn len(&self) -> usize {
        self.awarded.len()
    }

    /// Whether no identity has paid.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.awarded.is_empty()
    }

    /// Whether `key` has already paid.
    #[must_use]
    pub fn has_paid(&self, key: &StuntRewardKey) -> bool {
        self.awarded.contains(key)
    }

    /// The paid identities, in stable order.
    pub fn paid(&self) -> impl Iterator<Item = &StuntRewardKey> {
        self.awarded.iter()
    }

    /// Grants `key` for `session`, or refuses it as a duplicate.
    ///
    /// A sample from a different session generation than the ledger's is
    /// refused rather than merged: a replayed or delayed result must not
    /// write into a session that is not its own.
    ///
    /// # Errors
    ///
    /// [`StuntError::StaleSession`] when `session` is not the ledger's
    /// generation. The ledger is unchanged.
    pub fn grant(
        &mut self,
        session: SessionGeneration,
        key: StuntRewardKey,
    ) -> Result<Admission, StuntError> {
        if self.generation != Some(session) {
            return Err(StuntError::StaleSession {
                ledger: self.generation,
                given: session,
            });
        }
        if !self.awarded.insert(key) {
            return Ok(Admission::AlreadyRewarded);
        }
        Ok(Admission::Admitted)
    }
}

/// One completed traversal and the identity its reward is paid under.
#[derive(Clone, Debug, PartialEq)]
pub struct TraversalCompletion {
    /// The stunt that was completed.
    pub stunt: ContentId,
    /// The mission it was earned in.
    pub mission: ContentId,
    /// The actor that flew it.
    pub actor: ActorId,
    /// The tick it completed on.
    pub tick: Tick,
    /// Where the gate was crossed, in meters.
    pub crossing_m: [f64; 3],
    /// The cosine between the travel direction and the gate normal.
    pub forward_cosine: f64,
    /// The margin from the nearest rim, in meters.
    pub clearance_m: f64,
    /// Whether the geometry that earned it was measured or drawn.
    pub evidence: GateEvidence,
    /// Whether this completion may affect mission success.
    pub criticality: StuntCriticality,
    /// The identity the reward is paid under.
    pub key: StuntRewardKey,
    /// What it pays.
    pub reward: StuntReward,
}

/// Why one segment did not traverse one gate.
///
/// This is the geometry half of the refusal vocabulary and it names no stunt:
/// the aperture knows its own shape and the authored thresholds, and the
/// identity of the record that owns it belongs to [`PassRefusal`].
#[derive(Clone, Debug, PartialEq)]
pub enum GateRefusal {
    /// The sample did not move, so it traversed nothing.
    NoSweep,
    /// The segment did not cross the gate's mid-plane inside the aperture: it
    /// missed the hole, stopped short of the plane, turned back before it or
    /// ran parallel to it.
    MissedGate,
    /// The travel direction was too far from the gate normal.
    WrongDirection {
        /// The measured cosine between travel and normal.
        forward_cosine: f64,
        /// The authored minimum.
        required: f64,
    },
    /// The crossing was inside the aperture but inside the authored margin.
    InsufficientClearance {
        /// The measured margin from the nearest rim, in meters.
        clearance_m: f64,
        /// The authored minimum, in meters.
        required_m: f64,
    },
    /// The authored rim margin is wider than the gate's own hole, so no
    /// crossing of this gate could ever satisfy it.
    ///
    /// This is a defect in the authored record rather than a statement about
    /// the flight, and it is reported instead of silently refusing every
    /// passage as [`GateRefusal::InsufficientClearance`].
    UnsatisfiableClearance {
        /// The authored minimum margin, in meters.
        required_m: f64,
        /// The widest margin the aperture allows, in meters.
        max_possible_m: f64,
    },
    /// A threshold handed to the predicate was non-finite, out of range or
    /// negative. The record is corrupt, not the flight.
    BadRule,
}

impl fmt::Display for GateRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSweep => write!(f, "the sample did not move, so it traversed nothing"),
            Self::MissedGate => write!(
                f,
                "the segment did not cross the gate's mid-plane inside the aperture"
            ),
            Self::WrongDirection {
                forward_cosine,
                required,
            } => write!(
                f,
                "the travel direction is {forward_cosine} off the gate normal, \
                 below the authored minimum {required}"
            ),
            Self::InsufficientClearance {
                clearance_m,
                required_m,
            } => write!(
                f,
                "the crossing keeps {clearance_m} m from the nearest rim, \
                 below the authored minimum {required_m} m"
            ),
            Self::UnsatisfiableClearance {
                required_m,
                max_possible_m,
            } => write!(
                f,
                "the authored minimum margin {required_m} m exceeds the {max_possible_m} m \
                 this aperture can ever offer, so the rule is unusable"
            ),
            Self::BadRule => write!(f, "the traversal rule carries an unusable threshold"),
        }
    }
}

impl std::error::Error for GateRefusal {}

/// Why a traversal did not complete a stunt.
///
/// Every variant names the rule that refused it — the geometry measurement for
/// a geometric failure, the identity for a duplicate — so a caller can report
/// *why* a pass did not count instead of only that it did not. The variants
/// are ordered from the most fundamental (this actor, this authority, this
/// mission) to the most geometric.
#[derive(Clone, Debug, PartialEq)]
pub enum PassRefusal {
    /// The sample is not the actor the session bound to flight input.
    ForeignActor {
        /// The actor that moved.
        actor: ActorId,
        /// The session's subject.
        subject: ActorId,
    },
    /// The sample's authority can never earn.
    NotPlayerFlight {
        /// The authority that produced it.
        authority: StuntAuthority,
    },
    /// The mission does not declare this stunt.
    MissionNotEligible {
        /// The stunt that was flown.
        stunt: String,
        /// The mission that was being played.
        mission: String,
    },
    /// The movement was discontinuous, so it has no path through anything.
    Discontinuous {
        /// The stunt that was not earned.
        stunt: String,
    },
    /// The gate's own geometry refused the segment. The measurement travels
    /// with the refusal, so a caller can report *how* close the pass was.
    Geometry {
        /// The stunt whose gate was not traversed.
        stunt: String,
        /// The named geometric failure.
        reason: GateRefusal,
    },
    /// The identity already paid, so the traversal completed the stunt but
    /// pays nothing. It is a refusal rather than a completion so a caller
    /// cannot pay twice by treating it as a fresh award.
    AlreadyRewarded {
        /// The stunt that was flown again.
        stunt: String,
        /// The identity that had already paid.
        key: StuntRewardKey,
    },
}

impl PassRefusal {
    /// The stunt this refusal is about, when it is about one.
    #[must_use]
    pub fn stunt(&self) -> Option<&str> {
        match self {
            Self::ForeignActor { .. } | Self::NotPlayerFlight { .. } => None,
            Self::MissionNotEligible { stunt, .. }
            | Self::Discontinuous { stunt }
            | Self::Geometry { stunt, .. }
            | Self::AlreadyRewarded { stunt, .. } => Some(stunt),
        }
    }
}

impl fmt::Display for PassRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignActor { actor, subject } => write!(
                f,
                "actor {} is not the session's subject actor {}",
                actor.0, subject.0
            ),
            Self::NotPlayerFlight { authority } => {
                write!(f, "{authority:?} authority cannot earn a stunt")
            }
            Self::MissionNotEligible { stunt, mission } => {
                write!(f, "mission {mission:?} does not declare stunt {stunt:?}")
            }
            Self::Discontinuous { stunt } => {
                write!(
                    f,
                    "stunt {stunt:?} was not earned: the movement has no path"
                )
            }
            Self::Geometry { stunt, reason } => {
                write!(f, "stunt {stunt:?} was not earned: {reason}")
            }
            Self::AlreadyRewarded { key, .. } => {
                write!(
                    f,
                    "stunt already paid as reward identity {profile}/{mission}/{stunt}",
                    profile = key.profile,
                    mission = key.mission,
                    stunt = key.stunt
                )
            }
        }
    }
}

impl std::error::Error for PassRefusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Geometry { reason, .. } => Some(reason),
            _ => None,
        }
    }
}

/// The result of one sample against one rule.
///
/// A completion is boxed: it carries the whole reward identity (three content
/// ids and a profile id) next to its measurement, so keeping it inline would
/// make every *refusal* — the common case, one per rule per tick — pay for
/// that size.
#[derive(Clone, Debug, PartialEq)]
pub enum TraversalOutcome {
    /// The stunt was completed and the reward granted (or found already
    /// granted, which is reported as a refusal below so a caller cannot pay
    /// twice by accident).
    Completed(Box<TraversalCompletion>),
    /// The stunt was not completed, for a named reason.
    Refused(PassRefusal),
}

/// Why an observation was refused outright, without judging any rule.
#[derive(Clone, Debug, PartialEq)]
pub enum StuntObserveError {
    /// The sample belongs to a different session than the book.
    StaleSession {
        /// The book's generation.
        book: SessionGeneration,
        /// The sample's generation.
        given: SessionGeneration,
    },
    /// The sample's tick is not after the last observed one.
    NotAdvancing {
        /// The last observed tick.
        last: Tick,
        /// The sample's tick.
        given: Tick,
    },
    /// Two rules share one stunt id, so a sample would be ambiguous.
    DuplicateStunt {
        /// The duplicated id.
        id: String,
    },
    /// The reward ledger does not belong to the book's session generation.
    LedgerSession {
        /// The session the book is for.
        book: SessionGeneration,
        /// The session the ledger is for, or [`None`] for an unopened
        /// ledger.
        ledger: Option<SessionGeneration>,
    },
    /// The rule set or reward ledger is corrupt, so a sample cannot be
    /// judged. This is a defect in the lowered record, not in the flight.
    CorruptLedger {
        /// The named underlying reason.
        reason: String,
    },
}

impl fmt::Display for StuntObserveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleSession { book, given } => write!(
                f,
                "traversal sample is from session generation {}, but the book is session generation {}",
                given.0, book.0
            ),
            Self::NotAdvancing { last, given } => {
                write!(
                    f,
                    "tick {} does not advance the observed tick {}",
                    given.0, last.0
                )
            }
            Self::DuplicateStunt { id } => {
                write!(f, "stunt id {id:?} appears more than once in one book")
            }
            Self::LedgerSession { book, ledger } => write!(
                f,
                "the reward ledger is session generation {}, but the book is session generation {}",
                ledger.map_or(0, |generation| generation.0),
                book.0
            ),
            Self::CorruptLedger { reason } => {
                write!(f, "the stunt book is corrupt: {reason}")
            }
        }
    }
}

impl std::error::Error for StuntObserveError {}

/// One session's stunt book: the mission's rules, its subject, its reward
/// ledger and how many stunts it has completed.
///
/// The book is the single place a completion is counted, so "only eligible
/// traversals count" is one code path rather than a convention spread over
/// callers. A retry is a **new** book over the same declared set, seeded
/// from the persisted reward keys, which is what stops a one-time photo from
/// paying again.
#[derive(Clone, Debug, PartialEq)]
pub struct StuntBook {
    session: SessionGeneration,
    profile: ProfileId,
    mission: ContentId,
    subject: ActorId,
    rules: Vec<StuntRule>,
    ledger: StuntLedger,
    last_tick: Option<Tick>,
    completions: u32,
}

impl StuntBook {
    /// Builds a book for one session.
    ///
    /// The ledger must belong to `session`: pairing it with a book that could
    /// never write into it would turn every later grant into a
    /// mid-traversal failure, after the book had already advanced its tick.
    ///
    /// # Errors
    ///
    /// [`StuntObserveError::DuplicateStunt`] when two rules share one stunt
    /// id, [`StuntObserveError::LedgerSession`] when `ledger` does not belong
    /// to `session`, and [`StuntError::NotAMission`] when `mission` is not a
    /// `ContentKind::Mission` id.
    pub fn new(
        session: SessionGeneration,
        profile: ProfileId,
        mission: ContentId,
        subject: ActorId,
        rules: Vec<StuntRule>,
        ledger: StuntLedger,
    ) -> Result<Self, StuntObserveError> {
        if mission.kind() != ContentKind::Mission {
            return Err(StuntError::NotAMission {
                kind: mission.kind(),
            }
            .into());
        }
        if ledger.generation() != Some(session) {
            return Err(StuntObserveError::LedgerSession {
                book: session,
                ledger: ledger.generation(),
            });
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for rule in &rules {
            if !seen.insert(rule.id().as_str()) {
                return Err(StuntObserveError::DuplicateStunt {
                    id: rule.id().as_str().to_owned(),
                });
            }
        }
        Ok(Self {
            session,
            profile,
            mission,
            subject,
            rules,
            ledger,
            last_tick: None,
            completions: 0,
        })
    }

    /// The session this book belongs to.
    #[must_use]
    pub const fn session(&self) -> SessionGeneration {
        self.session
    }

    /// The mission being played.
    #[must_use]
    pub const fn mission(&self) -> &ContentId {
        &self.mission
    }

    /// The actor the session bound to flight input.
    #[must_use]
    pub const fn subject(&self) -> ActorId {
        self.subject
    }

    /// How many stunts this session has completed.
    #[must_use]
    pub const fn completions(&self) -> u32 {
        self.completions
    }

    /// The reward ledger.
    #[must_use]
    pub const fn ledger(&self) -> &StuntLedger {
        &self.ledger
    }

    /// Observes one tick's sample and returns one outcome per declared rule,
    /// in authored order.
    ///
    /// A rule is judged only when the sample could have earned it: the actor
    /// must be the subject, the authority must be player flight, the mission
    /// must declare the stunt and the movement must be continuous. Each
    /// eligible rule is then classified, and a completed one grants its
    /// reward identity — refusing a duplicate rather than paying twice.
    ///
    /// # Errors
    ///
    /// [`StuntObserveError`] for a stale session or a non-advancing tick. The
    /// book is unchanged: both are checked before any rule is judged, so a
    /// refused sample cannot advance the book or grant an identity.
    ///
    /// The grant below cannot fail either — [`StuntBook::new`] only accepts a
    /// ledger that belongs to this book's session — so no rule can be paid
    /// before a later one fails.
    pub fn observe(
        &mut self,
        request: &TraversalRequest,
    ) -> Result<Vec<TraversalOutcome>, StuntObserveError> {
        if request.session != self.session {
            return Err(StuntObserveError::StaleSession {
                book: self.session,
                given: request.session,
            });
        }
        if let Some(last) = self.last_tick
            && request.tick <= last
        {
            return Err(StuntObserveError::NotAdvancing {
                last,
                given: request.tick,
            });
        }
        self.last_tick = Some(request.tick);

        let mut outcomes = Vec::with_capacity(self.rules.len());
        for rule in &self.rules {
            let id = rule.id().as_str().to_owned();
            let outcome = if request.actor != self.subject {
                TraversalOutcome::Refused(PassRefusal::ForeignActor {
                    actor: request.actor,
                    subject: self.subject,
                })
            } else if !request.authority.can_earn() {
                TraversalOutcome::Refused(PassRefusal::NotPlayerFlight {
                    authority: request.authority,
                })
            } else if !rule.is_eligible_in(&self.mission) {
                TraversalOutcome::Refused(PassRefusal::MissionNotEligible {
                    stunt: id,
                    mission: self.mission.as_str().to_owned(),
                })
            } else if let Some((from_m, to_m)) = request.movement.swept() {
                match rule.rule().classify(from_m, to_m) {
                    Err(reason) => {
                        TraversalOutcome::Refused(PassRefusal::Geometry { stunt: id, reason })
                    }
                    Ok(passage) => {
                        let key = StuntRewardKey::new(
                            self.profile.clone(),
                            self.mission.clone(),
                            rule.id().clone(),
                        );
                        let admission = self
                            .ledger
                            .grant(self.session, key.clone())
                            .map_err(StuntObserveError::from)?;
                        // A repeatable stunt pays on every pass, so its repeat
                        // grant is a payment and not a duplicate. A one-time
                        // stunt that already paid is refused instead of
                        // counted, so a caller cannot pay twice by reading a
                        // completion as a fresh award.
                        let duplicate = admission == Admission::AlreadyRewarded
                            && rule.repeat() == StuntRepeat::Once;
                        if duplicate {
                            TraversalOutcome::Refused(PassRefusal::AlreadyRewarded {
                                stunt: id,
                                key,
                            })
                        } else {
                            self.completions += 1;
                            TraversalOutcome::Completed(Box::new(TraversalCompletion {
                                stunt: rule.id().clone(),
                                mission: self.mission.clone(),
                                actor: request.actor,
                                tick: request.tick,
                                crossing_m: passage.crossing_m,
                                forward_cosine: passage.forward_cosine,
                                clearance_m: passage.clearance_m,
                                evidence: rule.evidence(),
                                criticality: rule.criticality(),
                                key,
                                reward: rule.reward().clone(),
                            }))
                        }
                    }
                }
            } else {
                TraversalOutcome::Refused(PassRefusal::Discontinuous { stunt: id })
            };
            outcomes.push(outcome);
        }
        Ok(outcomes)
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(value: [f64; 3]) -> Option<[f64; 3]> {
    let length: f64 = dot(value, value).sqrt();
    if length <= 0.0 || !length.is_finite() {
        return None;
    }
    Some(value.map(|component| component / length))
}

/// Why a lowered stunt or rule was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum StuntError {
    /// The id is not in the `stunt` namespace.
    NotAStunt {
        /// The kind it actually names.
        kind: ContentKind,
    },
    /// The world id is not in the `world` namespace.
    NotAWorld {
        /// The kind it actually names.
        kind: ContentKind,
    },
    /// A mission scope entry, or the book itself, is not a mission.
    NotAMission {
        /// The kind it actually names.
        kind: ContentKind,
    },
    /// A gate field is unusable.
    BadGeometry {
        /// Which field is corrupt.
        field: &'static str,
    },
    /// A rule threshold is unusable.
    BadRule {
        /// Which threshold is corrupt.
        field: &'static str,
    },
    /// The authored clearance margin is wider than the gate's own hole, so no
    /// crossing of it could ever satisfy the rule.
    UnsatisfiableClearance {
        /// The authored minimum margin, in meters.
        required_m: f64,
        /// The widest margin the aperture can offer, in meters.
        max_possible_m: f64,
    },
    /// A canonical position or normal was rejected.
    BadSpace(SpaceError),
    /// A stunt declared no eligible mission.
    EmptyMissionScope,
    /// A mission appears twice in one scope.
    DuplicateMissionScope {
        /// The duplicated mission key.
        mission: String,
    },
    /// A grant named a session generation the ledger does not belong to.
    StaleSession {
        /// The ledger's generation, or [`None`] for an unopened ledger.
        ledger: Option<SessionGeneration>,
        /// The generation the caller named.
        given: SessionGeneration,
    },
}

impl From<StuntError> for StuntObserveError {
    fn from(value: StuntError) -> Self {
        match value {
            // A ledger built by `StuntLedger::new` always carries its
            // generation; an unopened one carries none, which this path cannot
            // reach from a book.
            StuntError::StaleSession {
                ledger: Some(book),
                given,
            } => Self::StaleSession { book, given },
            StuntError::StaleSession {
                ledger: None,
                given,
            } => Self::CorruptLedger {
                reason: format!(
                    "the ledger is unopened, so it cannot pay generation {}",
                    given.0
                ),
            },
            other => Self::CorruptLedger {
                reason: other.to_string(),
            },
        }
    }
}

impl fmt::Display for StuntError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAStunt { kind } => write!(f, "stunt id names a {kind}, not a stunt"),
            Self::NotAWorld { kind } => write!(f, "stunt world names a {kind}, not a world"),
            Self::NotAMission { kind } => write!(f, "mission id names a {kind}, not a mission"),
            Self::BadGeometry { field } => {
                write!(f, "stunt gate geometry field {field} is unusable")
            }
            Self::BadRule { field } => {
                write!(f, "stunt rule threshold {field} is not a usable value")
            }
            Self::UnsatisfiableClearance {
                required_m,
                max_possible_m,
            } => write!(
                f,
                "min_clearance_m {required_m} exceeds the {max_possible_m} this gate can offer"
            ),
            Self::BadSpace(error) => write!(f, "stunt position was rejected: {error}"),
            Self::EmptyMissionScope => {
                write!(f, "a stunt must declare at least one eligible mission")
            }
            Self::DuplicateMissionScope { mission } => {
                write!(f, "mission {mission:?} appears more than once in one scope")
            }
            Self::StaleSession { ledger, given } => write!(
                f,
                "grant names session generation {}, but the ledger is generation {}",
                given.0,
                ledger.map_or(0, |generation| generation.0)
            ),
        }
    }
}

impl std::error::Error for StuntError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::BadSpace(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The derived in-plane basis is deterministic and gives the synthetic
    /// gate's `-Z` normal the canonical `+X` right and `+Y` up axes, so
    /// "which side is right" is a property of the code and not of a caller.
    #[test]
    fn accept_f42_a_gate_basis_is_derived_and_deterministic() {
        let gate = Gate::new([0.0; 3], [0.0, 0.0, -2.0], 12.0, 8.0, 4.0).expect("a usable gate");
        assert_eq!(gate.normal().to_array(), [0.0, 0.0, -1.0]);
        assert_eq!(gate.right().to_array(), [1.0, 0.0, 0.0]);
        assert_eq!(gate.up().to_array(), [0.0, 1.0, 0.0]);
        // A near-vertical normal switches the seed axis and still yields an
        // orthonormal frame instead of a zero-length right vector.
        let vertical = Gate::new([0.0; 3], [0.0, 1.0, 0.0], 12.0, 8.0, 4.0).expect("a usable gate");
        assert_eq!(vertical.normal().to_array(), [0.0, 1.0, 0.0]);
        let right = vertical.right().to_array();
        assert!(dot(right, [0.0, 1.0, 0.0]).abs() < 1.0e-12);
        assert!((dot(right, right) - 1.0).abs() < 1.0e-12);
    }

    /// A zero-length normal, a non-positive extent and a negative depth are
    /// each refused by name rather than normalized into a usable gate.
    #[test]
    fn accept_f42_a_gate_refuses_unusable_geometry() {
        assert_eq!(
            Gate::new([0.0; 3], [0.0, 0.0, 0.0], 12.0, 8.0, 4.0),
            Err(StuntError::BadGeometry { field: "normal" })
        );
        assert_eq!(
            Gate::new([0.0; 3], [0.0, 0.0, -1.0], 0.0, 8.0, 4.0),
            Err(StuntError::BadGeometry {
                field: "right_half_extent_m"
            })
        );
        assert_eq!(
            Gate::new([0.0; 3], [0.0, 0.0, -1.0], 12.0, 8.0, -1.0),
            Err(StuntError::BadGeometry {
                field: "half_depth_m"
            })
        );
        assert_eq!(
            Gate::new([f64::NAN, 0.0, 0.0], [0.0, 0.0, -1.0], 12.0, 8.0, 4.0),
            Err(StuntError::BadSpace(SpaceError::NonFinite {
                field: "world.x",
            }))
        );
        assert_eq!(
            TraversalRule::new(
                Gate::new([0.0; 3], [0.0, 0.0, -1.0], 12.0, 8.0, 4.0).expect("a usable gate"),
                1.5,
                0.0
            ),
            Err(StuntError::BadRule {
                field: "min_forward_cosine"
            })
        );
    }

    /// The ledger grants an identity once, is order independent, and refuses
    /// a grant that names another session's generation without changing its
    /// state.
    #[test]
    fn accept_f42_a_ledger_grants_once_and_refuses_a_stale_session() {
        let session = SessionGeneration(1);
        let key = StuntRewardKey::new(
            ProfileId::new("profile-1").expect("a valid profile id"),
            ContentId::from_source(ContentKind::Mission, "synthetic.m01").expect("valid id"),
            ContentId::from_source(ContentKind::Stunt, "synthetic.flyby-gate").expect("valid id"),
        );
        let mut ledger = StuntLedger::new(session, Vec::new());
        assert!(ledger.is_empty());
        assert_eq!(ledger.grant(session, key.clone()), Ok(Admission::Admitted));
        assert_eq!(
            ledger.grant(session, key.clone()),
            Ok(Admission::AlreadyRewarded)
        );
        assert_eq!(ledger.len(), 1);
        assert!(ledger.has_paid(&key));
        assert_eq!(ledger.generation(), Some(session));

        // A grant from another session never merges into this one.
        let other = SessionGeneration(2);
        assert_eq!(
            ledger.grant(other, key.clone()),
            Err(StuntError::StaleSession {
                ledger: Some(session),
                given: other
            })
        );
        assert_eq!(ledger.len(), 1, "a refused grant changes nothing");
    }
}
