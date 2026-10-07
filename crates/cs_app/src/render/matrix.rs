//! The original-data **comparison matrix** and its material coverage
//! (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
//! stage `### F17-D`; shared contract `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! AC04 of the sheet is *"Original comparison set includes cockpit, skyline,
//! vegetation, night effects and close-up aircraft"*. This module is that set:
//! five named subjects, each resolved to **one stored mesh of the owner's
//! original installation** through the production GameZ readers, each drawn by
//! the production upload adapter, and each carrying the coverage table of the
//! materials that mesh references.
//!
//! # The five subjects, and what each one is anchored to
//!
//! A subject is selected in two steps and neither step is optional:
//!
//! 1. [`select`] finds the subject's **anchor** in the stored node forest — a
//!    stored name measured on this installation, never a search that accepts
//!    whatever it finds — and collects the anchor's mesh-bound descendants.
//!    A missing anchor is [`MatrixError::AnchorAbsent`], never a subject
//!    silently left out of the set.
//! 2. [`load`] ranks those candidates, reads the winner through
//!    [`RenderMesh::from_stored_groups`] and counts the coverage of the
//!    stored material records it references. Every candidate that was passed
//!    over is reported in [`ResolvedSubject::skipped`] with the reason, so a
//!    subject whose only geometry is refused fails with that refusal in hand
//!    instead of producing a blank frame.
//!
//! | subject | side | anchor (stored name) | measured on this installation |
//! | --- | --- | --- | --- |
//! | `cockpit` | aircraft | `cockpit1` under the first parentless airframe root that carries one | `player_bhawk`/`cockpit1`, 67 mesh-bound descendants |
//! | `skyline` | world | the world record's child `horizon` | `ZBD/C1C` slot 789, 5 mesh-bound descendants |
//! | `vegetation` | world | the world record's child family whose stored name repeats | one repeated name, 30 instances; first instance has 35 mesh-bound descendants |
//! | `night_effects` | world | the world's night-sky nodes, named `moon` or `stars` | slots 791 and 793; `stars` stores no position and is skipped by name |
//! | `close_up_aircraft` | aircraft | `healthy` under the root `bloodhawk` | `bloodhawk`/`healthy`, 29 mesh-bound descendants |
//!
//! **The ranking rule is one rule for every subject:** among the candidates
//! whose stored mesh holds at least one position and one polygon and builds
//! through the production adapter, the one that draws the most triangles
//! wins, ties broken by the lowest stored slot. Measured on this
//! installation, that picks the horizon subtree's most detailed mesh for
//! `skyline` (17 488 stored units wide), the cockpit assembly for `cockpit`
//! (231 triangles, six material groups), the first billboard of the
//! vegetation instance for `vegetation` (every billboard is a two-triangle
//! quad, so the tie rule decides), the `moon` for `night_effects` (its
//! sibling `stars` stores no geometry at all) and the most detailed part of
//! the intact airframe for `close_up_aircraft` (251 triangles, seven
//! material groups). The rule is declared once, it is a *drawn-geometry*
//! rule rather than an extent one — a needle-shaped mesh cannot win by being
//! long — and it is re-derived from the installation on every run: nothing
//! here pins a mesh-array index.
//!
//! # What a row of this matrix is, and is not
//!
//! A resolved row is: **this original mesh, uploaded through the production
//! adapter, drawn on a real GPU adapter, with the stored material records it
//! references classified through [`crate::render::material::classify`].** The
//! capture uses `crate::world::gpu_capture`'s declared flat material and key
//! light, so a row is evidence that the original geometry is presentable —
//! not a claim about the original's textures, lighting or colours, and not a
//! comparison against an original screenshot. No original run has happened;
//! that side of the comparison stays with `#358 REF-OWNER-FIRST-CAPTURE`.
//!
//! # The material-coverage half
//!
//! [`MaterialCoverage`] counts, for one subject's mesh, how many of its
//! stored material records establish a render class and how many do not,
//! with every refusal reason grouped by [`ClassificationFailure::code`]. A
//! stored GameZ record alone asserts no class
//! ([`MaterialFacts::for_raw_record`]), so on original data the measured
//! table is *every material unclassified, reason `undeclared`*: that is this
//! stage's finding, recorded rather than defaulted. Nothing here ever turns
//! an unclassified material into an opaque one.

use std::collections::BTreeMap;
use std::fmt;

use cs_content::mesh::{MeshPresentationUnknown, RenderMesh};
use cs_formats::gamez::nodes::{NodeKind, RawNode};
use cs_formats::gamez::{GameZMaterials, GameZMesh, GameZMeshes, GameZNodes};

use crate::render::material::{Classification, MaterialClass, MaterialFacts, classify};
use crate::world::retail::stored_presentation_unknowns;

/// The one world group the matrix's world-side subjects are read from.
///
/// A declared pin, the way the retail playtest pins its area: AC04 requires
/// the five *subjects*, and one documented world container carries all three
/// world-side anchors on this installation.
pub const MATRIX_WORLD_GROUP: &str = "C1C";

/// The stored name of the world's horizon child (`skyline`'s anchor).
pub const HORIZON_ANCHOR: &str = "horizon";

