//! The cross-section check between one container's **node array** and its
//! **mesh array**: which scene node names which stored mesh, and which of those
//! associations name nothing at all.
//!
//! The two facts live in two readers and neither one holds the other half.
//! [`GameZNodes`] carries each node's stored `mesh_index` raw and reports the
//! bounds it sees ([`GameZNodes::mesh_index_bounds`]) because it does not hold
//! the mesh section, so it has no array size and no per-slot presence to check
//! the index against. [`GameZMeshes`] holds the array but not the nodes. This
//! module is the one place that has both, so it is the place the check lives:
//! [`NodeMeshBindings::of`] takes a container's two sections and reports every
//! node whose `mesh_index` names no present mesh.
//!
//! # Why "present" and not merely "inside the array"
//!
//! The mesh index is **non-sequential**: an absent mesh record stores the index
//! the next present one is expected to carry, and that expectation follows a
//! different order than the array position (`Fixup`). So the slot a node names
//! is **not** its position in a compact list of the meshes that are there, and
//! the check must use [`GameZMeshes::get`] — the lookup the pinned reference
//! performs — rather than enumerating the present meshes. The two archives that
//! carry a fixup table (`planes.zbd` and C4 `gamez.zbd`) are exactly the ones
//! where a compact enumeration gives a different answer.
//!
//! A slot that is inside the array but holds an all-zero stub is a **different
//! fact** from a slot outside it, and the two are reported as two codes: an
//! absent mesh is a slot the container really has and stores nothing in, while
//! an index past the array names a position the container never had. A caller
//! that resolved both to one "unknown mesh" would lose that.
//!
//! # The node reader is not given the mesh array
//!
//! Nothing here moves into [`GameZNodes`]. The index stays raw there, the bounds
//! stay the honest substitute they are, and the finding is produced by a
//! function that is handed both sections. A node reader that took a mesh
//! section would have to be given one at every call site, and a call site that
//! has none could not run the check at all.
//!
//! A **finding** is not an error: both sections already read, every node is
//! still carried with its stored index, and the reference asserts a non-negative
//! `mesh_index` is inside the array *and* holds a present mesh — an assertion
//! this module measures instead of trusting. A container whose finding list is
//! empty is the strongest claim made about it.
//!
//! Evidence class of the layout this module reports on: documented in the pinned
//! reference *and* measured against the original installation, which is
//! `ObservedTool`. It is never `VerifiedOriginal` here — that needs an original
//! run, which has not happened.

use std::fmt;

use super::nodes::GameZNodes;
use super::reader::GameZMeshes;

/// Why a node's stored `mesh_index` names no present mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshSlotIssue {
    /// The stored index is outside the container's mesh array: the position
    /// does not exist in this container at all.
    OutOfRange {
        /// The stored index.
        slot: i32,
        /// How many array slots the mesh section has.
        slots: u32,
    },
    /// The stored index is inside the array, and the slot it names holds an
    /// all-zero stub record: the container has that position and stores no mesh
    /// in it.
    Absent {
        /// The stored index.
        slot: u32,
    },
}

impl MeshSlotIssue {
    /// Stable machine-matchable identifier of the reason.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::OutOfRange { .. } => "mesh_slot_out_of_range",
            Self::Absent { .. } => "mesh_slot_absent",
        }
    }

    /// The stored index as an unsigned number, which is the same number for
    /// both variants: a non-negative `mesh_index` is the mesh-array slot, and a
    /// negative one never reaches a report at all.
    #[must_use]
    pub const fn slot(self) -> u32 {
        match self {
            Self::OutOfRange { slot, .. } => slot as u32,
            Self::Absent { slot } => slot,
        }
    }
}

impl fmt::Display for MeshSlotIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange { slot, slots } => write!(
                f,
                "mesh slot {slot} is outside the container's {slots}-slot mesh array"
            ),
            Self::Absent { slot } => write!(
                f,
                "mesh slot {slot} is inside the array but holds an all-zero stub record"
            ),
        }
    }
}

/// One scene node whose stored `mesh_index` names no present mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeMeshFinding {
    /// Node array index of the node that stores the index.
    pub node: u32,
    /// The stored `mesh_index`, signed and unchanged: it crosses over exactly
    /// as the node reader read it.
    pub mesh_index: i32,
    /// Why the slot holds no present mesh.
    pub issue: MeshSlotIssue,
}

impl NodeMeshFinding {
    /// Stable machine-matchable identifier of the reason, from
    /// [`MeshSlotIssue::code`].
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.issue.code()
    }

    /// The mesh-array slot the node named.
    #[must_use]
    pub const fn slot(&self) -> u32 {
        self.issue.slot()
    }
}

