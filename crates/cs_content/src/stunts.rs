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

use crate::world::{RetailTriggerVolume, WorldId};

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
    /// Half of the aperture's thickness, along `normal`. A zero half depth is
    /// a legal infinitely thin plane gate; the thickness records how thick the
    /// gate was drawn and does not change whether a traversal counts.
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
    ///
    /// A margin wider than half the hole's smaller extent can never be met, so
    /// the runtime reports it as an unusable rule instead of refusing every
    /// traversal as a near miss.
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
        // A margin wider than the hole can never be met, so a known gate and a
        // known margin are checked together: the rule would otherwise refuse
        // every traversal of this stunt as a near miss.
        if let (Resolved::Known(gate), Resolved::Known(clearance)) = (&gate, &rules.min_clearance_m)
        {
            let max_possible_m = gate
                .value
                .right_half_extent_m
                .min(gate.value.up_half_extent_m);
            if clearance.value > max_possible_m {
                return Err(StuntError::UnsatisfiableClearance {
                    required_m: clearance.value,
                    max_possible_m,
                });
            }
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
    /// A known clearance rule is wider than the gate's own hole, so no
    /// crossing of this gate could ever satisfy it.
    UnsatisfiableClearance {
        /// The authored minimum margin, in meters.
        required_m: f64,
        /// The widest margin the authored aperture can offer, in meters.
        max_possible_m: f64,
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
            Self::UnsatisfiableClearance {
                required_m,
                max_possible_m,
            } => write!(
                f,
                "min_clearance_m {required_m} exceeds the {max_possible_m} this gate can offer"
            ),
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

// ------------------------------------------------- the measured encoding ----
//
// F42-D has to audit the original's mission-scoped stunts, and task #463
// measured where they are spelled. The original does not author a stunt gate's
// volume in a mission: it names a **detection zone** of the world it is set in,
// and the zone's box lives in the world container (task #427 measured those
// boxes). What this section adds is the other half — the scenario-side encoding
// that says which zone a fly-through objective is:
//
//   * the instant action scenario carries one reader-archive member `ia.zrd`
//     (an F49 scenario descriptor) whose `mission_type` names the scenario
//     mode, including `stunt_flying`; its `dzones` list binds a scenario-local
//     label (`dz1`, `sghangar`, …) to the world node it stands for
//     (`dzpath1`, …);
//   * the same scenario's `targets.zrd` declares one target per objective, and
//     a fly-through danger-zone target carries `category_label = MSG_OBJ_DZ`
//     and `help_label = MSG_OBJ_FLYTHROUGH` with `nodes = [<label>]`.
//
// Both members are `.zrd` documents: a small typed tree (`u32` tag then a
// payload) whose grammar F09 measured against every reader-archive member of
// `ZBD/zrdr.zbd` — tag `1` int, `2` float, `3` text (`u32` length + bytes), `4`
// list (`u32` count followed by **`count - 1`** children). The decoder below is
// the same grammar; the F42 content half cannot reach F09's private reader, so
// it spells the measured grammar out here and cites it.
//
// What is **not** measured, and is therefore not a field of any record here:
// a stunt's direction rule, its clearance rule, whether a second pass pays
// again, what it pays, and any linked scrapbook media. The scenario bytes
// carry none of those, so a consumer that needs them must treat this encoding
// as one input, never as the whole stunt.

/// The reader-archive member an instant-action scenario's mode and zone
/// binding live in (task #463, measured).
pub const SCENARIO_MEMBER: &str = "ia.zrd";

/// The reader-archive member an instant-action scenario's objective targets
/// live in (task #463, measured).
pub const SCENARIO_TARGETS_MEMBER: &str = "targets.zrd";

/// The scenario `mission_type` that names the original's fly-through stunt
/// mode (task #463, measured: four of the eight instant-action scenarios
/// declare it).
pub const STUNT_MISSION_TYPE: &str = "stunt_flying";

/// The `category_label` every measured fly-through danger-zone target carries.
pub const FLY_THROUGH_CATEGORY_LABEL: &str = "MSG_OBJ_DZ";

/// The `help_label` every measured fly-through danger-zone target carries.
pub const FLY_THROUGH_HELP_LABEL: &str = "MSG_OBJ_FLYTHROUGH";

/// The `ia.zrd` field that names the scenario mode.
pub const SCENARIO_MISSION_TYPE_KEY: &str = "mission_type";

/// The `ia.zrd` field that binds scenario zone labels to world node names.
pub const SCENARIO_ZONES_KEY: &str = "dzones";

/// The `targets.zrd` field naming the objective category, the field a
/// fly-through target is recognized by.
pub const TARGET_CATEGORY_KEY: &str = "category_label";

/// The `targets.zrd` field naming the objective help text.
pub const TARGET_HELP_KEY: &str = "help_label";

/// The `targets.zrd` field listing the nodes an objective is about.
pub const TARGET_NODES_KEY: &str = "nodes";

/// The `targets.zrd` field naming the objective description.
pub const TARGET_DESCRIPTION_KEY: &str = "description";

/// `.zrd` node tags: `1` int, `2` float, `3` text, `4` list (F09's measured
/// grammar, `docs/findings/2026-10-02-f09-palette-original-faction-palettes.md`).
pub const ZRD_TAG_INT: u32 = 1;
/// See [`ZRD_TAG_INT`].
pub const ZRD_TAG_FLOAT: u32 = 2;
/// See [`ZRD_TAG_INT`].
pub const ZRD_TAG_TEXT: u32 = 3;
/// See [`ZRD_TAG_INT`].
pub const ZRD_TAG_LIST: u32 = 4;

/// The smallest encoded `.zrd` node: a `u32` tag plus a `u32` payload word.
const MIN_ZRD_NODE_BYTES: usize = 8;

/// The deepest a `.zrd` document may nest before it is refused, so a hostile
/// member cannot exhaust the stack.
const MAX_ZRD_DEPTH: u32 = 64;

/// One decoded `.zrd` value.
///
/// The tree is deliberately value-only: the survey's provenance is the
/// **member's** byte span (from production discovery), which is the whole of
/// what a `.zrd` node needs to be traced back to the bytes.
#[derive(Clone, Debug, PartialEq)]
pub enum ZrdValue {
    /// Tag `1`: a `u32`.
    Int(u32),
    /// Tag `2`: an `f32`. Recognized so a document that holds one parses.
    Float(f32),
    /// Tag `3`: a `u32` length followed by that many UTF-8 bytes.
    Text(String),
    /// Tag `4`: a `u32` count followed by `count - 1` children.
    List(Vec<ZrdValue>),
}

impl ZrdValue {
    /// The list's children, or `None` when this is not a list.
    #[must_use]
    pub fn as_list(&self) -> Option<&[ZrdValue]> {
        match self {
            Self::List(children) => Some(children),
            _ => None,
        }
    }

    /// The text value, or `None` when this is not text.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    /// The integer value, or `None` when this is not an int node.
    #[must_use]
    pub const fn as_int(&self) -> Option<u32> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }
}

/// Why a `.zrd` member could not be decoded.
///
/// Named by a stable `code` and the byte `offset` it was found at, never a
/// symptom of a guessed layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZrdDecodeError {
    code: &'static str,
    offset: u64,
}