/// The stored name of an airframe's intact state (`close_up_aircraft`'s
/// anchor child).
pub const INTACT_ANCHOR: &str = "healthy";

/// The stored name of the shared airframe container's root
/// (`close_up_aircraft`'s anchor).
pub const AIRFRAME_ANCHOR: &str = "bloodhawk";

/// The stored name of the cockpit subtree (`cockpit`'s anchor).
pub const COCKPIT_ANCHOR: &str = "cockpit1";

/// The stored names that are the world's night-sky objects
/// (`night_effects`'s anchors).
pub const NIGHT_ANCHORS: [&str; 2] = ["moon", "stars"];

/// The five subjects AC04 names, in the order the sheet names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComparisonSubject {
    /// The original's cockpit interior: the `cockpit1` subtree of an airframe.
    Cockpit,
    /// The original's horizon: the world's `horizon` subtree.
    Skyline,
    /// The original's instanced vegetation children of the world record.
    Vegetation,
    /// The original's night-sky objects, `moon` and `stars`.
    NightEffects,
    /// A close view of one original airframe's intact state.
    CloseUpAircraft,
}

impl ComparisonSubject {
    /// AC04's five subjects, in the order the sheet names them.
    pub const ALL: [ComparisonSubject; 5] = [
        ComparisonSubject::Cockpit,
        ComparisonSubject::Skyline,
        ComparisonSubject::Vegetation,
        ComparisonSubject::NightEffects,
        ComparisonSubject::CloseUpAircraft,
    ];

    /// Stable lowercase identifier, one word per subject.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Cockpit => "cockpit",
            Self::Skyline => "skyline",
            Self::Vegetation => "vegetation",
            Self::NightEffects => "night_effects",
            Self::CloseUpAircraft => "close_up_aircraft",
        }
    }

    /// Position in [`Self::ALL`], also the coverage table's bucket index.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Cockpit => 0,
            Self::Skyline => 1,
            Self::Vegetation => 2,
            Self::NightEffects => 3,
            Self::CloseUpAircraft => 4,
        }
    }

    /// Which of the two containers the subject is anchored in.
    #[must_use]
    pub const fn side(self) -> SubjectSide {
        match self {
            Self::Cockpit | Self::CloseUpAircraft => SubjectSide::Aircraft,
            Self::Skyline | Self::Vegetation | Self::NightEffects => SubjectSide::World,
        }
    }
}

impl fmt::Display for ComparisonSubject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Which of the matrix's two containers a subject reads from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SubjectSide {
    /// The world container (`ZBD/<group>/gamez.zbd`).
    World,
    /// The shared airframe container (`ZBD/planes.zbd`).
    Aircraft,
}

/// One stored node as the selectors see it.
///
/// A projection of [`RawNode`] carrying exactly the facts a selection reads,
/// so a selector is a pure function over plain data and its failure cases can
/// be exercised without a parsed container. Production converts with
/// [`MatrixContainer::subject_nodes`]; the conversion interprets nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubjectNode {
    /// The node's slot in the stored info array.
    pub slot: u32,
    /// The stored display name, up to its first NUL.
    pub name: String,
    /// The parent slot, when the record declares one.
    pub parent: Option<u32>,
    /// The child slots, in stored order.
    pub children: Vec<u32>,
    /// The stored signed mesh index; `-1` means "no mesh".
    pub mesh_index: i32,
    /// Whether this record is the container's world record.
    pub is_world_record: bool,
}

impl From<&RawNode> for SubjectNode {
    fn from(node: &RawNode) -> Self {
        Self {
            slot: node.index,
            name: node.name.clone(),
            parent: node.parent,
            children: node.children.clone(),
            mesh_index: node.mesh_index(),
            is_world_record: matches!(node.kind, NodeKind::World(_)),
        }
    }
}

/// A candidate mesh of one subject: a mesh-bound node of the anchor's set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateNode {
    /// The node's slot.
    pub slot: u32,
    /// The node's stored name.
    pub name: String,
    /// The mesh-array slot the node associates.
    pub mesh_index: u32,
}

/// What [`select`] found for one subject: the anchor it matched and the
/// mesh-bound candidates underneath it, in stored order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubjectSelection {
    /// The subject being selected.
    pub subject: ComparisonSubject,
    /// The container's logical key, carried from the caller.
    pub container: String,
    /// The anchor node's slot.
    pub anchor_slot: u32,
    /// The anchor node's stored name.
    pub anchor_name: String,
    /// The mesh-bound candidates, in stored order.
    pub candidates: Vec<CandidateNode>,
}

