//! The declared stunt record: an authored traversal gate, the missions it is
//! eligible in, its direction/clearance rules, its reward and its scrapbook
//! media (F42-A).
//!
//! Spec: `specs/F42-stunts-fame-photos-and-optional-achievement-events.md`,
//! stage `### F42-A`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! This module is the **content half** of the stunt contract — the
//! normalized, provenance-carrying record a stunt importer produces and the
//! catalog consumes. Its runtime counterpart is `cs_sim::stunts` (the lowered
//! rule, the traversal predicate and the reward identity); the conversion
//! boundary is `cs_app::stunts`. The split mirrors `routes` ↔
//! `cs_sim::ai::navigation` and `campaign` ↔ `cs_sim::campaign`: this crate
//! cannot depend on `cs_sim`, so the declared record keeps its own typed
//! fields and re-validates them at its boundary.
//!
//! # The record
//!
//! A [`StuntDefinition`] binds:
//!
//! * a [`Gate`] — the authored traversal volume. It is an oriented aperture
//!   (a centre, a normal, two in-plane half extents and a half thickness),
//!   because "fly through the gate" is a statement about crossing a plane
//!   inside a hole, not about being near a position.
//! * a [`MissionScope`] — the missions that declare this stunt. There is
//!   deliberately **no** "every mission" variant: a world stunt is only as
//!   available as the missions that list it, so "do not award every world
//!   stunt in every mission automatically" (spec F42 behavior 3) is not
//!   expressible rather than merely forbidden.
//! * a [`TraversalRules`] pair of [`Resolved`] direction and clearance
//!   thresholds, a [`StuntCriticality`], a [`StuntRepeat`] policy and a
//!   [`StuntReward`].
//!
//! # Designed vocabulary, not original data
//!
//! The original 2000 PC game's stunt encoding is **not decoded**: nothing in
//! this repository measures which file stores stunts, how a gate is spelled,
//! what its extents or direction rule are, or how a stunt is bound to a
//! mission. Every id grammar, gate shape, rule kind, repeat policy and
//! fixture value here is therefore **newly authored project design** carrying
//! `Origin::SyntheticFixture` / [`GateEvidence::Reconstructed`] /
//! `Provenance::designed` provenance. Nothing in this module is a
//! measurement of the original game, and a record whose gate is
//! [`GateEvidence::Reconstructed`] can never support an original-fidelity
//! claim (spec F42 behavior 5).

use std::collections::HashSet;
use std::fmt;

use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};

/// Whether a declared gate's geometry may be used as evidence of original
/// behavior (spec F42 behavior 5).
///
/// The sheet requires that "manually drawn replacement volumes are marked
/// reconstructed until validated". That marking is a field of the record, not
/// a convention: a consumer can always tell a measured volume from a drawn
/// one, and an unmeasured volume from both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GateEvidence {
    /// The volume and its rules were measured from original data or
    /// observation. Nothing in this repository is in this state yet.
    Measured,
    /// A manually drawn replacement volume. Usable for the mission that
    /// declares it, never an original-behavior claim.
    Reconstructed,
    /// No geometry was recovered for this stunt, so the record cannot be
    /// lowered into a runtime rule at all.
    NotRecovered,
}

impl GateEvidence {
    /// Whether a gate in this state may be treated as measured original
    /// geometry.
    #[must_use]
    pub const fn is_measured(self) -> bool {
        matches!(self, Self::Measured)
    }
}

/// The authored traversal aperture of one stunt, in canonical meters.
///
/// The aperture is a plane (`normal`) with a rectangular hole (`right_half_
/// extent_m` × `up_half_extent_m`) and a thickness (`half_depth_m`). A zero
/// half depth is an infinitely thin gate, which is legal: the authored volume
/// may be a plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gate {
    /// The aperture's centre, in meters, in the world it belongs to.
    pub center_m: [f64; 3],
    /// The aperture's normal, in meters-of-authoring units. The direction a
    /// valid passage travels; it does not have to be unit length and is
    /// normalized by [`Gate::unit_normal`].
    pub normal: [f64; 3],
    /// Half of the hole's width, along the gate's derived `right` axis.
    pub right_half_extent_m: f64,
    /// Half of the hole's height, along the gate's derived `up` axis.
    pub up_half_extent_m: f64,
    /// Half of the aperture's thickness, along `normal`.
    pub half_depth_m: f64,
    /// Whether this volume is measured, drawn, or missing.
    pub evidence: GateEvidence,
}