impl ZrdDecodeError {
    /// The stable refusal code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// Offset of the refusal, relative to the member.
    #[must_use]
    pub const fn offset(&self) -> u64 {
        self.offset
    }
}

impl fmt::Display for ZrdDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the .zrd member is not decodable at {} ({})",
            self.offset, self.code
        )
    }
}

impl std::error::Error for ZrdDecodeError {}

/// A `.zrd` reader over one member's bytes.
struct ZrdReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> ZrdReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn read_u32(&mut self) -> Result<u32, ZrdDecodeError> {
        let offset = self.position as u64;
        let end = self.position.checked_add(4).ok_or(ZrdDecodeError {
            code: "truncated",
            offset,
        })?;
        let word = self.bytes.get(self.position..end).ok_or(ZrdDecodeError {
            code: "truncated",
            offset,
        })?;
        self.position = end;
        Ok(u32::from_le_bytes(word.try_into().expect("four bytes")))
    }

    fn read_body(&mut self, length: usize) -> Result<&'a [u8], ZrdDecodeError> {
        let offset = self.position as u64;
        let end = self.position.checked_add(length).ok_or(ZrdDecodeError {
            code: "truncated",
            offset,
        })?;
        let body = self.bytes.get(self.position..end).ok_or(ZrdDecodeError {
            code: "truncated",
            offset,
        })?;
        self.position = end;
        Ok(body)
    }

    fn parse_node(&mut self, depth: u32) -> Result<ZrdValue, ZrdDecodeError> {
        let start = self.position as u64;
        if depth > MAX_ZRD_DEPTH {
            return Err(ZrdDecodeError {
                code: "depth_exceeded",
                offset: start,
            });
        }
        let tag = self.read_u32()?;
        let value = match tag {
            ZRD_TAG_INT => ZrdValue::Int(self.read_u32()?),
            ZRD_TAG_FLOAT => {
                let word = self.read_u32()?;
                ZrdValue::Float(f32::from_bits(word))
            }
            ZRD_TAG_TEXT => {
                let length = self.read_u32()? as usize;
                let body = self.read_body(length)?;
                let text = std::str::from_utf8(body).map_err(|_| ZrdDecodeError {
                    code: "invalid_text",
                    offset: start,
                })?;
                ZrdValue::Text(text.to_owned())
            }
            ZRD_TAG_LIST => {
                // A list of `N` holds `N - 1` children (measured by F09, not
                // guessed): the profiler stores one more than the child count.
                let count = self.read_u32()?;
                let children = count.saturating_sub(1) as usize;
                // A child costs at least eight bytes, so a count larger than the
                // remaining bytes cannot be honest; refusing here also bounds
                // the allocation below.
                if children > self.remaining() / MIN_ZRD_NODE_BYTES + 1 {
                    return Err(ZrdDecodeError {
                        code: "count_exceeds_bytes",
                        offset: start,
                    });
                }
                let mut kids = Vec::with_capacity(children);
                for _ in 0..children {
                    kids.push(self.parse_node(depth + 1)?);
                }
                ZrdValue::List(kids)
            }
            _ => {
                return Err(ZrdDecodeError {
                    code: "unknown_tag",
                    offset: start,
                });
            }
        };
        Ok(value)
    }
}