/// Why a subject could not be selected from a stored node forest.
///
/// Every variant is a *structural* refusal: the original bytes did not hold
/// what the subject's rule names. Nothing here is a default, and nothing
/// leaves a subject out of the set — a selection failure becomes a
/// [`MatrixRow::Unresolved`] row carrying this value's message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MatrixError {
    /// The container's world record is missing, or several are stored.
    NoWorldRecord {
        /// The container's logical key.
        container: String,
        /// How many world records the container stored.
        found: usize,
    },
    /// The subject's anchor name is not where the rule looks.
    AnchorAbsent {
        /// The subject whose rule failed.
        subject: ComparisonSubject,
        /// The container's logical key.
        container: String,
        /// The stored name(s) the rule looked for.
        expected: String,
        /// What the rule searched, for a readable message.
        within: String,
    },
    /// The anchor matched more than one node where the rule allows one.
    AmbiguousAnchor {
        /// The subject whose rule failed.
        subject: ComparisonSubject,
        /// The container's logical key.
        container: String,
        /// The stored name that matched.
        expected: String,
        /// How many nodes matched.
        found: usize,
    },
    /// No child of the world record shares its name with another child, so
    /// there is no instanced family to select the vegetation from.
    InstancedChildrenAbsent {
        /// The container's logical key.
        container: String,
    },
    /// More than one child name is repeated, so "the instanced family" does
    /// not name one.
    AmbiguousInstancedChildren {
        /// The container's logical key.
        container: String,
        /// The repeated stored names, in first-appearance order.
        names: Vec<String>,
    },
    /// The anchor has no mesh-bound descendant at all.
    NoMeshBoundNode {
        /// The subject whose rule failed.
        subject: ComparisonSubject,
        /// The container's logical key.
        container: String,
        /// The anchor's stored name.
        anchor: String,
    },
    /// A candidate names a mesh-array slot the container does not hold.
    MeshAbsent {
        /// The subject being loaded.
        subject: ComparisonSubject,
        /// The container's logical key.
        container: String,
        /// The mesh-array slot that is absent.
        mesh_index: u32,
    },
    /// Every candidate was refused, each for its own reason.
    NoDrawableCandidate {
        /// The subject being loaded.
        subject: ComparisonSubject,
        /// The container's logical key.
        container: String,
        /// How many candidates were refused.
        refusals: usize,
    },
    /// A mesh references a material record the container's table does not
    /// hold, so its coverage cannot be counted.
    MaterialAbsent {
        /// The subject being loaded.
        subject: ComparisonSubject,
        /// The container's logical key.
        container: String,
        /// The material index the mesh referenced.
        material_index: u32,
    },
    /// The matrix has no row for a required subject.
    MissingSubject {
        /// The subject that is missing.
        subject: ComparisonSubject,
    },
    /// The matrix has more than one row for a subject.
    DuplicateSubject {
        /// The subject that appears twice.
        subject: ComparisonSubject,
    },
}

impl fmt::Display for MatrixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoWorldRecord { container, found } => write!(
                f,
                "{container} stored {found} world record(s), so the world-side anchors have \
                 nothing to hang on"
            ),
            Self::AnchorAbsent {
                subject,
                container,
                expected,
                within,
            } => write!(
                f,
                "the {subject} subject's anchor {expected:?} is absent from {within} in {container}"
            ),
            Self::AmbiguousAnchor {
                subject,
                container,
                expected,
                found,
            } => write!(
                f,
                "the {subject} subject's anchor {expected:?} matched {found} nodes in {container}; \
                 the rule allows one"
            ),
            Self::InstancedChildrenAbsent { container } => write!(
                f,
                "no child of {container}'s world record shares its stored name with another child, \
                 so no instanced vegetation family exists to select"
            ),
            Self::AmbiguousInstancedChildren { container, names } => write!(
                f,
                "{container}'s world record has more than one repeated child name ({}); the \
                 vegetation rule must name exactly one",
                names.join(", ")
            ),
            Self::NoMeshBoundNode {
                subject,
                container,
                anchor,
            } => write!(
                f,
                "the {subject} subject's anchor {anchor:?} in {container} has no mesh-bound \
                 descendant, so there is nothing to draw"
            ),
            Self::MeshAbsent {
                subject,
                container,
                mesh_index,
            } => write!(
                f,
                "the {subject} subject of {container} names mesh slot {mesh_index}, which the \
                 container does not hold"
            ),
            Self::NoDrawableCandidate {
                subject,
                container,
                refusals,
            } => write!(
                f,
                "none of the {subject} subject's {refusals} candidate(s) in {container} could be \
                 drawn: a subject nobody can draw is not evidence"
            ),
            Self::MaterialAbsent {
                subject,
                container,
                material_index,
            } => write!(
                f,
                "the {subject} subject's mesh in {container} references material {material_index}, \
                 which the container's material table does not hold"
            ),
            Self::MissingSubject { subject } => {
                write!(f, "the comparison set has no row for {subject}")
            }
            Self::DuplicateSubject { subject } => {
                write!(f, "the comparison set has more than one row for {subject}")
            }
        }
    }
}

impl std::error::Error for MatrixError {}

/// Why one candidate of a subject was not the mesh that got drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateSkip {
    /// The candidate's node slot.
    pub slot: u32,
    /// The candidate's stored name.
    pub name: String,
    /// The refusal, in readable form.
    pub reason: String,
}