impl Gate {
    /// The gate's normal, normalized, or [`None`] when it is not a usable
    /// direction (non-finite or zero length).
    #[must_use]
    pub fn unit_normal(&self) -> Option<[f64; 3]> {
        if !self.normal.iter().all(|value| value.is_finite()) {
            return None;
        }
        let length: f64 = self
            .normal
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt();
        if length <= 0.0 || !length.is_finite() {
            return None;
        }
        Some(self.normal.map(|value| value / length))
    }

    /// Whether the authored numbers are usable geometry.
    ///
    /// The refusing constructor is [`StuntDefinition::try_new`], which names
    /// the offending field; this predicate is the cheap form for callers that
    /// only need a yes/no.
    #[must_use]
    pub fn is_usable(&self) -> bool {
        self.center_m.iter().all(|value| value.is_finite())
            && self.unit_normal().is_some()
            && self.right_half_extent_m.is_finite()
            && self.right_half_extent_m > 0.0
            && self.up_half_extent_m.is_finite()
            && self.up_half_extent_m > 0.0
            && self.half_depth_m.is_finite()
            && self.half_depth_m >= 0.0
    }
}

/// The authored direction and clearance rules of one gate.
///
/// Both are [`Resolved`]: an original rule that was not measured stays an
/// explicit unknown with a claim id and a reason, and it refuses at the
/// lowering boundary instead of becoming a default that silently accepts or
/// rejects every passage.
#[derive(Clone, Debug, PartialEq)]
pub struct TraversalRules {
    /// The smallest cosine between the passage direction and the gate normal
    /// that still counts. `1.0` accepts only an exactly axial passage; `0.0`
    /// accepts any direction that is not backwards.
    pub min_forward_cosine: Resolved<f64>,
    /// The margin, in meters, the crossing point must keep from the nearest
    /// aperture rim. A pass inside the hole but inside this margin is
    /// `InsufficientClearance`, not a pass.
    pub min_clearance_m: Resolved<f64>,
}

/// Whether completing this stunt is allowed to matter to mission success
/// (spec F42 behavior 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StuntCriticality {
    /// Supplementary. It cannot force mission success or change the critical
    /// path, and modern achievement systems stay supplementary.
    Optional,
    /// The authored mission depends on it. This is a declaration, not an
    /// effect: no stage wires it into mission success on its own.
    CriticalPath,
}

impl StuntCriticality {
    /// Whether a completion of this stunt may affect mission success.
    #[must_use]
    pub const fn affects_mission_success(self) -> bool {
        matches!(self, Self::CriticalPath)
    }
}

/// Whether a second traversal of the same stunt pays again (spec F42
/// behavior 2).
///
/// Whether the original rules are one-time or repeatable is **unmeasured**;
/// this is a designed vocabulary so a record can state a policy and the
/// runtime can enforce exactly that policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StuntRepeat {
    /// One payout per reward identity, ever. A second traversal and a mission
    /// retry both refuse to pay again.
    Once,
    /// Every traversal pays.
    Repeatable,
}

/// What a completed traversal pays.
///
/// `media` is a `ContentKind::ScrapbookItem` id — the one-time photo the
/// sheet names. Its page, image and unlock state are F47's records; this
/// module only holds the identity the runtime dedups on.
#[derive(Clone, Debug, PartialEq)]
pub struct StuntReward {
    /// Fame points awarded for the completion.
    pub fame: Resolved<u32>,
    /// Cash in integer minor game units, the unit the outcome transaction
    /// moves (contract, "Outcome and economy transaction").
    pub cash_minor: Resolved<u64>,
    /// The scrapbook item this stunt unlocks, `Known(None)` when the record
    /// deliberately has no media.
    pub media: Resolved<Option<ContentId>>,
}

