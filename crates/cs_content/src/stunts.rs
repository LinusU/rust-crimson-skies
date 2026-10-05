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

// --------------------------------- the earning-authority surface (task #465) ---
//
// #463 measured *where* a stunt is spelled. It could not say **who** may earn
// one, because that is not a property of the bytes it measured. Task #465 asks
// the question directly, and its answer has three measured parts:
//
//   * the objective **records** (`targets.zrd`) carry six keys in total across
//     the whole installation, and none of them names a subject, an owner, a
//     team or an aircraft;
//   * the objective **state machine** (`objectives.zrd`) has exactly one
//     actor-scoped completion condition — `TRAVELERS`, whose first field is the
//     subject — and the original *does* use it with named non-player actors
//     (`secfury_5`, `wingman_3`, `devastator_1`, …). Its stunt completion
//     condition, `DANGER_ZONES_COMPLETED`, takes world zone names and **no**
//     actor at all;
//   * the instant-action scenario that declares the stunt zones also declares
//     non-player aircraft (wingmen, four enemy groups, a named ace), so AI
//     aircraft are part of every measured stunt scenario.
//
// What is **not** measured, and is not a field anywhere below: which aircraft the
// original credits for a zone crossing. No file records it. The survey reports
// the surface it measured and answers "unmeasured" with
// `RetailStuntAuthoritySurvey::earning_authority_is_measured()`, so a consumer
// cannot mistake "the data names no actor" for "an AI can never earn one".

/// The reader-archive member the objective state machine lives in (measured:
/// every reader that carries objectives declares one).
pub const SCENARIO_OBJECTIVES_MEMBER: &str = "objectives.zrd";

/// The measured marker key on an objective record with no value: the record is
/// a numbered objective rather than an ambient target.
pub const TARGET_OBJECTIVE_KEY: &str = "objective";

/// The measured marker key naming a second target of an objective.
pub const TARGET_OTHER_TARGET_KEY: &str = "other_target";

/// The measured `help_label` of a team-one objective (multiplayer readers).
pub const TEAM_ONE_HELP_LABEL: &str = "MSG_OBJ_TEAM_1";

/// The measured `help_label` of a team-two objective (multiplayer readers).
pub const TEAM_TWO_HELP_LABEL: &str = "MSG_OBJ_TEAM_2";

/// The prefix of a numbered objective block in the state machine
/// (`OBJECTIVE1`, `OBJECTIVE17`, …).
pub const OBJECTIVE_BLOCK_PREFIX: &str = "OBJECTIVE";

/// The original's own stunt completion condition: its value is a list of
/// `dzpath<N>` world detection-zone names and **no subject** (measured in 31
/// blocks).
pub const OBJECTIVE_DANGER_ZONES_KEY: &str = "DANGER_ZONES_COMPLETED";

/// How many of [`OBJECTIVE_DANGER_ZONES_KEY`]'s zones complete an objective
/// (measured in 6 blocks; absent from the other 25).
pub const OBJECTIVE_DANGER_ZONE_COUNT_KEY: &str = "DANGER_ZONES_COMPLETION_COUNT";

/// The only actor-scoped completion condition in the measured state machine.
/// Its first field is the subject (`player`, a named non-player actor, or a bare
/// index), its second the relation (`APPROACHING` / `LEAVING`), its third the
/// target.
pub const OBJECTIVE_TRAVELERS_KEY: &str = "TRAVELERS";

/// The scenario field naming the player's own aircraft.
pub const SCENARIO_PLAYER_PLANE_KEY: &str = "player_plane";

/// The scenario field counting the player's AI wingmen.
pub const SCENARIO_WINGMEN_KEY: &str = "num_wingmen";

/// The prefix of an enemy-group record in a scenario (`group1` … `group4`).
pub const SCENARIO_ENEMY_GROUP_PREFIX: &str = "group";

/// The enemy-group field counting its aircraft.
pub const ENEMY_COUNT_KEY: &str = "num_enemies";

/// The enemy-group field naming its aircraft.
pub const ENEMY_NAME_KEY: &str = "enemy_name";

/// The enemy-group field naming its aircraft type.
pub const ENEMY_PLANE_KEY: &str = "enemy_plane";

/// The enemy-group field naming its skill tier.
pub const ENEMY_SKILL_KEY: &str = "enemy_skill";

/// The measured `ace_name`: the label of the scenario's named ace.
pub const SCENARIO_ACE_NAME_KEY: &str = "ace_name";

/// The measured `ace_plane`: the aircraft type of the scenario's named ace.
pub const SCENARIO_ACE_PLANE_KEY: &str = "ace_plane";

/// The measured `ace_skill`: the skill tier of the scenario's named ace.
pub const SCENARIO_ACE_SKILL_KEY: &str = "ace_skill";

/// The measured lowest enemy skill tier.
pub const ENEMY_SKILL_NOVICE: &str = "novice";

/// The measured middle enemy skill tier.
pub const ENEMY_SKILL_VETERAN: &str = "veteran";

/// The measured highest enemy skill tier.
pub const ENEMY_SKILL_ACE: &str = "ace";

/// The actor name the original spells for the player (measured 43 times as the
/// subject of [`OBJECTIVE_TRAVELERS_KEY`]).
pub const PLAYER_ACTOR: &str = "player";

/// The prefix of a scenario-local detection-zone label (`dz1` … `dz18`).
///
/// Measured in #463's `dzones` bindings. It is a **lower bound**: the original
/// also authors named labels (`sghangar`, `h3_marker`), so a record carrying a
/// named label is not recognized by this prefix. The survey therefore reports
/// every [`OBJECTIVE_TRAVELERS_KEY`] target it read, not only the ones this
/// prefix claims.
pub const DANGER_ZONE_LABEL_PREFIX: &str = "dz";

/// The keys an objective record or objective block would carry **if** the
/// original scoped a completion to an earning authority.
///
/// Measured: none of them occurs anywhere in the installation's objective
/// records or objective blocks. The list is a declared search vocabulary, not a
/// claim that these are the spellings the original would have used.
pub const AUTHORITY_KEY_VOCABULARY: [&str; 12] = [
    "player",
    "player_only",
    "actor",
    "subject",
    "owner",
    "who",
    "pilot",
    "aircraft",
    "plane",
    "squadron",
    "faction",
    "team",
];

/// The flat alternating `.zrd` record shape, as a field list.
///
/// #463 measured two shapes: `ia.zrd` and `objectives.zrd` are *flat
/// alternating* (`["key", value, "key", value, …]`) while every `targets.zrd`
/// objective is a *list of pairs*. This reader is deliberately flat-only: a
/// shape-agnostic walk would read a value that happens to be a two-element list
/// (`INACTIVE1 ["fuel_truck01", "tank"]`) as a key/value pair and invent keys
/// out of the data, which is exactly the failure the authority census must not
/// have. A child that is not text is skipped, so a malformed tail cannot shift
/// the pairing for the rest of the record.
///
/// It **assumes every key has a value**. Inside an objective block that is false
/// (a bare directive is followed directly by the next key); use
/// [`zrd_directive_fields`] there.
#[must_use]
pub fn zrd_flat_fields(node: &ZrdValue) -> Vec<(&str, &ZrdValue)> {
    let Some(children) = node.as_list() else {
        return Vec::new();
    };
    let mut fields = Vec::new();
    let mut index = 0;
    while index + 1 < children.len() {
        if let Some(name) = children[index].as_text() {
            fields.push((name, &children[index + 1]));
            index += 2;
        } else {
            index += 1;
        }
    }
    fields
}