/// One subject's mesh, its selection, and the material coverage of the stored
/// records that mesh references.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedSubject {
    /// Which subject this row is.
    pub subject: ComparisonSubject,
    /// The container's logical key the mesh came from.
    pub container: String,
    /// The anchor slot the selection matched.
    pub anchor_slot: u32,
    /// The anchor's stored name.
    pub anchor_name: String,
    /// The candidate that was drawn.
    pub chosen: CandidateNode,
    /// Candidates that could not be drawn, each with the reason.
    pub skipped: Vec<CandidateSkip>,
    /// Triangles the production render mesh holds for it.
    pub triangles: usize,
    /// The stored bounding box of the drawn mesh, in stored units. The stored
    /// unit itself is unmeasured (task #436), so this is never a length
    /// claim.
    pub extent: [f32; 3],
    /// The presentation open questions the production retail path reports for
    /// a stored mesh
    /// ([`crate::world::retail::stored_presentation_unknowns`]), handed to the
    /// upload adapter unchanged.
    pub unknowns: Vec<MeshPresentationUnknown>,
    /// The coverage of every stored material record the mesh references.
    pub coverage: MaterialCoverage,
    /// The production render mesh the adapter draws.
    pub render: RenderMesh,
}

/// How many of one mesh's stored materials establish a render class, and why
/// the rest do not.
///
/// The counts always add up: [`Self::total`] is the number of stored material
/// records the mesh references, and every one lands in exactly one class
/// bucket or in [`Self::unclassified`] with at least one
/// [`ClassificationFailure`](crate::render::material::ClassificationFailure)
/// reason. A material whose facts establish nothing is **never** counted as
/// opaque (spec F17 non-negotiable 1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MaterialCoverage {
    total: usize,
    classified: [usize; 5],
    unclassified: usize,
    reasons: BTreeMap<&'static str, usize>,
}

impl MaterialCoverage {
    /// Classifies every fact of one mesh's materials and counts the verdicts.
    ///
    /// One [`MaterialFacts`] per stored material record, in the order the mesh
    /// references them.
    #[must_use]
    pub fn of(facts: impl IntoIterator<Item = MaterialFacts>) -> Self {
        let mut coverage = Self::default();
        for facts in facts {
            coverage.total += 1;
            match classify(&facts) {
                Classification::Classified(material) => {
                    coverage.classified[index_of(material.class())] += 1;
                }
                Classification::Unclassified { reasons } => {
                    coverage.unclassified += 1;
                    for reason in reasons {
                        *coverage.reasons.entry(reason.code()).or_insert(0) += 1;
                    }
                }
            }
        }
        coverage
    }

    /// How many stored material records were counted.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.total
    }

    /// How many established the given class.
    #[must_use]
    pub fn count(&self, class: MaterialClass) -> usize {
        self.classified[index_of(class)]
    }

    /// How many established some class.
    #[must_use]
    pub fn classified(&self) -> usize {
        self.classified.iter().sum()
    }

    /// How many established none, each with at least one reason.
    #[must_use]
    pub const fn unclassified(&self) -> usize {
        self.unclassified
    }

    /// The refusal reasons, keyed by their stable code and sorted by key.
    #[must_use]
    pub fn reasons(&self) -> &BTreeMap<&'static str, usize> {
        &self.reasons
    }

    /// Whether every counted material is accounted for exactly once.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.total == self.classified() + self.unclassified
    }

    /// Whether no material of this subject establishes a render class, and at
    /// least one material was counted.
    ///
    /// This is the state the stage measures on original data: a stored record
    /// alone asserts no class, so a matrix row over original content reports
    /// every material unclassified rather than picking one. An empty coverage
    /// measured nothing and is never "fully unclassified".
    #[must_use]
    pub fn is_fully_unclassified(&self) -> bool {
        self.total > 0 && self.total == self.unclassified && self.classified() == 0
    }
}

/// Index of one class in the coverage table's fixed bucket order.
const fn index_of(class: MaterialClass) -> usize {
    match class {
        MaterialClass::Opaque => 0,
        MaterialClass::Masked => 1,
        MaterialClass::Blended => 2,
        MaterialClass::Additive => 3,
        MaterialClass::Emissive => 4,
    }
}

/// One subject's row of the comparison set: either the mesh that was drawn
/// with its coverage, or the refusal that explains why there is no mesh.
///
/// An unresolved row is **kept**. The subject is still part of the set — AC04
/// asks that the set *include* it — and dropping it would turn a gap into an
/// absence, which is the failure this type exists to prevent.
#[derive(Clone, Debug, PartialEq)]
pub enum MatrixRow {
    /// The subject resolved to original content and was read.
    Resolved(Box<ResolvedSubject>),
    /// The subject's rule found nothing it may draw, with the reason.
    Unresolved {
        /// Which subject failed to resolve.
        subject: ComparisonSubject,
        /// The refusal, verbatim.
        reason: String,
    },
}

impl MatrixRow {
    /// The subject this row is for.
    #[must_use]
    pub fn subject(&self) -> ComparisonSubject {
        match self {
            Self::Resolved(resolved) => resolved.subject,
            Self::Unresolved { subject, .. } => *subject,
        }
    }

    /// The resolved subject, when the row has one.
    #[must_use]
    pub fn resolved(&self) -> Option<&ResolvedSubject> {
        match self {
            Self::Resolved(resolved) => Some(resolved.as_ref()),
            Self::Unresolved { .. } => None,
        }
    }