impl StuntReward {
    /// A reward that pays nothing at all. Legal, and different from an
    /// unmeasured reward: `Unknown` is an error at the lowering boundary.
    #[must_use]
    pub fn nothing() -> Self {
        let provenance = Provenance::designed(
            cs_types::evidence::ClaimId::new("f42a.reward-nothing")
                .expect("the fixture claim id is valid"),
        );
        Self {
            fame: Resolved::Known(cs_types::content::Known::new(0, provenance.clone())),
            cash_minor: Resolved::Known(cs_types::content::Known::new(0, provenance)),
            media: Resolved::Known(cs_types::content::Known::new(
                None,
                Provenance::designed(
                    cs_types::evidence::ClaimId::new("f42a.reward-nothing-media")
                        .expect("the fixture claim id is valid"),
                ),
            )),
        }
    }
}

/// The missions that declare one stunt eligible.
///
/// The scope is a list, never a flag: a world stunt's availability is
/// mission-scoped, so two missions that share one world can declare different
/// sets (spec F42 behavior 3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionScope {
    missions: Vec<ContentId>,
}

impl MissionScope {
    /// Validates a mission list: non-empty, no duplicates, every entry a
    /// `ContentKind::Mission` id.
    ///
    /// # Errors
    ///
    /// [`StuntError::EmptyMissionScope`],
    /// [`StuntError::NotAMission`] or [`StuntError::DuplicateMissionScope`].
    pub fn try_new(missions: Vec<ContentId>) -> Result<Self, StuntError> {
        if missions.is_empty() {
            return Err(StuntError::EmptyMissionScope);
        }
        let mut seen: HashSet<&str> = HashSet::new();
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
        Ok(Self { missions })
    }

    /// The declared missions, in authored order.
    #[must_use]
    pub fn missions(&self) -> &[ContentId] {
        &self.missions
    }

    /// How many missions declare this stunt.
    #[must_use]
    pub fn len(&self) -> usize {
        self.missions.len()
    }

    /// Always `false`: an empty scope cannot be constructed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Whether `mission` declares this stunt.
    #[must_use]
    pub fn contains(&self, mission: &ContentId) -> bool {
        self.missions.iter().any(|declared| declared == mission)
    }
}

/// One declared stunt: the normalized record the catalog stores.
#[derive(Clone, Debug, PartialEq)]
pub struct StuntDefinition {
    id: ContentId,
    origin: Origin,
    world: ContentId,
    gate: Resolved<Gate>,
    rules: TraversalRules,
    scope: MissionScope,
    criticality: StuntCriticality,
    repeat: StuntRepeat,
    reward: StuntReward,
    provenance: Provenance,
}

/// The raw parts of a [`StuntDefinition`], collected so the validating
/// constructor takes one record instead of a long positional argument list.
#[derive(Clone, Debug, PartialEq)]
pub struct StuntDraft {
    /// The `stunt` content id.
    pub id: ContentId,
    /// Where the record came from.
    pub origin: Origin,
    /// The `world` the gate lives in.
    pub world: ContentId,
    /// The authored gate, or an explicit unknown.
    pub gate: Resolved<Gate>,
    /// The authored direction and clearance rules.
    pub rules: TraversalRules,
    /// The missions that declare the stunt.
    pub scope: MissionScope,
    /// Whether the stunt can affect mission success.
    pub criticality: StuntCriticality,
    /// Whether a second traversal pays again.
    pub repeat: StuntRepeat,
    /// What a completion pays.
    pub reward: StuntReward,
    /// The provenance of the record itself.
    pub provenance: Provenance,
}