/// What a bare directive is paired with by [`zrd_directive_fields`]: an empty
/// list, so a reader that wants an argument finds none.
static BARE_DIRECTIVE_ARGUMENT: ZrdValue = ZrdValue::List(Vec::new());

/// Whether `value` is the placeholder [`zrd_directive_fields`] pairs with a bare
/// directive (by identity, so a genuine empty list is not mistaken for one).
#[must_use]
pub fn zrd_is_bare_argument(value: &ZrdValue) -> bool {
    std::ptr::eq(value, &BARE_DIRECTIVE_ARGUMENT)
}

/// An objective block's children read with the measured **directive grammar**.
///
/// [`zrd_flat_fields`] assumes every key is followed by exactly one value. The
/// original spells a no-argument directive (`INSTANTWIN`, `INSTANTLOSS`) by
/// leaving the next key beside it, so that walk pairs the bare key with the next
/// key's spelling and skips the next key entirely: 22 of the retail
/// installation's 1118 `BEGIN_DORMANT` sites vanish that way. Here a text
/// followed by another text (or by the end of the block) is a bare directive,
/// paired with [`zrd_is_bare_argument`]'s placeholder and advancing by one; any
/// other follower is its argument and the walk advances by two. This is the same
/// rule as `mission_control::measure_control_record`. A non-text child where a
/// key is expected is skipped, as in the flat walk.
#[must_use]
pub fn zrd_directive_fields(node: &ZrdValue) -> Vec<(&str, &ZrdValue)> {
    let Some(children) = node.as_list() else {
        return Vec::new();
    };
    let mut fields = Vec::new();
    let mut index = 0;
    while index < children.len() {
        let Some(name) = children[index].as_text() else {
            index += 1;
            continue;
        };
        match children.get(index + 1) {
            None | Some(ZrdValue::Text(_)) => {
                fields.push((name, &BARE_DIRECTIVE_ARGUMENT));
                index += 1;
            }
            Some(argument) => {
                fields.push((name, argument));
                index += 2;
            }
        }
    }
    fields
}

/// The keys every objective record in one `targets.zrd` member uses, with the
/// number of records each key appears in, sorted by key.
///
/// The result is a **complete inventory**, not a filtered one: a caller can
/// check the whole vocabulary, which is what makes "no objective record names an
/// earning authority" a measurement instead of an assumption. A record that is
/// not a list contributes nothing rather than a key of its own.
#[must_use]
pub fn objective_record_keys(targets: &ZrdValue) -> Vec<(String, u32)> {
    let mut counts: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    for record in targets.as_list().unwrap_or_default() {
        for child in record.as_list().unwrap_or_default() {
            let Some(pair) = child.as_list() else {
                continue;
            };
            let Some(name) = pair.first().and_then(ZrdValue::as_text) else {
                continue;
            };
            *counts.entry(name.to_owned()).or_insert(0) += 1;
        }
    }
    counts.into_iter().collect()
}

/// How many objective records one `targets.zrd` member declares.
///
/// The member's root is the list of records, so this is its length; a member
/// whose root is not a list declares none rather than being read as one record.
#[must_use]
pub fn objective_record_count(targets: &ZrdValue) -> u32 {
    targets.as_list().map_or(0, <[ZrdValue]>::len) as u32
}

/// Whether one objective record is **labelled** a fly-through danger-zone
/// target, reading either measured label.
///
/// This is the looser of the two readings and is deliberately kept beside
/// [`scenario_fly_through_targets`], whose selector additionally requires a
/// `category_label`. Measured over the whole installation the two disagree by
/// **three** records — all of them campaign-mission objectives that carry
/// `help_label = MSG_OBJ_FLYTHROUGH` and no `category_label` at all (C1/M02's
/// `h3_marker`, C4/M03's `dz2`, C5/M02's `dz1`) — so a survey that reported only
/// the stricter count would under-report the original's stunt records by three.
#[must_use]
pub fn is_fly_through_labelled(record: &ZrdValue) -> bool {
    let category = zrd_field(record, TARGET_CATEGORY_KEY).and_then(ZrdValue::as_text);
    let help = zrd_field(record, TARGET_HELP_KEY).and_then(ZrdValue::as_text);
    category == Some(FLY_THROUGH_CATEGORY_LABEL) || help == Some(FLY_THROUGH_HELP_LABEL)
}

/// How many objective records in one `targets.zrd` member are labelled a
/// fly-through danger-zone target by [`is_fly_through_labelled`].
#[must_use]
pub fn fly_through_labelled_objectives(targets: &ZrdValue) -> u32 {
    targets
        .as_list()
        .unwrap_or_default()
        .iter()
        .filter(|record| is_fly_through_labelled(record))
        .count() as u32
}

/// How many objective records in one `targets.zrd` member are **team**-scoped.
///
/// Measured: the multiplayer readers carry objectives whose `help_label` is
/// [`TEAM_ONE_HELP_LABEL`] or [`TEAM_TWO_HELP_LABEL`]. That is the one owning
/// authority the original's objective *record* layer does carry — a team, never
/// an aircraft — and it is why "an objective record never names an owner" would
/// be too strong a reading.
#[must_use]
pub fn team_scoped_objectives(targets: &ZrdValue) -> u32 {
    targets
        .as_list()
        .unwrap_or_default()
        .iter()
        .filter(|record| {
            let help = zrd_field(record, TARGET_HELP_KEY).and_then(ZrdValue::as_text);
            help == Some(TEAM_ONE_HELP_LABEL) || help == Some(TEAM_TWO_HELP_LABEL)
        })
        .count() as u32
}

/// One enemy group a scenario declares at its root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioEnemyGroup {
    index: u32,
    name_label: Option<String>,
    plane: Option<String>,
    skill: Option<String>,
    count: Option<u32>,
}

impl ScenarioEnemyGroup {
    /// Assembles one group from the measured `group<N>` record.
    #[must_use]
    pub fn new(
        index: u32,
        name_label: Option<String>,
        plane: Option<String>,
        skill: Option<String>,
        count: Option<u32>,
    ) -> Self {
        Self {
            index,
            name_label,
            plane,
            skill,
            count,
        }
    }

    /// The record's own number, as the author wrote it (`group3` → `3`).
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// The localized label of the group's aircraft, when the record names one.
    #[must_use]
    pub fn name_label(&self) -> Option<&str> {
        self.name_label.as_deref()
    }

    /// The group's aircraft type (`Firebrand`, …), when the record names one.
    #[must_use]
    pub fn plane(&self) -> Option<&str> {
        self.plane.as_deref()
    }

    /// The group's skill tier (`novice` / `veteran` / `ace`), when declared.
    #[must_use]
    pub fn skill(&self) -> Option<&str> {
        self.skill.as_deref()
    }

    /// How many aircraft the record declares, when it declares a count.
    ///
    /// This is an authored number, not a spawn count: nothing in the data says
    /// every declared aircraft is placed.
    #[must_use]
    pub const fn count(&self) -> Option<u32> {
        self.count
    }
}

/// The named ace a scenario declares at its root.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScenarioAce {
    name_label: Option<String>,
    plane: Option<String>,
    skill: Option<String>,
}

impl ScenarioAce {
    /// The localized label of the ace (`MSG_SSCRAWFORD_NAME`), when declared.
    #[must_use]
    pub fn name_label(&self) -> Option<&str> {
        self.name_label.as_deref()
    }

    /// The ace's aircraft type, when declared.
    #[must_use]
    pub fn plane(&self) -> Option<&str> {
        self.plane.as_deref()
    }

    /// The ace's skill tier, when declared.
    #[must_use]
    pub fn skill(&self) -> Option<&str> {
        self.skill.as_deref()
    }