impl fmt::Display for NodeMeshFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: node {} stores mesh_index {} and {}",
            self.issue.code(),
            self.node,
            self.mesh_index,
            self.issue
        )
    }
}

/// The cross-section verdict for one container: which of its nodes name a
/// stored mesh, and which do not.
///
/// Every field is a count over stored records or a list of the findings, so a
/// caller can enumerate the whole set and see which parts are missing — the
/// shape [`super::census::FaceCensus`] has for faces.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NodeMeshBindings {
    /// Every node record the node section holds.
    pub nodes: u32,
    /// How many nodes store a non-negative `mesh_index` — the same number
    /// [`GameZNodes::mesh_index_bounds`] reports as `bound`, kept here so the
    /// verdict can be checked without a second walk.
    pub bound: u32,
    /// Of those, how many name a slot that holds a **present** mesh.
    pub resolved: u32,
    /// How many store a negative `mesh_index`. `-1` is the reference's "no mesh"
    /// sentinel and is not a problem; a value below `-1` is a separate finding
    /// the node reader already reports
    /// ([`super::nodes::NodeFinding::MeshIndexSentinel`]).
    pub unnamed: u32,
    /// Array slots the mesh section has, absent stubs included.
    pub slots: u32,
    /// How many of those slots stored a mesh.
    pub present: u32,
    /// Every node whose `mesh_index` names no present mesh, in stored node
    /// order. Empty means every non-negative `mesh_index` in this container
    /// names a present mesh, which is the strongest claim this module makes.
    pub findings: Vec<NodeMeshFinding>,
}

impl NodeMeshBindings {
    /// Pairs one container's node array with its mesh section and reports every
    /// node whose `mesh_index` names no present mesh.
    ///
    /// The presence test is [`GameZMeshes::get`] on the **stored** slot, which
    /// is the lookup the pinned reference performs. A compact enumeration of the
    /// present meshes would answer for the two archives that carry a mesh-index
    /// fixup table (`planes.zbd`, C4 `gamez.zbd`) with a different slot than the
    /// node stored.
    #[must_use]
    pub fn of(nodes: &GameZNodes, meshes: &GameZMeshes) -> Self {
        let slots = u32::try_from(meshes.slot_count()).unwrap_or(u32::MAX);
        let mut bindings = Self {
            nodes: u32::try_from(nodes.nodes.len()).unwrap_or(u32::MAX),
            slots,
            present: u32::try_from(meshes.present_count()).unwrap_or(u32::MAX),
            ..Self::default()
        };
        for node in &nodes.nodes {
            let mesh_index = node.mesh_index();
            // A negative index is the `-1` "no mesh" sentinel and names no
            // position in the array, so there is no slot to range-check.
            let Ok(slot) = u32::try_from(mesh_index) else {
                bindings.unnamed += 1;
                continue;
            };
            bindings.bound += 1;
            if meshes.get(slot).is_some() {
                bindings.resolved += 1;
                continue;
            }
            // Inside the array but empty, or outside it: two different facts
            // about the container, so two codes.
            let issue = if slot < slots {
                MeshSlotIssue::Absent { slot }
            } else {
                MeshSlotIssue::OutOfRange {
                    slot: mesh_index,
                    slots,
                }
            };
            bindings.findings.push(NodeMeshFinding {
                node: node.index,
                mesh_index,
                issue,
            });
        }
        bindings
    }

    /// The nodes that named a slot with no present mesh: `bound - resolved`,
    /// which is also [`Self::findings`]'s length.
    #[must_use]
    pub const fn unresolved(&self) -> u32 {
        self.bound - self.resolved
    }

    /// Every **non-negative** `mesh_index` in this container names a present
    /// mesh. This is the reference's own assertion, measured rather than
    /// assumed. A node storing the `-1` sentinel names no position in the array
    /// and so is not part of the claim.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.findings.is_empty()
    }

    /// The finding for one node, when it has one.
    #[must_use]
    pub fn finding(&self, node: u32) -> Option<&NodeMeshFinding> {
        self.findings.iter().find(|finding| finding.node == node)
    }
}

impl fmt::Display for NodeMeshBindings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} of {} nodes name a present mesh ({} of them name a slot at all) across {} slots \
             ({} present); {} of those name none",
            self.resolved,
            self.nodes,
            self.bound,
            self.slots,
            self.present,
            self.unresolved()
        )
    }
}