impl StuntDefinition {
    /// Validates and assembles a declared stunt.
    ///
    /// # Errors
    ///
    /// [`StuntError`] for every rule the record must satisfy. An explicit
    /// unknown is *not* an error here — it is content a later stage must
    /// block on — but a **known** value of the wrong kind, sign or finiteness
    /// is an authoring error.
    pub fn try_new(draft: StuntDraft) -> Result<Self, StuntError> {
        let StuntDraft {
            id,
            origin,
            world,
            gate,
            rules,
            scope,
            criticality,
            repeat,
            reward,
            provenance,
        } = draft;
        if id.kind() != ContentKind::Stunt {
            return Err(StuntError::NotAStunt { kind: id.kind() });
        }
        if world.kind() != ContentKind::World {
            return Err(StuntError::NotAWorld { kind: world.kind() });
        }
        if let Resolved::Known(known) = &gate
            && let Some(error) = gate_error(&known.value)
        {
            return Err(error);
        }
        if let Resolved::Known(known) = &rules.min_forward_cosine
            && !(known.value.is_finite() && (-1.0..=1.0).contains(&known.value))
        {
            return Err(StuntError::ForwardCosineOutOfRange { value: known.value });
        }
        if let Resolved::Known(known) = &rules.min_clearance_m
            && !(known.value.is_finite() && known.value >= 0.0)
        {
            return Err(StuntError::NegativeClearance { value: known.value });
        }
        if let Resolved::Known(known) = &reward.media
            && let Some(media) = &known.value
            && media.kind() != ContentKind::ScrapbookItem
        {
            return Err(StuntError::MediaNotScrapbookItem { kind: media.kind() });
        }
        Ok(Self {
            id,
            origin,
            world,
            gate,
            rules,
            scope,
            criticality,
            repeat,
            reward,
            provenance,
        })
    }

    /// The `stunt` content id.
    #[must_use]
    pub fn id(&self) -> &ContentId {
        &self.id
    }

    /// Where this record's bytes came from.
    #[must_use]
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The `world` the gate lives in.
    #[must_use]
    pub fn world(&self) -> &ContentId {
        &self.world
    }

    /// The authored gate, or an explicit unknown.
    #[must_use]
    pub fn gate(&self) -> &Resolved<Gate> {
        &self.gate
    }

    /// The authored direction and clearance rules.
    #[must_use]
    pub fn rules(&self) -> &TraversalRules {
        &self.rules
    }

    /// The missions that declare this stunt.
    #[must_use]
    pub fn scope(&self) -> &MissionScope {
        &self.scope
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
    pub fn reward(&self) -> &StuntReward {
        &self.reward
    }

    /// The provenance of the record itself.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// The evidence state of the gate this record carries: the gate's own
    /// marking, or [`GateEvidence::NotRecovered`] when no geometry was
    /// recovered at all.
    #[must_use]
    pub fn gate_evidence(&self) -> GateEvidence {
        match &self.gate {
            Resolved::Known(known) => known.value.evidence,
            Resolved::Unknown { .. } => GateEvidence::NotRecovered,
        }
    }
}

/// The first gate-authoring problem, named.
fn gate_error(gate: &Gate) -> Option<StuntError> {
    const FIELDS: [&str; 3] = ["gate.center_m", "gate.normal", "gate.extents"];
    if !gate.center_m.iter().all(|value| value.is_finite()) {
        return Some(StuntError::NonFiniteGate { field: FIELDS[0] });
    }
    if !gate.normal.iter().all(|value| value.is_finite()) {
        return Some(StuntError::NonFiniteGate { field: FIELDS[1] });
    }
    if gate.unit_normal().is_none() {
        return Some(StuntError::ZeroGateNormal);
    }
    if !gate.right_half_extent_m.is_finite()
        || gate.right_half_extent_m <= 0.0
        || !gate.up_half_extent_m.is_finite()
        || gate.up_half_extent_m <= 0.0
    {
        return Some(StuntError::NonPositiveGateExtent { field: FIELDS[2] });
    }
    if !gate.half_depth_m.is_finite() || gate.half_depth_m < 0.0 {
        return Some(StuntError::NegativeGateDepth {
            value: gate.half_depth_m,
        });
    }
    None
}

/// Why a declared stunt was refused.
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
    /// A mission scope entry is not a mission.
    NotAMission {
        /// The kind it actually names.
        kind: ContentKind,
    },
    /// A known gate field was not finite.
    NonFiniteGate {
        /// Which field is corrupt.
        field: &'static str,
    },
    /// A known gate normal had no usable direction.
    ZeroGateNormal,
    /// A known gate extent was not strictly positive and finite.
    NonPositiveGateExtent {
        /// Which extent group is corrupt.
        field: &'static str,
    },
    /// A known gate half depth was not finite and non-negative.
    NegativeGateDepth {
        /// The offending value.
        value: f64,
    },
    /// A known direction rule was outside `[-1, 1]` or not finite.
    ForwardCosineOutOfRange {
        /// The offending value.
        value: f64,
    },
    /// A known clearance rule was negative or not finite.
    NegativeClearance {
        /// The offending value.
        value: f64,
    },
    /// The reward's media id is not a scrapbook item.
    MediaNotScrapbookItem {
        /// The kind it actually names.
        kind: ContentKind,
    },
    /// A stunt declared no eligible mission at all.
    EmptyMissionScope,
    /// A mission appears twice in one scope.
    DuplicateMissionScope {
        /// The duplicated mission key.
        mission: String,
    },
}