    /// Whether the scenario declared any part of an ace.
    #[must_use]
    pub const fn is_declared(&self) -> bool {
        self.name_label.is_some() || self.plane.is_some() || self.skill.is_some()
    }
}

/// The non-player aircraft one scenario declares.
///
/// This is the measured hazard for the runtime's `StuntAuthority::AiFlight`
/// refusal: the four scenarios the original marks `stunt_flying` each declare
/// wingmen, four enemy groups and an ace **in the same record that declares
/// their stunt zones**. Nothing here says which of those aircraft may fly a
/// gate; that is the unmeasured question.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScenarioNonPlayerAircraft {
    player_plane: Option<String>,
    wingmen: Option<u32>,
    enemy_groups: Vec<ScenarioEnemyGroup>,
    ace: ScenarioAce,
}

impl ScenarioNonPlayerAircraft {
    /// The player's own aircraft type, when the scenario names one.
    #[must_use]
    pub fn player_plane(&self) -> Option<&str> {
        self.player_plane.as_deref()
    }

    /// How many AI wingmen the scenario declares, when it declares a count.
    #[must_use]
    pub const fn wingmen(&self) -> Option<u32> {
        self.wingmen
    }

    /// The enemy groups, ordered by their authored record number.
    #[must_use]
    pub fn enemy_groups(&self) -> &[ScenarioEnemyGroup] {
        &self.enemy_groups
    }

    /// The scenario's named ace.
    #[must_use]
    pub const fn ace(&self) -> &ScenarioAce {
        &self.ace
    }

    /// Whether the scenario declares at least one **measurable** non-player
    /// aircraft fact.
    ///
    /// A group record the reader found but whose fields are all absent
    /// (`group3` with a value that is not a record) contributes nothing here:
    /// the record's presence is reported by [`Self::enemy_groups`], its content
    /// by this predicate.
    #[must_use]
    pub fn is_declared(&self) -> bool {
        self.wingmen.is_some()
            || self.ace.is_declared()
            || self.enemy_groups.iter().any(|group| {
                group.count.is_some()
                    || group.plane.is_some()
                    || group.skill.is_some()
                    || group.name_label.is_some()
            })
    }

    /// The sum of the enemy groups' authored counts.
    ///
    /// Arithmetic over measured values, deliberately **not** a total aircraft
    /// count: the ace, the wingmen and any aircraft an enemy group spawns at
    /// runtime are not part of it.
    #[must_use]
    pub fn summed_enemy_group_counts(&self) -> u32 {
        self.enemy_groups
            .iter()
            .filter_map(ScenarioEnemyGroup::count)
            .sum()
    }
}

/// One text value the original wraps in a one-element list (`["Kestrel"]`).
fn zrd_wrapped_text(value: &ZrdValue) -> Option<String> {
    if let Some(text) = value.as_text() {
        return Some(text.to_owned());
    }
    let list = value.as_list()?;
    if list.len() == 1 {
        return list[0].as_text().map(str::to_owned);
    }
    None
}

/// One integer value the original wraps in a one-element list (`[3]`).
fn zrd_wrapped_int(value: &ZrdValue) -> Option<u32> {
    if let Some(number) = value.as_int() {
        return Some(number);
    }
    let list = value.as_list()?;
    if list.len() == 1 {
        return list[0].as_int();
    }
    None
}

/// `group3` → `Some(3)`; anything else → `None`.
fn enemy_group_index(key: &str) -> Option<u32> {
    key.strip_prefix(SCENARIO_ENEMY_GROUP_PREFIX)
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|digits| digits.parse().ok())
}

/// `OBJECTIVE17` → `Some("OBJECTIVE17")`; `OBJECTIVE_X` → `None`.
fn objective_block_id(key: &str) -> Option<&str> {
    key.strip_prefix(OBJECTIVE_BLOCK_PREFIX).and_then(|digits| {
        (!digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())).then_some(key)
    })
}

/// The non-player aircraft one scenario descriptor declares.
///
/// Measured shape: `ia.zrd` is a flat alternating record whose root carries
/// `player_plane`, `num_wingmen`, `group1` … `group4` and `ace_*`. A scenario
/// that declares none of them yields an all-empty record with
/// [`ScenarioNonPlayerAircraft::is_declared`] false — never a zero filled in as
/// if it were measured.
#[must_use]
pub fn scenario_non_player_aircraft(scenario: &ZrdValue) -> ScenarioNonPlayerAircraft {
    let mut aircraft = ScenarioNonPlayerAircraft::default();
    let mut groups: Vec<ScenarioEnemyGroup> = Vec::new();
    for (key, value) in zrd_flat_fields(scenario) {
        match key {
            SCENARIO_PLAYER_PLANE_KEY => aircraft.player_plane = zrd_wrapped_text(value),
            SCENARIO_WINGMEN_KEY => aircraft.wingmen = zrd_wrapped_int(value),
            SCENARIO_ACE_NAME_KEY => aircraft.ace.name_label = zrd_wrapped_text(value),
            SCENARIO_ACE_PLANE_KEY => aircraft.ace.plane = zrd_wrapped_text(value),
            SCENARIO_ACE_SKILL_KEY => aircraft.ace.skill = zrd_wrapped_text(value),
            _ => {
                let Some(index) = enemy_group_index(key) else {
                    continue;
                };
                let mut group = ScenarioEnemyGroup::new(index, None, None, None, None);
                for (field, field_value) in zrd_flat_fields(value) {
                    match field {
                        ENEMY_COUNT_KEY => group.count = zrd_wrapped_int(field_value),
                        ENEMY_NAME_KEY => group.name_label = zrd_wrapped_text(field_value),
                        ENEMY_PLANE_KEY => group.plane = zrd_wrapped_text(field_value),
                        ENEMY_SKILL_KEY => group.skill = zrd_wrapped_text(field_value),
                        _ => {}
                    }
                }
                groups.push(group);
            }
        }
    }
    groups.sort_by_key(ScenarioEnemyGroup::index);
    aircraft.enemy_groups = groups;
    aircraft
}

/// The subject of one measured [`OBJECTIVE_TRAVELERS_KEY`] condition.
///
/// `Player` is the original's own spelling. `Named` is a non-player actor the
/// data names outright (an AI aircraft or a zeppelin). `Indexed` is a bare
/// integer: the corpus contains six, they are **not decoded** here, and
/// correlating them with the scenario's actor name table is a lead, not a
/// measurement. `Unreadable` is a first field that is neither text nor int,
/// recorded so a shape change is reported instead of dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TravellerSubject {
    /// The original's `player` actor.
    Player,
    /// A named actor, which in the measured corpus is a non-player aircraft or
    /// zeppelin.
    Named(String),
    /// A bare index whose meaning is not measured.
    Indexed(u32),
    /// A first field of a shape this reader does not decode.
    Unreadable,
}

/// One measured actor-scoped completion condition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TravellerCondition {
    objective: String,
    subject: TravellerSubject,
    relation: Option<String>,
    target: Option<String>,
    danger_zone_label: bool,
}

impl TravellerCondition {
    /// Assembles one condition from its measured fields.
    #[must_use]
    pub fn new(
        objective: impl Into<String>,
        subject: TravellerSubject,
        relation: Option<String>,
        target: Option<String>,
    ) -> Self {
        let danger_zone_label = target
            .as_deref()
            .is_some_and(|target| target.starts_with(DANGER_ZONE_LABEL_PREFIX));
        Self {
            objective: objective.into(),
            subject,
            relation,
            target,
            danger_zone_label,
        }
    }