    /// The refusal reason, when the row is unresolved.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Resolved(_) => None,
            Self::Unresolved { reason, .. } => Some(reason),
        }
    }

    /// An unresolved row for a subject whose rule failed.
    #[must_use]
    pub fn unresolved(subject: ComparisonSubject, error: MatrixError) -> Self {
        Self::Unresolved {
            subject,
            reason: error.to_string(),
        }
    }
}

/// The comparison set: exactly one row for each of [`ComparisonSubject::ALL`].
///
/// Built only through [`ComparisonMatrix::build`], which refuses a set that is
/// missing a required subject or carries one twice, so "the set includes
/// cockpit, skyline, vegetation, night effects and close-up aircraft" is a
/// property of the value rather than of a caller's diligence.
#[derive(Clone, Debug, PartialEq)]
pub struct ComparisonMatrix {
    rows: Vec<MatrixRow>,
}

impl ComparisonMatrix {
    /// Builds the set from its rows, checking the subject list.
    ///
    /// # Errors
    ///
    /// [`MatrixError::MissingSubject`] when a required subject has no row and
    /// [`MatrixError::DuplicateSubject`] when one has two. Unresolved rows are
    /// accepted: a gap is reported, not refused.
    pub fn build(rows: Vec<MatrixRow>) -> Result<Self, MatrixError> {
        let mut seen = [false; 5];
        for row in &rows {
            let index = row.subject().index();
            if seen[index] {
                return Err(MatrixError::DuplicateSubject {
                    subject: row.subject(),
                });
            }
            seen[index] = true;
        }
        for subject in ComparisonSubject::ALL {
            if !seen[subject.index()] {
                return Err(MatrixError::MissingSubject { subject });
            }
        }
        Ok(Self { rows })
    }

    /// The rows, in the order they were given.
    #[must_use]
    pub fn rows(&self) -> &[MatrixRow] {
        &self.rows
    }

    /// One subject's row.
    #[must_use]
    pub fn row(&self, subject: ComparisonSubject) -> Option<&MatrixRow> {
        self.rows.iter().find(|row| row.subject() == subject)
    }

    /// How many subjects resolved to drawable original content.
    #[must_use]
    pub fn resolved_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| matches!(row, MatrixRow::Resolved(_)))
            .count()
    }

    /// Whether every subject of the set resolved.
    #[must_use]
    pub fn is_fully_resolved(&self) -> bool {
        self.resolved_count() == ComparisonSubject::ALL.len()
    }
}

/// The two parsed containers the matrix reads, borrowed from the caller.
///
/// The module takes the readers' own values rather than opening files, so
/// production discovery, the digests and the parse budget stay where they
/// already are (the retail playtest's [`crate::playtest_retail`]) and this
/// module adds no file access of its own.
pub struct MatrixContainer<'a> {
    /// The container's logical key, e.g. `zbd/c1c/gamez.zbd`.
    pub key: &'a str,
    /// The decoded node array.
    pub nodes: &'a GameZNodes,
    /// The decoded mesh array.
    pub meshes: &'a GameZMeshes,
    /// The decoded material records.
    pub materials: &'a GameZMaterials,
}

impl<'a> MatrixContainer<'a> {
    /// Borrows the three sections a subject needs.
    #[must_use]
    pub const fn new(
        key: &'a str,
        nodes: &'a GameZNodes,
        meshes: &'a GameZMeshes,
        materials: &'a GameZMaterials,
    ) -> Self {
        Self {
            key,
            nodes,
            meshes,
            materials,
        }
    }

    /// The node array projected to the facts the selectors read.
    #[must_use]
    pub fn subject_nodes(&self) -> Vec<SubjectNode> {
        self.nodes.nodes.iter().map(SubjectNode::from).collect()
    }
}

/// Finds a subject's anchor and collects its mesh-bound candidates.
///
/// Pure over [`SubjectNode`]s: the caller passes one container's projected
/// forest and the container's own key, which every refusal names.
///
/// # Errors
///
/// One of the structural refusals of [`MatrixError`]: no world record, an
/// absent or ambiguous anchor, no instanced family for `vegetation`, or an
/// anchor with no mesh-bound descendant.
pub fn select(
    subject: ComparisonSubject,
    container: &str,
    nodes: &[SubjectNode],
) -> Result<SubjectSelection, MatrixError> {
    let anchor = match subject {
        ComparisonSubject::Cockpit => cockpit_anchor(subject, container, nodes)?,
        ComparisonSubject::CloseUpAircraft => intact_airframe_anchor(subject, container, nodes)?,
        ComparisonSubject::Skyline => named_child_of_world(
            subject,
            container,
            nodes,
            HORIZON_ANCHOR,
            "the world record's stored child list",
        )?,
        ComparisonSubject::Vegetation => vegetation_anchor(container, nodes)?,
        ComparisonSubject::NightEffects => night_anchor(subject, container, nodes)?,
    };

    let candidates = candidates(subject, nodes, anchor);
    if candidates.is_empty() {
        return Err(MatrixError::NoMeshBoundNode {
            subject,
            container: container.to_owned(),
            anchor: anchor.name.clone(),
        });
    }
    Ok(SubjectSelection {
        subject,
        container: container.to_owned(),
        anchor_slot: anchor.slot,
        anchor_name: anchor.name.clone(),
        candidates,
    })
}