impl fmt::Display for StuntError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAStunt { kind } => write!(f, "stunt id names a {kind}, not a stunt"),
            Self::NotAWorld { kind } => write!(f, "stunt world names a {kind}, not a world"),
            Self::NotAMission { kind } => write!(f, "mission scope names a {kind}, not a mission"),
            Self::NonFiniteGate { field } => write!(f, "{field} is not finite"),
            Self::ZeroGateNormal => {
                write!(f, "gate.normal has no usable direction (it is zero length)")
            }
            Self::NonPositiveGateExtent { field } => {
                write!(f, "{field} must be strictly positive and finite")
            }
            Self::NegativeGateDepth { value } => {
                write!(f, "gate half depth {value} is not finite and non-negative")
            }
            Self::ForwardCosineOutOfRange { value } => {
                write!(f, "min_forward_cosine {value} is outside [-1, 1]")
            }
            Self::NegativeClearance { value } => {
                write!(f, "min_clearance_m {value} is not finite and non-negative")
            }
            Self::MediaNotScrapbookItem { kind } => {
                write!(f, "reward media names a {kind}, not a scrapbook item")
            }
            Self::EmptyMissionScope => {
                write!(f, "a stunt must declare at least one eligible mission")
            }
            Self::DuplicateMissionScope { mission } => {
                write!(f, "mission {mission:?} appears more than once in one scope")
            }
        }
    }
}

impl std::error::Error for StuntError {}

// -------------------------------------------------------------- fixture ----

/// The designed aperture of [`declared_synthetic_gate_stunt`]: a 24 m wide,
/// 16 m tall, 8 m deep gate on the world origin, facing the canonical
/// forward axis. Every number is a designed fixture value, never a
/// measurement.
pub const SYNTHETIC_GATE_CENTER_M: [f64; 3] = [0.0, 0.0, 0.0];
/// The designed normal of the synthetic gate: canonical forward, -Z.
pub const SYNTHETIC_GATE_NORMAL: [f64; 3] = [0.0, 0.0, -1.0];
/// The designed hole width of the synthetic gate, in meters.
pub const SYNTHETIC_GATE_HALF_WIDTH_M: f64 = 12.0;
/// The designed hole height of the synthetic gate, in meters.
pub const SYNTHETIC_GATE_HALF_HEIGHT_M: f64 = 8.0;
/// The designed thickness of the synthetic gate, in meters.
pub const SYNTHETIC_GATE_HALF_DEPTH_M: f64 = 4.0;
/// The designed direction rule of the synthetic gate: within 60° of the
/// normal.
pub const SYNTHETIC_GATE_MIN_FORWARD_COSINE: f64 = 0.5;
/// The designed rim margin of the synthetic gate, in meters.
pub const SYNTHETIC_GATE_MIN_CLEARANCE_M: f64 = 2.0;
/// The designed fame payout of the synthetic gate.
pub const SYNTHETIC_GATE_FAME: u32 = 25;
/// The designed cash payout of the synthetic gate, in minor units.
pub const SYNTHETIC_GATE_CASH_MINOR: u64 = 500;