    /// The numbered objective block the condition belongs to (`OBJECTIVE17`).
    #[must_use]
    pub fn objective(&self) -> &str {
        &self.objective
    }

    /// Who the condition is about.
    #[must_use]
    pub const fn subject(&self) -> &TravellerSubject {
        &self.subject
    }

    /// The relation the original spells (`APPROACHING`, `LEAVING`).
    #[must_use]
    pub fn relation(&self) -> Option<&str> {
        self.relation.as_deref()
    }

    /// The condition's target: a node name, a label, or — measured three times —
    /// a detection-zone label.
    #[must_use]
    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    /// Whether the target starts with the measured detection-zone label prefix.
    ///
    /// A **lower bound**: a named label (`sghangar`) is a zone target too and is
    /// not recognized by the prefix, so a consumer must read
    /// [`Self::target`] as well.
    #[must_use]
    pub const fn target_is_danger_zone_label(&self) -> bool {
        self.danger_zone_label
    }

    /// Whether the subject is anything other than [`TravellerSubject::Player`].
    #[must_use]
    pub const fn subject_is_non_player(&self) -> bool {
        !matches!(self.subject, TravellerSubject::Player)
    }
}

/// One measured stunt completion condition: the danger zones whose completion
/// completes a numbered objective.
///
/// The condition carries zone names and an optional required count. It carries
/// **no subject**, which is the measurement task #465 exists to record. Its
/// [`Self::keys`] are the **complete** field inventory of the block it lives
/// in (not only the fields this reader interprets), which is the surface task
/// #464 measures a payout or a repeat policy against: a block that paid a
/// reward or stated a repeat rule would carry that key here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StuntCompletionCondition {
    objective: String,
    zones: Vec<String>,
    required_count: Option<u32>,
    keys: Vec<(String, u32)>,
}

impl StuntCompletionCondition {
    /// Assembles one condition with the complete key inventory of its block.
    #[must_use]
    pub fn new(
        objective: impl Into<String>,
        zones: Vec<String>,
        required_count: Option<u32>,
        keys: Vec<(String, u32)>,
    ) -> Self {
        Self {
            objective: objective.into(),
            zones,
            required_count,
            keys,
        }
    }

    /// The numbered objective block the condition belongs to.
    #[must_use]
    pub fn objective(&self) -> &str {
        &self.objective
    }

    /// The `dzpath<N>` world zones the condition names, in authored order.
    #[must_use]
    pub fn zones(&self) -> &[String] {
        &self.zones
    }

    /// How many of [`Self::zones`] complete the objective, when the block
    /// declares a count. `None` means the block declared none — not "zero".
    #[must_use]
    pub const fn required_count(&self) -> Option<u32> {
        self.required_count
    }

    /// The complete key vocabulary of the block, sorted by key, with the number
    /// of times each key occurs in it.
    #[must_use]
    pub fn keys(&self) -> &[(String, u32)] {
        &self.keys
    }

    /// The keys of the block that name a payout, per [`REWARD_KEY_VOCABULARY`].
    #[must_use]
    pub fn reward_keys(&self) -> Vec<&str> {
        vocabulary_keys(&self.keys, &REWARD_KEY_VOCABULARY)
    }

    /// The keys of the block that name a repeat policy, per
    /// [`REPEAT_KEY_VOCABULARY`].
    #[must_use]
    pub fn repeat_keys(&self) -> Vec<&str> {
        vocabulary_keys(&self.keys, &REPEAT_KEY_VOCABULARY)
    }
}

/// The measured objective state machine of one reader.
///
/// Everything in it is a **field name or a field order**, never a decoded
/// behaviour: which objective number a completion wakes, what a nap duration
/// means and what an integer subject indexes are all unmeasured.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjectiveStateMachine {
    blocks: u32,
    keys: Vec<(String, u32)>,
    stunt_conditions: Vec<StuntCompletionCondition>,
    travellers: Vec<TravellerCondition>,
}

impl ObjectiveStateMachine {
    /// How many numbered `OBJECTIVE<N>` blocks the member declares.
    #[must_use]
    pub const fn blocks(&self) -> u32 {
        self.blocks
    }

    /// The complete key vocabulary inside the blocks, sorted by key, with the
    /// number of blocks each key appears in.
    #[must_use]
    pub fn keys(&self) -> &[(String, u32)] {
        &self.keys
    }

    /// Every measured stunt completion condition, in authored order.
    #[must_use]
    pub fn stunt_conditions(&self) -> &[StuntCompletionCondition] {
        &self.stunt_conditions
    }

    /// Every measured actor-scoped condition, in authored order.
    #[must_use]
    pub fn travellers(&self) -> &[TravellerCondition] {
        &self.travellers
    }
}

/// The record a wrapped `.zrd` member holds: measured in every member of the
/// installation, the root is a one-element list holding one flat alternating
/// record. This is the shape of both `objectives.zrd` and the global reader's
/// `player.zrd` (the score table), so the unwrapping is shared.
///
/// A document that is already a record (or whose single child is not a record)
/// is read as itself, so a hand-authored document needs no wrapper and a
/// wrapper-only document cannot hide its record.
#[must_use]
pub fn objective_record(document: &ZrdValue) -> &ZrdValue {
    match document.as_list() {
        Some([only]) if only.as_list().is_some() => only,
        _ => document,
    }
}

/// The objective state machine one `objectives.zrd` member declares.
///
/// Measured over the installation: the keys inside the numbered blocks are the
/// objective machine's complete vocabulary, [`OBJECTIVE_DANGER_ZONES_KEY`] is the
/// original's own stunt completion condition and [`OBJECTIVE_TRAVELERS_KEY`] the
/// only actor-scoped one. Nothing here decodes what a block *does*.
#[must_use]
pub fn objective_state_machine(document: &ZrdValue) -> ObjectiveStateMachine {
    let mut counts: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    let mut machine = ObjectiveStateMachine::default();
    for (key, value) in zrd_flat_fields(objective_record(document)) {
        let Some(block) = objective_block_id(key) else {
            continue;
        };
        machine.blocks += 1;
        let mut zones: Vec<String> = Vec::new();
        let mut required_count = None;
        let mut block_counts: std::collections::BTreeMap<String, u32> =
            std::collections::BTreeMap::new();
        for (field, field_value) in zrd_directive_fields(value) {
            *counts.entry(field.to_owned()).or_insert(0) += 1;
            *block_counts.entry(field.to_owned()).or_insert(0) += 1;
            match field {
                OBJECTIVE_DANGER_ZONES_KEY => {
                    zones = field_value
                        .as_list()
                        .unwrap_or_default()
                        .iter()
                        .filter_map(ZrdValue::as_text)
                        .map(str::to_owned)
                        .collect();
                }
                OBJECTIVE_DANGER_ZONE_COUNT_KEY => required_count = zrd_wrapped_int(field_value),
                OBJECTIVE_TRAVELERS_KEY => {
                    let Some(entries) = field_value.as_list() else {
                        continue;
                    };
                    let subject = match entries.first() {
                        Some(ZrdValue::Text(name)) if name == PLAYER_ACTOR => {
                            TravellerSubject::Player
                        }
                        Some(ZrdValue::Text(name)) => TravellerSubject::Named(name.clone()),
                        Some(ZrdValue::Int(index)) => TravellerSubject::Indexed(*index),
                        _ => TravellerSubject::Unreadable,
                    };
                    machine.travellers.push(TravellerCondition::new(
                        block,
                        subject,
                        entries
                            .get(1)
                            .and_then(ZrdValue::as_text)
                            .map(str::to_owned),
                        entries
                            .get(2)
                            .and_then(ZrdValue::as_text)
                            .map(str::to_owned),
                    ));
                }
                _ => {}
            }
        }
        if !zones.is_empty() {
            machine.stunt_conditions.push(StuntCompletionCondition::new(
                block,
                zones,
                required_count,
                block_counts.into_iter().collect(),
            ));
        }
    }
    machine.keys = counts.into_iter().collect();
    machine
}