/// Decodes one `.zrd` member, refusing a tail no node accounts for.
///
/// # Errors
///
/// [`ZrdDecodeError`] with a stable code and the byte offset: `truncated` for a
/// word or body that does not fit, `invalid_text` for a text body that is not
/// UTF-8, `count_exceeds_bytes` for a list longer than the member could hold,
/// `depth_exceeded` past [`MAX_ZRD_DEPTH`], `unknown_tag` for a tag outside the
/// four measured kinds, and `trailing_bytes` when the nodes stop short of the
/// member's end.
pub fn decode_zrd(bytes: &[u8]) -> Result<ZrdValue, ZrdDecodeError> {
    let mut reader = ZrdReader::new(bytes);
    let root = reader.parse_node(0)?;
    if reader.remaining() != 0 {
        return Err(ZrdDecodeError {
            code: "trailing_bytes",
            offset: reader.position as u64,
        });
    }
    Ok(root)
}

/// The value of a key in a `.zrd` record.
///
/// The original writes a record in **two** measured shapes, and this reads
/// both:
///
/// * a flat alternating sequence of text keys and their values, the shape
///   `ia.zrd`'s root uses (`["mission_type", [...], "dzones", [...], …]`);
/// * a list of two-element `[key, value]` pairs, the shape every `targets.zrd`
///   objective uses (`[["description", "…"], ["nodes", ["dz1"]], …]`).
///
/// The flat shape is tried first and the pair shape second, so a record that
/// matches neither answers `None` rather than a value attributed to the wrong
/// key. A key that is not text is skipped, never silently treated as one.
#[must_use]
pub fn zrd_field<'a>(node: &'a ZrdValue, key: &str) -> Option<&'a ZrdValue> {
    let children = node.as_list()?;
    let mut index = 0;
    while index < children.len() {
        if let Some(name) = children[index].as_text() {
            let value = children.get(index + 1);
            if name == key {
                return value;
            }
            index += 2;
        } else {
            index += 1;
        }
    }
    for child in children {
        let Some(pair) = child.as_list() else {
            continue;
        };
        if pair.first().and_then(ZrdValue::as_text) == Some(key) {
            return pair.get(1);
        }
    }
    None
}

/// The `mission_type` an instant-action scenario declares.
///
/// Measured values include `stunt_flying`, `dogfight_squadron` and
/// `zeppelin_run`; the field names the scenario's mode, not a stunt's. The
/// original wraps the mode in a one-element list (`["stunt_flying"]`); a
/// bare text value is accepted too, so the reader does not depend on the
/// wrapper being present.
#[must_use]
pub fn scenario_mission_type(scenario: &ZrdValue) -> Option<&str> {
    let value = zrd_field(scenario, SCENARIO_MISSION_TYPE_KEY)?;
    if let Some(text) = value.as_text() {
        return Some(text);
    }
    let list = value.as_list()?;
    if list.len() == 1 {
        list[0].as_text()
    } else {
        None
    }
}