/// The minimal declared synthetic fixture: one fly-by gate in a synthetic
/// world, declared eligible in exactly one synthetic mission.
///
/// It is the F42-A version of the declared-only fixtures in
/// [`cs_content::routes`] and [`cs_content::campaign`]: a real record that
/// the validating constructor accepts, that the lowering boundary turns into
/// a runtime rule, and that the runtime predicate can complete. Its origin is
/// [`Origin::SyntheticFixture`], its gate is
/// [`GateEvidence::Reconstructed`] and every value carries designed
/// provenance, so it can never be mistaken for — or stand in for — original
/// content.
#[must_use]
pub fn declared_synthetic_gate_stunt() -> StuntDefinition {
    let designed = |claim: &str| {
        Provenance::designed(
            cs_types::evidence::ClaimId::new(claim).expect("the fixture claim id is valid"),
        )
    };
    StuntDefinition::try_new(StuntDraft {
        id: ContentId::from_source(ContentKind::Stunt, "synthetic.flyby-gate")
            .expect("fixture stunt id is valid"),
        origin: Origin::SyntheticFixture,
        world: ContentId::from_source(ContentKind::World, "synthetic.harbor")
            .expect("fixture world id is valid"),
        gate: Resolved::Known(cs_types::content::Known::new(
            Gate {
                center_m: SYNTHETIC_GATE_CENTER_M,
                normal: SYNTHETIC_GATE_NORMAL,
                right_half_extent_m: SYNTHETIC_GATE_HALF_WIDTH_M,
                up_half_extent_m: SYNTHETIC_GATE_HALF_HEIGHT_M,
                half_depth_m: SYNTHETIC_GATE_HALF_DEPTH_M,
                evidence: GateEvidence::Reconstructed,
            },
            designed("f42a.synthetic-gate"),
        )),
        rules: TraversalRules {
            min_forward_cosine: Resolved::Known(cs_types::content::Known::new(
                SYNTHETIC_GATE_MIN_FORWARD_COSINE,
                designed("f42a.synthetic-gate-direction"),
            )),
            min_clearance_m: Resolved::Known(cs_types::content::Known::new(
                SYNTHETIC_GATE_MIN_CLEARANCE_M,
                designed("f42a.synthetic-gate-clearance"),
            )),
        },
        scope: MissionScope::try_new(vec![
            ContentId::from_source(ContentKind::Mission, "synthetic.m01")
                .expect("fixture mission id is valid"),
        ])
        .expect("the fixture scope is valid"),
        criticality: StuntCriticality::Optional,
        repeat: StuntRepeat::Once,
        reward: StuntReward {
            fame: Resolved::Known(cs_types::content::Known::new(
                SYNTHETIC_GATE_FAME,
                designed("f42a.synthetic-gate-fame"),
            )),
            cash_minor: Resolved::Known(cs_types::content::Known::new(
                SYNTHETIC_GATE_CASH_MINOR,
                designed("f42a.synthetic-gate-cash"),
            )),
            media: Resolved::Known(cs_types::content::Known::new(
                Some(
                    ContentId::from_source(ContentKind::ScrapbookItem, "synthetic.flyby-photo")
                        .expect("fixture media id is valid"),
                ),
                designed("f42a.synthetic-gate-media"),
            )),
        },
        provenance: designed("f42a.synthetic-gate-stunt"),
    })
    .expect("the declared synthetic gate stunt is valid")
}

/// The synthetic mission the declared gate fixture is eligible in.
#[must_use]
pub fn synthetic_gate_mission() -> ContentId {
    ContentId::from_source(ContentKind::Mission, "synthetic.m01")
        .expect("fixture mission id is valid")
}