/// The measured objective records of one `targets.zrd` member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetailObjectiveCorpus {
    count: u32,
    fly_through: u32,
    fly_through_labelled: u32,
    team_scoped: u32,
    keys: Vec<(String, u32)>,
    span: StuntEncodingSpan,
}

impl RetailObjectiveCorpus {
    /// Assembles one corpus.
    #[must_use]
    pub fn new(
        count: u32,
        fly_through: u32,
        fly_through_labelled: u32,
        team_scoped: u32,
        keys: Vec<(String, u32)>,
        span: StuntEncodingSpan,
    ) -> Self {
        Self {
            count,
            fly_through,
            fly_through_labelled,
            team_scoped,
            keys,
            span,
        }
    }

    /// How many objective records the member declares.
    #[must_use]
    pub const fn count(&self) -> u32 {
        self.count
    }

    /// How many are fly-through danger-zone targets (task #463's selector, which
    /// requires a `category_label` as well as the help label).
    #[must_use]
    pub const fn fly_through(&self) -> u32 {
        self.fly_through
    }

    /// How many carry **either** measured fly-through label, whether or not the
    /// record also carries the other one.
    ///
    /// Measured over the whole installation this is three records higher than
    /// [`Self::fly_through`]; see [`is_fly_through_labelled`].
    #[must_use]
    pub const fn fly_through_labelled(&self) -> u32 {
        self.fly_through_labelled
    }

    /// How many are team-scoped objectives (`MSG_OBJ_TEAM_1` / `_2`).
    #[must_use]
    pub const fn team_scoped(&self) -> u32 {
        self.team_scoped
    }

    /// The member's complete objective key vocabulary, sorted by key.
    #[must_use]
    pub fn keys(&self) -> &[(String, u32)] {
        &self.keys
    }

    /// Where the member's bytes are.
    #[must_use]
    pub const fn span(&self) -> &StuntEncodingSpan {
        &self.span
    }

    /// The keys of this corpus that name an earning authority, per
    /// [`AUTHORITY_KEY_VOCABULARY`].
    #[must_use]
    pub fn authority_keys(&self) -> Vec<&str> {
        self.keys
            .iter()
            .map(|(key, _)| key.as_str())
            .filter(|key| AUTHORITY_KEY_VOCABULARY.contains(key))
            .collect()
    }

    /// The keys of this corpus that name a payout, per
    /// [`REWARD_KEY_VOCABULARY`].
    #[must_use]
    pub fn reward_keys(&self) -> Vec<&str> {
        vocabulary_keys(&self.keys, &REWARD_KEY_VOCABULARY)
    }

    /// The keys of this corpus that name a repeat policy, per
    /// [`REPEAT_KEY_VOCABULARY`].
    #[must_use]
    pub fn repeat_keys(&self) -> Vec<&str> {
        vocabulary_keys(&self.keys, &REPEAT_KEY_VOCABULARY)
    }
}

/// The measured objective state machine of one reader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetailObjectiveMachine {
    machine: ObjectiveStateMachine,
    span: StuntEncodingSpan,
}

impl RetailObjectiveMachine {
    /// Assembles one machine row.
    #[must_use]
    pub fn new(machine: ObjectiveStateMachine, span: StuntEncodingSpan) -> Self {
        Self { machine, span }
    }

    /// The extracted machine.
    #[must_use]
    pub const fn machine(&self) -> &ObjectiveStateMachine {
        &self.machine
    }

    /// Where the member's bytes are.
    #[must_use]
    pub const fn span(&self) -> &StuntEncodingSpan {
        &self.span
    }

    /// The machine keys that name an earning authority, per
    /// [`AUTHORITY_KEY_VOCABULARY`].
    #[must_use]
    pub fn authority_keys(&self) -> Vec<&str> {
        self.machine
            .keys
            .iter()
            .map(|(key, _)| key.as_str())
            .filter(|key| AUTHORITY_KEY_VOCABULARY.contains(key))
            .collect()
    }

    /// The machine keys that name a payout, per [`REWARD_KEY_VOCABULARY`].
    #[must_use]
    pub fn reward_keys(&self) -> Vec<&str> {
        vocabulary_keys(&self.machine.keys, &REWARD_KEY_VOCABULARY)
    }

    /// The machine keys that name a repeat policy, per
    /// [`REPEAT_KEY_VOCABULARY`].
    #[must_use]
    pub fn repeat_keys(&self) -> Vec<&str> {
        vocabulary_keys(&self.machine.keys, &REPEAT_KEY_VOCABULARY)
    }
}

/// The measured scenario descriptor of one instant-action reader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetailScenarioAuthority {
    mission_type: String,
    aircraft: ScenarioNonPlayerAircraft,
    span: StuntEncodingSpan,
}

impl RetailScenarioAuthority {
    /// Assembles one scenario row.
    #[must_use]
    pub fn new(
        mission_type: impl Into<String>,
        aircraft: ScenarioNonPlayerAircraft,
        span: StuntEncodingSpan,
    ) -> Self {
        Self {
            mission_type: mission_type.into(),
            aircraft,
            span,
        }
    }

    /// The scenario mode the descriptor declares.
    #[must_use]
    pub fn mission_type(&self) -> &str {
        &self.mission_type
    }

    /// The non-player aircraft the same record declares.
    #[must_use]
    pub const fn aircraft(&self) -> &ScenarioNonPlayerAircraft {
        &self.aircraft
    }

    /// Where the member's bytes are.
    #[must_use]
    pub const fn span(&self) -> &StuntEncodingSpan {
        &self.span
    }
}

/// One reader archive's measured earning-authority surface.
///
/// A member is [`None`] when the reader carries none: an instant-action reader
/// has no objective state machine block set and a campaign mission has no
/// `ia.zrd`. That is a measured absence, not a skipped row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetailObjectiveAuthorityRow {
    container: String,
    container_sha256: String,
    objectives: Option<RetailObjectiveCorpus>,
    machine: Option<RetailObjectiveMachine>,
    scenario: Option<RetailScenarioAuthority>,
}

impl RetailObjectiveAuthorityRow {
    /// Assembles one row.
    #[must_use]
    pub fn new(
        container: impl Into<String>,
        container_sha256: impl Into<String>,
        objectives: Option<RetailObjectiveCorpus>,
        machine: Option<RetailObjectiveMachine>,
        scenario: Option<RetailScenarioAuthority>,
    ) -> Self {
        Self {
            container: container.into(),
            container_sha256: container_sha256.into(),
            objectives,
            machine,
            scenario,
        }
    }

    /// The reader archive's logical key.
    #[must_use]
    pub fn container(&self) -> &str {
        &self.container
    }

    /// SHA-256 of that whole container, from production discovery.
    #[must_use]
    pub fn container_sha256(&self) -> &str {
        &self.container_sha256
    }

    /// The reader's objective records, when it declares any.
    #[must_use]
    pub const fn objectives(&self) -> Option<&RetailObjectiveCorpus> {
        self.objectives.as_ref()
    }

    /// The reader's objective state machine, when it declares one.
    #[must_use]
    pub const fn machine(&self) -> Option<&RetailObjectiveMachine> {
        self.machine.as_ref()
    }