/// The scenario's detection-zone bindings, as `(world_node, label)` pairs.
///
/// Measured: `dzones` is a list of two-element lists, each
/// `[<world node>, <scenario label>]`, e.g. `["dzpath1", "dz1"]`. The world
/// node name is the `dzpath<N>` node task #427 measured a box for.
#[must_use]
pub fn scenario_zone_bindings(scenario: &ZrdValue) -> Vec<(&str, &str)> {
    let Some(zones) = zrd_field(scenario, SCENARIO_ZONES_KEY) else {
        return Vec::new();
    };
    let Some(entries) = zones.as_list() else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let pair = entry.as_list()?;
            let world_node = pair.first()?.as_text()?;
            let label = pair.get(1)?.as_text()?;
            Some((world_node, label))
        })
        .collect()
}

/// One fly-through danger-zone objective a scenario's `targets.zrd` declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioFlyThroughTarget {
    /// The scenario-local node label the target is about (`dz1`, `sghangar`).
    pub zone_label: String,
    /// The localized description key.
    pub description: String,
    /// The localized category key; measured `MSG_OBJ_DZ`.
    pub category_label: String,
    /// The localized help key; measured `MSG_OBJ_FLYTHROUGH`.
    pub help_label: String,
}

/// The fly-through danger-zone targets a scenario's `targets.zrd` declares.
///
/// A target is selected by its own `category_label`/`help_label` pair, not by
/// position, so a non-danger-zone objective (a zeppelin, a target building) is
/// never mistaken for a stunt gate. The returned `zone_label` is the first node
/// in the target's `nodes` list; a target that names no node is skipped rather
/// than given an empty name a consumer could not resolve.
#[must_use]
pub fn scenario_fly_through_targets(targets: &ZrdValue) -> Vec<ScenarioFlyThroughTarget> {
    let Some(entries) = targets.as_list() else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let category = zrd_field(entry, TARGET_CATEGORY_KEY)?.as_text()?;
            let help_label = zrd_field(entry, TARGET_HELP_KEY)?.as_text()?;
            if category != FLY_THROUGH_CATEGORY_LABEL && help_label != FLY_THROUGH_HELP_LABEL {
                return None;
            }
            let nodes = zrd_field(entry, TARGET_NODES_KEY)?.as_list()?;
            let zone_label = nodes.first()?.as_text()?.to_owned();
            let description = zrd_field(entry, TARGET_DESCRIPTION_KEY)
                .and_then(ZrdValue::as_text)
                .unwrap_or_default()
                .to_owned();
            Some(ScenarioFlyThroughTarget {
                zone_label,
                description,
                category_label: category.to_owned(),
                help_label: help_label.to_owned(),
            })
        })
        .collect()
}

/// Where one measured scenario target's bytes are, so a reader can go back to
/// them: the reader-archive container, its digest, the member and the member's
/// own span inside the container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StuntEncodingSpan {
    container: String,
    container_sha256: String,
    member: String,
    offset: u64,
    length: u64,
}

impl StuntEncodingSpan {
    /// Assembles one member's span.
    #[must_use]
    pub fn new(
        container: impl Into<String>,
        container_sha256: impl Into<String>,
        member: impl Into<String>,
        offset: u64,
        length: u64,
    ) -> Self {
        Self {
            container: container.into(),
            container_sha256: container_sha256.into(),
            member: member.into(),
            offset,
            length,
        }
    }

    /// The logical key of the container the member came from.
    #[must_use]
    pub fn container(&self) -> &str {
        &self.container
    }

    /// SHA-256 of that whole container file, from production discovery.
    #[must_use]
    pub fn container_sha256(&self) -> &str {
        &self.container_sha256
    }

    /// The member's name inside the reader archive.
    #[must_use]
    pub fn member(&self) -> &str {
        &self.member
    }

    /// Absolute container offset of the member's first byte.
    #[must_use]
    pub const fn offset(&self) -> u64 {
        self.offset
    }

    /// How many bytes the member occupies.
    #[must_use]
    pub const fn length(&self) -> u64 {
        self.length
    }
}

/// One fly-through danger-zone target, resolved to the world node and box it
/// names.
///
/// The label binding and the world box are both `Option`s because a scenario
/// may name a label its own `dzones` does not bind, or a node the world
/// container does not carry; either is a **reported gap**, not a dropped
/// target, so a consumer can see it rather than infer it from a shorter list.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailStuntGate {
    world: WorldId,
    mission_type: String,
    zone_label: String,
    world_zone: Option<String>,
    description: String,
    category_label: String,
    help_label: String,
    span: StuntEncodingSpan,
    geometry: Option<RetailTriggerVolume>,
}