/// The world record, of which there must be exactly one.
fn world_record<'a>(
    container: &str,
    nodes: &'a [SubjectNode],
) -> Result<&'a SubjectNode, MatrixError> {
    let found: Vec<&SubjectNode> = nodes.iter().filter(|node| node.is_world_record).collect();
    match found.as_slice() {
        [only] => Ok(only),
        other => Err(MatrixError::NoWorldRecord {
            container: container.to_owned(),
            found: other.len(),
        }),
    }
}

/// The one child of `parent` whose stored name is `name`.
fn child_named<'a>(
    subject: ComparisonSubject,
    container: &str,
    nodes: &'a [SubjectNode],
    parent: &SubjectNode,
    name: &str,
    within: &str,
) -> Result<&'a SubjectNode, MatrixError> {
    let matches: Vec<&SubjectNode> = parent
        .children
        .iter()
        .filter_map(|slot| nodes.get(*slot as usize))
        .filter(|node| node.name == name)
        .collect();
    match matches.as_slice() {
        [only] => Ok(only),
        [] => Err(MatrixError::AnchorAbsent {
            subject,
            container: container.to_owned(),
            expected: name.to_owned(),
            within: within.to_owned(),
        }),
        many => Err(MatrixError::AmbiguousAnchor {
            subject,
            container: container.to_owned(),
            expected: name.to_owned(),
            found: many.len(),
        }),
    }
}

/// `skyline`: the world record's child named `horizon`.
fn named_child_of_world<'a>(
    subject: ComparisonSubject,
    container: &str,
    nodes: &'a [SubjectNode],
    name: &str,
    within: &str,
) -> Result<&'a SubjectNode, MatrixError> {
    let world = world_record(container, nodes)?;
    child_named(subject, container, nodes, world, name, within)
}

/// `cockpit`: the `cockpit1` child of the first parentless airframe root that
/// carries one.
///
/// "First" is stored order, which is stable for a given container, and the
/// rule names the *child* rather than the root because every player airframe
/// carries its own `cockpit1` and the subject is the cockpit subtree.
fn cockpit_anchor<'a>(
    subject: ComparisonSubject,
    container: &str,
    nodes: &'a [SubjectNode],
) -> Result<&'a SubjectNode, MatrixError> {
    for root in nodes.iter().filter(|node| node.parent.is_none()) {
        let matches: Vec<&SubjectNode> = root
            .children
            .iter()
            .filter_map(|slot| nodes.get(*slot as usize))
            .filter(|node| node.name == COCKPIT_ANCHOR)
            .collect();
        match matches.as_slice() {
            [only] => return Ok(only),
            [] => {}
            many => {
                return Err(MatrixError::AmbiguousAnchor {
                    subject,
                    container: container.to_owned(),
                    expected: COCKPIT_ANCHOR.to_owned(),
                    found: many.len(),
                });
            }
        }
    }
    Err(MatrixError::AnchorAbsent {
        subject,
        container: container.to_owned(),
        expected: COCKPIT_ANCHOR.to_owned(),
        within: "the parentless airframe roots' stored child lists".to_owned(),
    })
}

/// `close_up_aircraft`: the `healthy` child of the root named `bloodhawk`.
fn intact_airframe_anchor<'a>(
    subject: ComparisonSubject,
    container: &str,
    nodes: &'a [SubjectNode],
) -> Result<&'a SubjectNode, MatrixError> {
    let roots: Vec<&SubjectNode> = nodes
        .iter()
        .filter(|node| node.parent.is_none() && node.name == AIRFRAME_ANCHOR)
        .collect();
    if roots.is_empty() {
        return Err(MatrixError::AnchorAbsent {
            subject,
            container: container.to_owned(),
            expected: AIRFRAME_ANCHOR.to_owned(),
            within: "the parentless aircraft roots".to_owned(),
        });
    }
    if roots.len() > 1 {
        return Err(MatrixError::AmbiguousAnchor {
            subject,
            container: container.to_owned(),
            expected: AIRFRAME_ANCHOR.to_owned(),
            found: roots.len(),
        });
    }
    child_named(
        subject,
        container,
        nodes,
        roots[0],
        INTACT_ANCHOR,
        "the airframe root's stored child list",
    )
}

/// `vegetation`: the world record's child family whose stored name is shared
/// by more than one instance, then that family's first instance.
///
/// The rule never names a generated `g…` identifier: it selects *the
/// instanced family*, and on this installation exactly one child name is
/// repeated, so the family is unambiguous. Zero repetitions means the world
/// stores no instanced content at all and two would mean the rule no longer
/// names one family — both are refusals, never a first-match guess.
fn vegetation_anchor<'a>(
    container: &str,
    nodes: &'a [SubjectNode],
) -> Result<&'a SubjectNode, MatrixError> {
    let world = world_record(container, nodes)?;
    let mut families: Vec<(String, Vec<u32>)> = Vec::new();
    for slot in &world.children {
        let Some(node) = nodes.get(*slot as usize) else {
            continue;
        };
        match families.iter_mut().find(|(name, _)| *name == node.name) {
            Some((_, slots)) => slots.push(*slot),
            None => families.push((node.name.clone(), vec![*slot])),
        }
    }
    let repeated: Vec<&(String, Vec<u32>)> = families
        .iter()
        .filter(|(_, slots)| slots.len() > 1)
        .collect();
    match repeated.as_slice() {
        [] => Err(MatrixError::InstancedChildrenAbsent {
            container: container.to_owned(),
        }),
        [(_, slots)] => match nodes.get(slots[0] as usize) {
            Some(node) => Ok(node),
            None => Err(MatrixError::InstancedChildrenAbsent {
                container: container.to_owned(),
            }),
        },
        many => Err(MatrixError::AmbiguousInstancedChildren {
            container: container.to_owned(),
            names: many.iter().map(|(name, _)| name.clone()).collect(),
        }),
    }
}