    /// The reader's scenario descriptor, when it carries one.
    #[must_use]
    pub const fn scenario(&self) -> Option<&RetailScenarioAuthority> {
        self.scenario.as_ref()
    }
}

/// How many measured conditions name each kind of subject.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TravellerSubjectCensus {
    /// Conditions whose subject is the original's `player` actor.
    pub player: u32,
    /// Conditions whose subject is a named non-player actor.
    pub named: u32,
    /// Conditions whose subject is a bare, undecoded index.
    pub indexed: u32,
    /// Conditions whose subject this reader could not decode.
    pub unreadable: u32,
}

/// The measured earning-authority surface of a whole installation (task #465).
///
/// One row per reader archive. The survey reports what the objective data
/// **contains** — its complete key vocabularies, its stunt completion
/// conditions, its actor-scoped conditions and each scenario's declared
/// aircraft — and answers the rule itself with
/// [`Self::earning_authority_is_measured`], which is `false`: no measured file
/// records which aircraft a zone crossing credits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetailStuntAuthoritySurvey {
    install_sha256: String,
    rows: Vec<RetailObjectiveAuthorityRow>,
}

impl RetailStuntAuthoritySurvey {
    /// Assembles the survey. Rows are keyed by their own container, so a
    /// duplicate is impossible and no validation is needed.
    #[must_use]
    pub fn new(install_sha256: impl Into<String>, rows: Vec<RetailObjectiveAuthorityRow>) -> Self {
        Self {
            install_sha256: install_sha256.into(),
            rows,
        }
    }

    /// The installation fingerprint the measurement was taken over.
    #[must_use]
    pub fn install_sha256(&self) -> &str {
        &self.install_sha256
    }

    /// Every measured reader archive, in inventory order.
    #[must_use]
    pub fn rows(&self) -> &[RetailObjectiveAuthorityRow] {
        &self.rows
    }

    /// How many reader archives were walked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether no reader archive was walked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Every measured objective record.
    #[must_use]
    pub fn objective_records(&self) -> u32 {
        self.rows
            .iter()
            .filter_map(|row| row.objectives())
            .map(RetailObjectiveCorpus::count)
            .sum()
    }

    /// Every measured fly-through danger-zone objective (task #463's selector).
    #[must_use]
    pub fn fly_through_objectives(&self) -> u32 {
        self.rows
            .iter()
            .filter_map(|row| row.objectives())
            .map(RetailObjectiveCorpus::fly_through)
            .sum()
    }

    /// Every objective record that carries **either** measured fly-through
    /// label.
    ///
    /// Measured over the owner's installation: three more than
    /// [`Self::fly_through_objectives`], because three campaign-mission records
    /// carry `help_label = MSG_OBJ_FLYTHROUGH` and no `category_label`. A
    /// consumer that reported only the stricter count would under-report the
    /// original's own stunt records.
    #[must_use]
    pub fn fly_through_labelled_objectives(&self) -> u32 {
        self.rows
            .iter()
            .filter_map(|row| row.objectives())
            .map(RetailObjectiveCorpus::fly_through_labelled)
            .sum()
    }

    /// Every measured team-scoped objective.
    #[must_use]
    pub fn team_scoped_objectives(&self) -> u32 {
        self.rows
            .iter()
            .filter_map(|row| row.objectives())
            .map(RetailObjectiveCorpus::team_scoped)
            .sum()
    }

    /// Every measured numbered objective block.
    #[must_use]
    pub fn objective_blocks(&self) -> u32 {
        self.rows
            .iter()
            .filter_map(|row| row.machine())
            .map(|machine| machine.machine().blocks())
            .sum()
    }

    /// The complete key vocabulary of the objective records **and** of the
    /// objective blocks, summed over every row and sorted by key.
    #[must_use]
    pub fn objective_keys(&self) -> Vec<(String, u32)> {
        let mut counts: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
        for row in &self.rows {
            if let Some(corpus) = row.objectives() {
                for (key, count) in &corpus.keys {
                    *counts.entry(key.clone()).or_insert(0) += count;
                }
            }
            if let Some(machine) = row.machine() {
                for (key, count) in machine.machine().keys() {
                    *counts.entry(key.clone()).or_insert(0) += count;
                }
            }
        }
        counts.into_iter().collect()
    }

    /// Every `(container, key)` whose key names an earning authority, per
    /// [`AUTHORITY_KEY_VOCABULARY`], in row order.
    ///
    /// Measured over the owner's installation: **empty**. That is a statement
    /// about the objective data's vocabulary, not about what the game does.
    #[must_use]
    pub fn keys_naming_an_authority(&self) -> Vec<(String, String)> {
        let mut found = Vec::new();
        for row in &self.rows {
            if let Some(corpus) = row.objectives() {
                for key in corpus.authority_keys() {
                    found.push((row.container.clone(), key.to_owned()));
                }
            }
            if let Some(machine) = row.machine() {
                for key in machine.authority_keys() {
                    found.push((row.container.clone(), key.to_owned()));
                }
            }
        }
        found
    }

    /// Every measured stunt completion condition, in row then authored order.
    pub fn stunt_conditions(&self) -> impl Iterator<Item = &StuntCompletionCondition> {
        self.rows
            .iter()
            .filter_map(|row| row.machine())
            .flat_map(|machine| machine.machine().stunt_conditions())
    }

    /// Every measured actor-scoped condition, in row then authored order.
    pub fn travellers(&self) -> impl Iterator<Item = &TravellerCondition> {
        self.rows
            .iter()
            .filter_map(|row| row.machine())
            .flat_map(|machine| machine.machine().travellers())
    }

    /// How many measured conditions name each kind of subject.
    #[must_use]
    pub fn traveller_subject_census(&self) -> TravellerSubjectCensus {
        let mut census = TravellerSubjectCensus::default();
        for condition in self.travellers() {
            match condition.subject() {
                TravellerSubject::Player => census.player += 1,
                TravellerSubject::Named(_) => census.named += 1,
                TravellerSubject::Indexed(_) => census.indexed += 1,
                TravellerSubject::Unreadable => census.unreadable += 1,
            }
        }
        census
    }