/// The synthetic world the declared gate fixture lives in.
#[must_use]
pub fn synthetic_gate_world() -> ContentId {
    ContentId::from_source(ContentKind::World, "synthetic.harbor")
        .expect("fixture world id is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_types::content::Known;
    use cs_types::evidence::ClaimId;

    fn claim(id: &str) -> ClaimId {
        ClaimId::new(id).expect("valid claim id")
    }

    fn designed() -> Provenance {
        Provenance::designed(claim("f42a.unit"))
    }

    fn draft(stunt: StuntDefinition) -> StuntDraft {
        StuntDraft {
            id: stunt.id,
            origin: stunt.origin,
            world: stunt.world,
            gate: stunt.gate,
            rules: stunt.rules,
            scope: stunt.scope,
            criticality: stunt.criticality,
            repeat: stunt.repeat,
            reward: stunt.reward,
            provenance: stunt.provenance,
        }
    }

    fn fixture() -> StuntDraft {
        draft(declared_synthetic_gate_stunt())
    }

    /// The declared fixture is a synthetic, single-mission, one-time
    /// fly-by gate whose geometry is marked reconstructed — so it can never
    /// be read as measured original content.
    #[test]
    fn accept_f42_a_declared_synthetic_gate_is_synthetic_reconstructed_and_once() {
        let record = declared_synthetic_gate_stunt();
        assert_eq!(record.id().as_str(), "stunt/synthetic.flyby-gate");
        assert_eq!(record.id().kind(), ContentKind::Stunt);
        assert_eq!(record.world(), &synthetic_gate_world());
        assert_eq!(record.origin(), &Origin::SyntheticFixture);
        assert!(!record.origin().is_original());
        assert_eq!(record.gate_evidence(), GateEvidence::Reconstructed);
        assert!(!record.gate_evidence().is_measured());
        assert_eq!(record.repeat(), StuntRepeat::Once);
        assert_eq!(record.criticality(), StuntCriticality::Optional);
        assert!(!record.criticality().affects_mission_success());
        assert_eq!(record.scope().len(), 1);
        assert!(record.scope().contains(&synthetic_gate_mission()));
        assert!(!record.scope().is_empty());

        let gate = record
            .gate()
            .clone()
            .known()
            .expect("the fixture gate is known");
        assert_eq!(gate.unit_normal(), Some([0.0, 0.0, -1.0]));
        assert!(gate.is_usable());
        assert_eq!(
            record.rules().min_forward_cosine.clone().known(),
            Some(SYNTHETIC_GATE_MIN_FORWARD_COSINE)
        );
        assert_eq!(
            record.rules().min_clearance_m.clone().known(),
            Some(SYNTHETIC_GATE_MIN_CLEARANCE_M)
        );
        assert_eq!(
            record.reward().fame.clone().known(),
            Some(SYNTHETIC_GATE_FAME)
        );
        let media = record
            .reward()
            .media
            .clone()
            .known()
            .expect("the fixture media is resolved")
            .expect("the fixture stunt has media");
        assert_eq!(media.as_str(), "scrapbook_item/synthetic.flyby-photo");
    }

    /// Every authoring mistake the constructor must name: a wrong namespace,
    /// corrupt gate geometry, an out-of-range direction rule, a negative
    /// clearance, media that is not a scrapbook item and an unusable mission
    /// scope.
    #[test]
    fn accept_f42_a_declared_stunt_names_every_authoring_mistake() {
        let base = fixture();

        let mut wrong_namespace = base.clone();
        wrong_namespace.id =
            ContentId::from_source(ContentKind::Route, "synthetic.wrong").expect("valid id");
        assert_eq!(
            StuntDefinition::try_new(wrong_namespace),
            Err(StuntError::NotAStunt {
                kind: ContentKind::Route
            })
        );

        let mut wrong_world = base.clone();
        wrong_world.world =
            ContentId::from_source(ContentKind::Mission, "synthetic.wrong").expect("valid id");
        assert_eq!(
            StuntDefinition::try_new(wrong_world),
            Err(StuntError::NotAWorld {
                kind: ContentKind::Mission
            })
        );

        let zero_normal = |normal| Gate {
            normal,
            ..synthetic_gate()
        };
        for (normal, expected) in [
            ([0.0, 0.0, 0.0], StuntError::ZeroGateNormal),
            (
                [f64::NAN, 0.0, -1.0],
                StuntError::NonFiniteGate {
                    field: "gate.normal",
                },
            ),
        ] {
            let mut broken = base.clone();
            broken.gate = Resolved::Known(Known::new(zero_normal(normal), designed()));
            assert_eq!(StuntDefinition::try_new(broken), Err(expected));
        }

        let mut non_finite_center = base.clone();
        non_finite_center.gate = Resolved::Known(Known::new(
            Gate {
                center_m: [f64::INFINITY, 0.0, 0.0],
                ..synthetic_gate()
            },
            designed(),
        ));
        assert_eq!(
            StuntDefinition::try_new(non_finite_center),
            Err(StuntError::NonFiniteGate {
                field: "gate.center_m"
            })
        );

        let mut zero_width = base.clone();
        zero_width.gate = Resolved::Known(Known::new(
            Gate {
                right_half_extent_m: 0.0,
                ..synthetic_gate()
            },
            designed(),
        ));
        assert_eq!(
            StuntDefinition::try_new(zero_width),
            Err(StuntError::NonPositiveGateExtent {
                field: "gate.extents"
            })
        );

        let mut negative_depth = base.clone();
        negative_depth.gate = Resolved::Known(Known::new(
            Gate {
                half_depth_m: -1.0,
                ..synthetic_gate()
            },
            designed(),
        ));
        assert_eq!(
            StuntDefinition::try_new(negative_depth),
            Err(StuntError::NegativeGateDepth { value: -1.0 })
        );

        let mut wide_rule = base.clone();
        wide_rule.rules.min_forward_cosine = Resolved::Known(Known::new(1.5, designed()));
        assert_eq!(
            StuntDefinition::try_new(wide_rule),
            Err(StuntError::ForwardCosineOutOfRange { value: 1.5 })
        );

        let mut negative_clearance = base.clone();
        negative_clearance.rules.min_clearance_m = Resolved::Known(Known::new(-0.5, designed()));
        assert_eq!(
            StuntDefinition::try_new(negative_clearance),
            Err(StuntError::NegativeClearance { value: -0.5 })
        );

        let mut wrong_media = base.clone();
        wrong_media.reward.media = Resolved::Known(Known::new(
            Some(
                ContentId::from_source(ContentKind::Image, "synthetic.not-a-photo")
                    .expect("valid id"),
            ),
            designed(),
        ));
        assert_eq!(
            StuntDefinition::try_new(wrong_media),
            Err(StuntError::MediaNotScrapbookItem {
                kind: ContentKind::Image
            })
        );

        assert_eq!(
            MissionScope::try_new(Vec::new()),
            Err(StuntError::EmptyMissionScope)
        );
        assert_eq!(
            MissionScope::try_new(vec![
                ContentId::from_source(ContentKind::Route, "synthetic.not-a-mission")
                    .expect("valid id")
            ]),
            Err(StuntError::NotAMission {
                kind: ContentKind::Route
            })
        );
        assert_eq!(
            MissionScope::try_new(vec![synthetic_gate_mission(), synthetic_gate_mission()]),
            Err(StuntError::DuplicateMissionScope {
                mission: "mission/synthetic.m01".to_owned()
            })
        );
    }

    /// An explicit unknown is content, not an authoring error: a record with
    /// no recovered gate and no measured rules still validates, reports
    /// `NotRecovered`, and only refuses where a consumer needs a value.
    #[test]
    fn accept_f42_a_declared_stunt_carries_unknowns_instead_of_guessing() {
        let mut record = fixture();
        record.gate = Resolved::unknown(
            claim("f42a.unknown-gate"),
            "no stunt trigger volume was recovered from the mission",
        )
        .expect("a reason is present");
        record.rules.min_forward_cosine = Resolved::unknown(
            claim("f42a.unknown-direction"),
            "the original direction threshold was not measured",
        )
        .expect("a reason is present");
        let stunt = StuntDefinition::try_new(record).expect("unknowns are content");
        assert_eq!(stunt.gate_evidence(), GateEvidence::NotRecovered);
        assert!(!stunt.gate().is_known());
        assert!(!stunt.rules().min_forward_cosine.is_known());
    }

    fn synthetic_gate() -> Gate {
        Gate {
            center_m: SYNTHETIC_GATE_CENTER_M,
            normal: SYNTHETIC_GATE_NORMAL,
            right_half_extent_m: SYNTHETIC_GATE_HALF_WIDTH_M,
            up_half_extent_m: SYNTHETIC_GATE_HALF_HEIGHT_M,
            half_depth_m: SYNTHETIC_GATE_HALF_DEPTH_M,
            evidence: GateEvidence::Reconstructed,
        }
    }
}