/// `night_effects`: the world's night-sky nodes, named `moon` or `stars`.
///
/// The anchors **are** the candidates here: they are leaves of the stored
/// forest, and the ranking rule decides which of them stores something
/// drawable. Measured on this installation, `moon` stores a flat 422 × 423
/// quad and `stars` stores no position at all, so `stars` is skipped by name
/// rather than drawn blank.
fn night_anchor<'a>(
    subject: ComparisonSubject,
    container: &str,
    nodes: &'a [SubjectNode],
) -> Result<&'a SubjectNode, MatrixError> {
    let mut found: Vec<&SubjectNode> = nodes
        .iter()
        .filter(|node| NIGHT_ANCHORS.contains(&node.name.as_str()))
        .collect();
    found.sort_by_key(|node| node.slot);
    match found.first() {
        Some(only) => Ok(only),
        None => Err(MatrixError::AnchorAbsent {
            subject,
            container: container.to_owned(),
            expected: NIGHT_ANCHORS.join("|"),
            within: "the whole world node array".to_owned(),
        }),
    }
}

/// The mesh-bound nodes of one subject: every descendant of the anchor that
/// binds a mesh (the anchor included), or — for `night_effects` — every
/// night-sky node, since the anchors are leaves.
fn candidates(
    subject: ComparisonSubject,
    nodes: &[SubjectNode],
    anchor: &SubjectNode,
) -> Vec<CandidateNode> {
    let slots: Vec<u32> = if subject == ComparisonSubject::NightEffects {
        let mut night: Vec<u32> = nodes
            .iter()
            .filter(|node| NIGHT_ANCHORS.contains(&node.name.as_str()))
            .filter(|node| node.mesh_index >= 0)
            .map(|node| node.slot)
            .collect();
        night.sort_unstable();
        night
    } else {
        let mut out: Vec<u32> = Vec::new();
        let mut stack = vec![anchor.slot];
        while let Some(slot) = stack.pop() {
            if out.contains(&slot) {
                continue;
            }
            out.push(slot);
            let Some(node) = nodes.get(slot as usize) else {
                continue;
            };
            for child in node.children.iter().rev() {
                stack.push(*child);
            }
        }
        out.sort_unstable();
        out.into_iter()
            .filter(|slot| {
                nodes
                    .get(*slot as usize)
                    .is_some_and(|node| node.mesh_index >= 0)
            })
            .collect()
    };

    slots
        .into_iter()
        .filter_map(|slot| nodes.get(slot as usize))
        .map(|node| CandidateNode {
            slot: node.slot,
            name: node.name.clone(),
            mesh_index: node.mesh_index as u32,
        })
        .collect()
}

/// One candidate that could be drawn, with the facts the ranking compares.
#[derive(Debug)]
struct DrawnCandidate {
    /// The candidate itself.
    candidate: CandidateNode,
    /// Triangles the production render mesh holds for it.
    triangles: usize,
    /// The stored extent of the mesh, in stored units.
    extent: [f32; 3],
    /// The production render mesh itself.
    render: RenderMesh,
}

/// Whether a stored mesh has anything to draw, and its bounding box:
/// `drawable`, `extent`.
fn bounds(mesh: &GameZMesh) -> (bool, [f32; 3]) {
    if mesh.mesh.positions.is_empty() || mesh.mesh.polygons.is_empty() {
        return (false, [0.0; 3]);
    }
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for position in &mesh.mesh.positions {
        for axis in 0..3 {
            min[axis] = min[axis].min(position[axis]);
            max[axis] = max[axis].max(position[axis]);
        }
    }
    let mut extent = [0.0_f32; 3];
    for axis in 0..3 {
        extent[axis] = max[axis] - min[axis];
    }
    (extent.iter().all(|value| value.is_finite()), extent)
}