    /// The distinct non-player subjects the original names outright, sorted.
    #[must_use]
    pub fn non_player_subjects(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .travellers()
            .filter_map(|condition| match condition.subject() {
                TravellerSubject::Named(name) => Some(name.as_str()),
                _ => None,
            })
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// The actor-scoped conditions whose target is a detection-zone label.
    ///
    /// A lower bound by construction ([`TravellerCondition::target_is_danger_zone_label`]
    /// recognizes the measured `dz…` prefix only); read
    /// [`TravellerCondition::target`] for the rest.
    pub fn traveller_conditions_naming_a_danger_zone(
        &self,
    ) -> impl Iterator<Item = &TravellerCondition> {
        self.travellers()
            .filter(|condition| condition.target_is_danger_zone_label())
    }

    /// The actor-scoped conditions that name a detection-zone label **and** a
    /// non-player subject.
    ///
    /// Measured over the owner's installation: **empty** — every measured stunt
    /// condition is either anonymous ([`Self::stunt_conditions`]) or names
    /// `player` as its subject.
    pub fn non_player_danger_zone_conditions(&self) -> impl Iterator<Item = &TravellerCondition> {
        self.traveller_conditions_naming_a_danger_zone()
            .filter(|condition| condition.subject_is_non_player())
    }

    /// Every measured scenario descriptor.
    pub fn scenarios(&self) -> impl Iterator<Item = &RetailScenarioAuthority> {
        self.rows.iter().filter_map(|row| row.scenario())
    }

    /// The scenario descriptors that also declare non-player aircraft.
    pub fn scenarios_declaring_non_player_aircraft(
        &self,
    ) -> impl Iterator<Item = &RetailScenarioAuthority> {
        self.scenarios()
            .filter(|scenario| scenario.aircraft().is_declared())
    }

    /// The scenario descriptors of the scenarios the original marks
    /// [`STUNT_MISSION_TYPE`].
    pub fn stunt_flying_scenarios(&self) -> impl Iterator<Item = &RetailScenarioAuthority> {
        self.scenarios()
            .filter(|scenario| scenario.mission_type() == STUNT_MISSION_TYPE)
    }

    /// Whether this survey measured **which actor may earn a stunt**. It did
    /// not, and it cannot: no measured file records the earning authority of a
    /// zone crossing.
    ///
    /// A consumer must treat the *contents* of this survey (the key
    /// vocabularies, the conditions, the declared aircraft) as what was
    /// measured, and any earning rule as still unknown until an original run
    /// settles it.
    #[must_use]
    pub const fn earning_authority_is_measured(&self) -> bool {
        false
    }
}

// ------------------------------- the reward and repeat surface (task #464) ---
//
// #463 measured *where* a stunt is spelled and #465 measured *who* may earn
// one. Neither can say what a completion **pays** or whether a second pass
// pays again, because the objective bytes that spell a stunt name a zone and
// an objective number and nothing else. Task #464 asks the payout question
// directly, and its answer has three measured parts:
//
//   * the objective **blocks** that carry the original's own stunt completion
//     condition (`DANGER_ZONES_COMPLETED`) carry a complete, closed key
//     inventory, and **none** of those keys names a payout or a repeat policy;
//   * the objective **records** and the objective **state machine** carry no
//     such key either;
//   * the only numeric score table in the whole installation is the five-key
//     `score_*` multiplayer match table in the global reader's `player.zrd`
//     (kill, return flag, suicide, zep, enemy flag), and it names no stunt,
//     no danger zone and no photo.
//
// What is **not** measured, and is not a field anywhere below: how many fame
// points or how much cash the original paid for a stunt, and whether a repeat
// traversal paid again. No file records either. The survey reports the surface
// it measured and answers "unmeasured" with
// `RetailStuntRewardSurvey::reward_is_measured()` and
// `RetailStuntRewardSurvey::repeat_is_measured()`, so a consumer cannot mistake
// "the data names no payout" for "the original paid nothing".

/// The reader-archive member the original's only numeric score table lives in
/// (measured: one reader carries it, the installation's global `zbd/zrdr.zbd`).
pub const SCORE_CONFIG_MEMBER: &str = "player.zrd";

/// The measured prefix of every key of that table (`score_kill`,
/// `score_return_flag`, `score_suicide`, `score_zep`, `score_enemy_flag`).
pub const SCORE_KEY_PREFIX: &str = "score_";

/// The keys an objective record or objective block would carry **if** the
/// original paid a reward for completing it.
///
/// Measured: none of them occurs anywhere in the installation's objective
/// records or objective blocks. The list is a declared search vocabulary, not a
/// claim that these are the spellings the original would have used.
pub const REWARD_KEY_VOCABULARY: [&str; 16] = [
    "fame",
    "cash",
    "money",
    "score",
    "reward",
    "bonus",
    "payout",
    "pay",
    "prize",
    "points",
    "credit",
    "credits",
    "award",
    "medal",
    "achievement",
    "unlock",
];

/// The keys an objective record or objective block would carry **if** the
/// original stated whether a completion may repeat.
///
/// Measured: none of them occurs anywhere in the installation's objective
/// records or objective blocks. The original's only count-like field on a
/// stunt block is [`OBJECTIVE_DANGER_ZONE_COUNT_KEY`], which says how many of
/// the listed zones complete the objective — not how many times it may pay.
pub const REPEAT_KEY_VOCABULARY: [&str; 10] = [
    "repeat",
    "repeatable",
    "repeat_count",
    "once",
    "one_time",
    "one-time",
    "recurring",
    "respawn",
    "reset",
    "reset_count",
];

/// The keys of `keys` that exactly equal an entry of `vocabulary`, in `keys`
/// order.
///
/// Exact matching on purpose: a substring scan would read
/// `DANGER_ZONES_COMPLETION_COUNT` as a repeat count, which is exactly the
/// false positive a payout census must not have.
#[must_use]
pub fn vocabulary_keys<'a>(keys: &'a [(String, u32)], vocabulary: &[&str]) -> Vec<&'a str> {
    keys.iter()
        .map(|(key, _)| key.as_str())
        .filter(|key| vocabulary.contains(key))
        .collect()
}

/// One numeric entry of the measured `score_*` table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScoreEntry {
    key: String,
    raw: u32,
}

impl ScoreEntry {
    /// Assembles one entry from its measured key and stored word.
    #[must_use]
    pub fn new(key: impl Into<String>, raw: u32) -> Self {
        Self {
            key: key.into(),
            raw,
        }
    }

    /// The table key (`score_kill`).
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The stored word, exactly as the record holds it.
    #[must_use]
    pub const fn raw(&self) -> u32 {
        self.raw
    }

    /// The same word read as a signed 32-bit score.
    ///
    /// The stored word is unsigned; `score_suicide` is the only entry whose
    /// signed reading is negative, so this is a documented reinterpretation of
    /// the measured word, not a decoded schema.
    #[must_use]
    pub const fn signed(&self) -> i32 {
        self.raw as i32
    }
}

/// The measured `score_*` entries of one `.zrd` record (the global `player.zrd`).
///
/// The real member wraps its record in a one-element list, so the document is
/// unwrapped with [`objective_record`] exactly as `objectives.zrd` is. A record
/// that carries no `score_*` key yields an empty list, never a zero-filled
/// table: the absence of a payout is a measurement, not a default.
#[must_use]
pub fn score_entries(document: &ZrdValue) -> Vec<ScoreEntry> {
    zrd_flat_fields(objective_record(document))
        .into_iter()
        .filter(|(key, _)| key.starts_with(SCORE_KEY_PREFIX))
        .filter_map(|(key, value)| {
            zrd_wrapped_int(value).map(|raw| ScoreEntry::new(key.to_owned(), raw))
        })
        .collect()
}

/// The measured `score_*` table of one reader, with the provenance of the
/// member its bytes came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetailScoreTable {
    entries: Vec<ScoreEntry>,
    span: StuntEncodingSpan,
}

impl RetailScoreTable {
    /// Assembles one table row.
    #[must_use]
    pub fn new(entries: Vec<ScoreEntry>, span: StuntEncodingSpan) -> Self {
        Self { entries, span }
    }

    /// The measured entries, in authored order.
    #[must_use]
    pub fn entries(&self) -> &[ScoreEntry] {
        &self.entries
    }

    /// How many entries the table declares.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the table declares no `score_*` entry at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The table keys, in authored order.
    #[must_use]
    pub fn keys(&self) -> Vec<&str> {
        self.entries.iter().map(ScoreEntry::key).collect()
    }

    /// The stored word of `key`, when the table declares it.
    #[must_use]
    pub fn raw(&self, key: &str) -> Option<u32> {
        self.entries
            .iter()
            .find(|entry| entry.key == key)
            .map(ScoreEntry::raw)
    }

    /// Where the member's bytes are.
    #[must_use]
    pub const fn span(&self) -> &StuntEncodingSpan {
        &self.span
    }
}