impl RetailStuntGate {
    /// Assembles one resolved target.
    #[must_use]
    pub fn new(
        world: WorldId,
        mission_type: impl Into<String>,
        zone_label: impl Into<String>,
        world_zone: Option<String>,
        target: &ScenarioFlyThroughTarget,
        span: StuntEncodingSpan,
        geometry: Option<RetailTriggerVolume>,
    ) -> Self {
        Self {
            world,
            mission_type: mission_type.into(),
            zone_label: zone_label.into(),
            world_zone,
            description: target.description.clone(),
            category_label: target.category_label.clone(),
            help_label: target.help_label.clone(),
            span,
            geometry,
        }
    }

    /// The world group the scenario is set in.
    #[must_use]
    pub const fn world(&self) -> &WorldId {
        &self.world
    }

    /// The scenario mode the target was declared under.
    #[must_use]
    pub fn mission_type(&self) -> &str {
        &self.mission_type
    }

    /// The scenario-local node label the target names.
    #[must_use]
    pub fn zone_label(&self) -> &str {
        &self.zone_label
    }

    /// The world detection-zone node the label binds, when `dzones` binds it.
    #[must_use]
    pub fn world_zone(&self) -> Option<&str> {
        self.world_zone.as_deref()
    }

    /// The localized description key the target declares.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// The measured category key (`MSG_OBJ_DZ`).
    #[must_use]
    pub fn category_label(&self) -> &str {
        &self.category_label
    }

    /// The measured help key (`MSG_OBJ_FLYTHROUGH`).
    #[must_use]
    pub fn help_label(&self) -> &str {
        &self.help_label
    }

    /// Where the target's declaration's bytes are.
    #[must_use]
    pub const fn span(&self) -> &StuntEncodingSpan {
        &self.span
    }

    /// The world box task #427 measured for the bound node, when it resolved.
    #[must_use]
    pub const fn geometry(&self) -> Option<&RetailTriggerVolume> {
        self.geometry.as_ref()
    }

    /// Whether the target resolved all the way to a measured world box.
    #[must_use]
    pub const fn is_resolved(&self) -> bool {
        self.world_zone.is_some() && self.geometry.is_some()
    }
}

/// The original stunt encoding task #463 measured, one row per fly-through
/// danger-zone target.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailStuntEncodingSurvey {
    install_sha256: String,
    gates: Vec<RetailStuntGate>,
}

impl RetailStuntEncodingSurvey {
    /// Assembles the survey. No validation, because every row is a measurement
    /// and a duplicate is impossible: rows are keyed by their own span.
    #[must_use]
    pub fn new(install_sha256: impl Into<String>, gates: Vec<RetailStuntGate>) -> Self {
        Self {
            install_sha256: install_sha256.into(),
            gates,
        }
    }

    /// The installation fingerprint the measurement was taken over.
    #[must_use]
    pub fn install_sha256(&self) -> &str {
        &self.install_sha256
    }

    /// Every measured fly-through target, in discovery order.
    #[must_use]
    pub fn gates(&self) -> &[RetailStuntGate] {
        &self.gates
    }

    /// How many targets were measured.
    #[must_use]
    pub fn len(&self) -> usize {
        self.gates.len()
    }

    /// Whether no target was measured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.gates.is_empty()
    }

    /// The targets of the scenarios the original marks `stunt_flying`.
    pub fn stunt_flying_gates(&self) -> impl Iterator<Item = &RetailStuntGate> {
        let stunt = STUNT_MISSION_TYPE;
        self.gates
            .iter()
            .filter(move |gate| gate.mission_type == stunt)
    }

    /// The targets whose label bound no world node, or whose world node carries
    /// no measured box. A reported gap, never silently dropped.
    pub fn unresolved_gates(&self) -> impl Iterator<Item = &RetailStuntGate> {
        self.gates.iter().filter(|gate| !gate.is_resolved())
    }

    /// Whether every measured target resolved to a measured world box.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.gates.iter().all(RetailStuntGate::is_resolved)
    }

    /// Whether this survey measured a stunt's **direction rule**. It did not:
    /// the scenario bytes name a zone, not a direction, so a consumer must get
    /// that from another measurement rather than from this record.
    #[must_use]
    pub const fn direction_rule_is_measured(&self) -> bool {
        false
    }

    /// Whether this survey measured a stunt's **clearance rule**. It did not,
    /// for the same reason as [`Self::direction_rule_is_measured`].
    #[must_use]
    pub const fn clearance_rule_is_measured(&self) -> bool {
        false
    }

    /// Whether this survey measured a stunt's **reward or scrapbook media**. It
    /// did not: the scenario declares an objective, not a payout or a photo.
    #[must_use]
    pub const fn reward_is_measured(&self) -> bool {
        false
    }

    /// Whether this survey measured a stunt's **repeat policy**. It did not.
    #[must_use]
    pub const fn repeat_is_measured(&self) -> bool {
        false
    }
}

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