/// Reads a subject's selection out of its container: builds every candidate
/// the production adapter accepts, ranks them, and counts the coverage of the
/// stored material records the winner references.
///
/// The ranking is the module's one declared rule — **most drawn triangles,
/// ties to the lowest stored slot** — applied to the candidates [`select`]
/// produced. Every candidate that could not be drawn at all is recorded in
/// [`ResolvedSubject::skipped`] with the reason; the drawable candidates that
/// simply ranked lower are alternatives of the same subject, not refusals.
///
/// # Errors
///
/// [`MatrixError::MeshAbsent`] for a mesh slot the container does not hold,
/// [`MatrixError::NoDrawableCandidate`] when no candidate could be drawn (the
/// count of refusals), and [`MatrixError::MaterialAbsent`] when the drawn mesh
/// references a material record the table does not hold.
pub fn load(
    container: &MatrixContainer<'_>,
    selection: &SubjectSelection,
) -> Result<ResolvedSubject, MatrixError> {
    let subject = selection.subject;
    let key = container.key.to_owned();

    let mut drawn: Vec<DrawnCandidate> = Vec::new();
    let mut skipped: Vec<CandidateSkip> = Vec::new();
    for candidate in &selection.candidates {
        let Some(mesh) = container
            .meshes
            .meshes
            .get(candidate.mesh_index as usize)
            .and_then(Option::as_ref)
        else {
            return Err(MatrixError::MeshAbsent {
                subject,
                container: key.clone(),
                mesh_index: candidate.mesh_index,
            });
        };
        let (drawable, extent) = bounds(mesh);
        if !drawable {
            skipped.push(CandidateSkip {
                slot: candidate.slot,
                name: candidate.name.clone(),
                reason: "the stored mesh holds no position and no polygon, so there is no frame \
                         to draw"
                    .to_owned(),
            });
            continue;
        }
        match RenderMesh::from_stored_groups(&mesh.mesh, &mesh.material_groups) {
            Ok(render) if !render.triangles().is_empty() => drawn.push(DrawnCandidate {
                candidate: candidate.clone(),
                triangles: render.triangles().len(),
                extent,
                render,
            }),
            Ok(render) => skipped.push(CandidateSkip {
                slot: candidate.slot,
                name: candidate.name.clone(),
                reason: format!(
                    "the mesh builds but holds {} drawable triangles",
                    render.triangles().len()
                ),
            }),
            Err(error) => skipped.push(CandidateSkip {
                slot: candidate.slot,
                name: candidate.name.clone(),
                reason: error.to_string(),
            }),
        }
    }

    drawn.sort_by(|left, right| {
        right
            .triangles
            .cmp(&left.triangles)
            .then_with(|| left.candidate.slot.cmp(&right.candidate.slot))
    });
    let Some(chosen) = drawn.into_iter().next() else {
        return Err(MatrixError::NoDrawableCandidate {
            subject,
            container: key,
            refusals: skipped.len(),
        });
    };
    let DrawnCandidate {
        candidate,
        triangles,
        extent,
        render,
    } = chosen;

    let unknowns = stored_presentation_unknowns(&render);
    let coverage = coverage_of(container, subject, &candidate)?;

    Ok(ResolvedSubject {
        subject,
        container: container.key.to_owned(),
        anchor_slot: selection.anchor_slot,
        anchor_name: selection.anchor_name.clone(),
        chosen: candidate,
        skipped,
        triangles,
        extent,
        unknowns,
        coverage,
        render,
    })
}

/// Classifies every stored material record the chosen mesh references.
fn coverage_of(
    container: &MatrixContainer<'_>,
    subject: ComparisonSubject,
    candidate: &CandidateNode,
) -> Result<MaterialCoverage, MatrixError> {
    let mesh = container
        .meshes
        .meshes
        .get(candidate.mesh_index as usize)
        .and_then(Option::as_ref)
        .expect("the mesh was read by the caller");
    let mut facts = Vec::with_capacity(mesh.materials.len());
    for reference in &mesh.materials {
        let Some(record) = container
            .materials
            .materials
            .get(reference.material_index as usize)
        else {
            return Err(MatrixError::MaterialAbsent {
                subject,
                container: container.key.to_owned(),
                material_index: reference.material_index,
            });
        };
        facts.push(MaterialFacts::for_raw_record(&record.record));
    }
    Ok(MaterialCoverage::of(facts))
}

/// Builds the comparison set for both containers, keeping every refusal as a
/// row.
///
/// A failure of either step becomes [`MatrixRow::Unresolved`] with the error
/// message as its reason, so the set always has five rows and the caller
/// decides — through [`ComparisonMatrix::is_fully_resolved`] — whether the
/// gaps are acceptable. This is the production path from parsed containers to
/// a matrix.
pub fn resolve_all(
    world: &MatrixContainer<'_>,
    aircraft: &MatrixContainer<'_>,
) -> ComparisonMatrix {
    let world_nodes = world.subject_nodes();
    let aircraft_nodes = aircraft.subject_nodes();
    let mut rows = Vec::with_capacity(ComparisonSubject::ALL.len());
    for subject in ComparisonSubject::ALL {
        let (container, nodes) = match subject.side() {
            SubjectSide::World => (world, &world_nodes),
            SubjectSide::Aircraft => (aircraft, &aircraft_nodes),
        };
        let row = match select(subject, container.key, nodes)
            .and_then(|selection| load(container, &selection))
        {
            Ok(resolved) => MatrixRow::Resolved(Box::new(resolved)),
            Err(error) => MatrixRow::unresolved(subject, error),
        };
        rows.push(row);
    }
    // The rows come from `ComparisonSubject::ALL`, so this can only fail if
    // that constant stopped naming five distinct subjects — a failure worth
    // surfacing rather than unwrapping silently.
    ComparisonMatrix::build(rows).unwrap_or_else(|error| {
        panic!("the set built from ComparisonSubject::ALL is not a set: {error}");
    })
}