/// One reader archive's measured payout/repeat surface.
///
/// A member is [`None`] when the reader carries none: only the installation's
/// global reader carries `player.zrd`, and not every reader carries objectives.
/// That is a measured absence, not a skipped row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetailRewardRow {
    container: String,
    container_sha256: String,
    objectives: Option<RetailObjectiveCorpus>,
    machine: Option<RetailObjectiveMachine>,
    score: Option<RetailScoreTable>,
}

impl RetailRewardRow {
    /// Assembles one row.
    #[must_use]
    pub fn new(
        container: impl Into<String>,
        container_sha256: impl Into<String>,
        objectives: Option<RetailObjectiveCorpus>,
        machine: Option<RetailObjectiveMachine>,
        score: Option<RetailScoreTable>,
    ) -> Self {
        Self {
            container: container.into(),
            container_sha256: container_sha256.into(),
            objectives,
            machine,
            score,
        }
    }

    /// The reader archive's logical key.
    #[must_use]
    pub fn container(&self) -> &str {
        &self.container
    }

    /// SHA-256 of that whole container, from production discovery.
    #[must_use]
    pub fn container_sha256(&self) -> &str {
        &self.container_sha256
    }

    /// The reader's objective records, when it declares any.
    #[must_use]
    pub const fn objectives(&self) -> Option<&RetailObjectiveCorpus> {
        self.objectives.as_ref()
    }

    /// The reader's objective state machine, when it declares one.
    #[must_use]
    pub const fn machine(&self) -> Option<&RetailObjectiveMachine> {
        self.machine.as_ref()
    }

    /// The reader's `score_*` table, when it carries one.
    #[must_use]
    pub const fn score(&self) -> Option<&RetailScoreTable> {
        self.score.as_ref()
    }
}

/// The measured payout/repeat surface of a whole installation (task #464).
///
/// One row per reader archive. The survey reports what the objective data
/// **contains** — its complete key vocabularies, the complete key inventory of
/// every stunt completion block, and the only numeric score table the
/// installation carries — and answers the rule itself with
/// [`Self::reward_is_measured`] and [`Self::repeat_is_measured`], which are
/// `false`: no measured file records what a stunt paid or whether it paid
/// again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetailStuntRewardSurvey {
    install_sha256: String,
    rows: Vec<RetailRewardRow>,
}

impl RetailStuntRewardSurvey {
    /// Assembles the survey. Rows are keyed by their own container, so a
    /// duplicate is impossible and no validation is needed.
    #[must_use]
    pub fn new(install_sha256: impl Into<String>, rows: Vec<RetailRewardRow>) -> Self {
        Self {
            install_sha256: install_sha256.into(),
            rows,
        }
    }

    /// The installation fingerprint the measurement was taken over.
    #[must_use]
    pub fn install_sha256(&self) -> &str {
        &self.install_sha256
    }

    /// Every measured reader archive, in inventory order.
    #[must_use]
    pub fn rows(&self) -> &[RetailRewardRow] {
        &self.rows
    }

    /// How many reader archives were walked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether no reader archive was walked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Every measured objective record.
    #[must_use]
    pub fn objective_records(&self) -> u32 {
        self.rows
            .iter()
            .filter_map(|row| row.objectives())
            .map(RetailObjectiveCorpus::count)
            .sum()
    }

    /// Every measured numbered objective block.
    #[must_use]
    pub fn objective_blocks(&self) -> u32 {
        self.rows
            .iter()
            .filter_map(|row| row.machine())
            .map(|machine| machine.machine().blocks())
            .sum()
    }

    /// The complete key vocabulary of the objective records **and** of the
    /// objective blocks, summed over every row and sorted by key.
    #[must_use]
    pub fn objective_keys(&self) -> Vec<(String, u32)> {
        let mut counts: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
        for row in &self.rows {
            if let Some(corpus) = row.objectives() {
                for (key, count) in &corpus.keys {
                    *counts.entry(key.clone()).or_insert(0) += count;
                }
            }
            if let Some(machine) = row.machine() {
                for (key, count) in machine.machine().keys() {
                    *counts.entry(key.clone()).or_insert(0) += count;
                }
            }
        }
        counts.into_iter().collect()
    }

    /// Every `(container, key)` whose key names a payout, per
    /// [`REWARD_KEY_VOCABULARY`], in row order.
    ///
    /// Measured over the owner's installation: **empty**. That is a statement
    /// about the objective data's vocabulary, not about what the game paid.
    #[must_use]
    pub fn reward_keys(&self) -> Vec<(String, String)> {
        self.vocabulary_hits(&REWARD_KEY_VOCABULARY)
    }

    /// Every `(container, key)` whose key names a repeat policy, per
    /// [`REPEAT_KEY_VOCABULARY`], in row order.
    ///
    /// Measured over the owner's installation: **empty**.
    #[must_use]
    pub fn repeat_keys(&self) -> Vec<(String, String)> {
        self.vocabulary_hits(&REPEAT_KEY_VOCABULARY)
    }

    /// The `(container, key)` pairs of every objective key that exactly equals
    /// an entry of `vocabulary`, over both the record and the machine surface.
    fn vocabulary_hits(&self, vocabulary: &[&str]) -> Vec<(String, String)> {
        let mut found = Vec::new();
        for row in &self.rows {
            if let Some(corpus) = row.objectives() {
                for key in vocabulary_keys(&corpus.keys, vocabulary) {
                    found.push((row.container.clone(), key.to_owned()));
                }
            }
            if let Some(machine) = row.machine() {
                for key in vocabulary_keys(&machine.machine().keys, vocabulary) {
                    found.push((row.container.clone(), key.to_owned()));
                }
            }
        }
        found
    }

    /// Every measured stunt completion condition, in row then authored order.
    pub fn stunt_conditions(&self) -> impl Iterator<Item = &StuntCompletionCondition> {
        self.rows
            .iter()
            .filter_map(|row| row.machine())
            .flat_map(|machine| machine.machine().stunt_conditions())
    }

    /// The complete key vocabulary of every measured stunt completion block,
    /// summed over the installation and sorted by key.
    ///
    /// This is the closed surface a payout or a repeat policy would have to
    /// appear on: every field of every block that carries
    /// [`OBJECTIVE_DANGER_ZONES_KEY`].
    #[must_use]
    pub fn stunt_block_keys(&self) -> Vec<(String, u32)> {
        let mut counts: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
        for condition in self.stunt_conditions() {
            for (key, count) in condition.keys() {
                *counts.entry(key.clone()).or_insert(0) += count;
            }
        }
        counts.into_iter().collect()
    }

    /// Every measured `score_*` table, in row order.
    pub fn score_tables(&self) -> impl Iterator<Item = &RetailScoreTable> {
        self.rows.iter().filter_map(|row| row.score())
    }

    /// Every measured score entry with its container, in row then authored
    /// order.
    pub fn score_entries(&self) -> impl Iterator<Item = (&str, &ScoreEntry)> {
        self.rows
            .iter()
            .filter_map(|row| row.score().map(|table| (row.container.as_str(), table)))
            .flat_map(|(container, table)| {
                table.entries().iter().map(move |entry| (container, entry))
            })
    }

    /// Whether this survey measured **what a stunt pays**. It did not, and it
    /// cannot: the objective data names no payout for a completion.
    ///
    /// A consumer must treat the *contents* of this survey (the key
    /// inventories, the stunt-block surface and the score table) as what was
    /// measured, and any payout as still unknown until an original run settles
    /// it.
    #[must_use]
    pub const fn reward_is_measured(&self) -> bool {
        false
    }

    /// Whether this survey measured **whether a repeat traversal pays again**.
    /// It did not, for the same reason as [`Self::reward_is_measured`].
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
