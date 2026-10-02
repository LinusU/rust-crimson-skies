//! Canonical scene records: the typed contract between a node-array parser
//! and every runtime consumer
//! (`specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`, stage
//! `### F11-A`; shared contract `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! [`ParsedNode`] is the typed input: one record per stored node of a GameZ
//! container, holding the authored name, kind, parent/children slots, mesh
//! association, zone, raw flags and the authored local transform, exactly as
//! the F11-B reader will produce them. It carries no interpretation the
//! source has not earned: the CS node flag bits are unmeasured, so
//! [`SceneNode::visibility`] is an explicit [`Resolved::Unknown`] rather than
//! a guessed bit.
//!
//! [`SceneGraph::build`] converts a parsed node array into stable
//! [`SceneNodeId`] records — a [`ContentId`] in the `scene_node` namespace
//! whose key is the container key plus the node's authored name-path, so an
//! airframe references a root like `scene_node/planes.<name>` and never a
//! mesh-array position. The build rejects cycles, dangling parents,
//! inconsistent parent/child links, duplicate derived ids and empty scenes
//! (F11 non-negotiable behavior 1), keeps the authored transform beside the
//! canonical one, and keeps every LOD variant: nothing is flattened.
//!
//! # Transforms and mirroring
//!
//! An authored transform is a rotation (an euler triple, or the stored 3×3
//! when the record carries a disagreeing one), a per-axis scale and a
//! translation, in the source's declared convention. Canonical conversion
//! builds the source-space linear map `rotation · scale`, conjugates it
//! through the [`SourceAdapter`]'s axis map and scales the translation by
//! `meters_per_unit`, producing a [`CanonicalTransform`] — a row-major 3×3
//! linear map plus a translation, in meters. It is deliberately a matrix,
//! not TRS: a hierarchy containing a negative scale produces mirror
//! compositions no rotation/scale decomposition can hold. Composition walks
//! roots first so `world = parent_world ∘ local`, and
//! [`CanonicalTransform::mirrored`] reports a negative determinant — the one
//! flag render and collision consumers both read, so a mirrored hull cannot
//! drift apart from its visual across the conversion (the AC01 scenario).
//!
//! [`SceneNode::visual_transform`] and [`SceneNode::collision_transform`]
//! return the *same* composed transform by construction: LOD switching may
//! only change which node's mesh is drawn, never where collision, weapon
//! origins or damage identity live (F11 non-negotiable behavior 4).
//!
//! # Semantic bindings
//!
//! A [`BindingMap`] attaches evidence-backed [`SemanticBinding`] records to
//! nodes by exact authored name-path — never by pattern, never by index.
//! Each binding carries a [`Resolved<PartRole>`] (guns, rocket mounts,
//! engines, control surfaces, camera anchors, damage zones, cockpits), a
//! [`Resolved<CollisionRole>`], [`AnimationBinding`] channels and the
//! [`Provenance`] of the mapping itself. A node with no matching rule is
//! unbound, not silently bound; a rule that matches no node is reported in
//! [`SceneGraph::unmatched_bindings`] rather than dropped.
//!
//! # Part sockets (F11-C)
//!
//! [`PartSocket`] is the typed record a runtime consumer binds to: the bound
//! node, its [`Resolved<PartRole>`] and [`Resolved<CollisionRole>`], its
//! stored zone id, its animation channels, the provenance of the rule — and
//! the node's **one composed** [`CanonicalTransform`] copied in as the mount
//! pose. A socket is derived from the binding, so it can never disagree with
//! the node it names, and it is addressed by [`SceneGraph::socket`] (stable id)
//! or [`SceneGraph::sockets_of_role`], never by array position. A rule whose
//! role is an explicit unknown still yields a socket and is listed by
//! [`SceneGraph::unresolved_sockets`], so the runtime consumer can refuse it
//! visibly instead of defaulting a role.
//!
//! # LOD selection (F11-B)
//!
//! [`select_lod_variant`] is the presentation rule: among the `Lod` siblings
//! that share a parent — one variant group standing for one physical part — it
//! picks the single band that is presented at a viewer distance, reporting how
//! it got there ([`LodCoverage`]: covered, overlapping bands or an authored
//! gap). It returns an index and a coverage, nothing else: no transform, no
//! identity, no damage state, so a distance change can only ever change which
//! variant is drawn (F11 non-negotiable behavior 4). The rule is designed
//! engine contract; the original's selection behaviour is unmeasured.
//!
//! # Roster audit (F11-D)
//!
//! [`AirframeRoster::audit`] is the evidence stage's instrument: it takes the
//! declared roster (one [`RosterEntry`] per discovered airframe, each with the
//! root it references and the roles it is declared to carry) plus the set of
//! discovered [`SceneContainerRef`]s, and maps every root, part, mount and
//! cockpit binding it can reach — or names the blocker that stopped it.
//!
//! Three properties make the verdict worth having. A root is reached by
//! [`SceneRootRef`], never by array position, and a socket is reached by its
//! stable [`SceneNodeId`], so a mapping cannot silently attach to a
//! neighbouring airframe. A container whose node array has not been decoded
//! contributes a typed [`ContainerBlocker`] carrying the *measured* stored
//! record count and offset, and every airframe in it inherits that blocker —
//! the audit reports "nothing is mapped and here is the exact missing step"
//! instead of an empty pass. And roster availability is discovered
//! separately from forced mission assignments: a [`ForcedMissionAssignment`]
//! never promotes an airframe to [`RosterAvailability::Selectable`], so a
//! mission-only type stays mission-only and an undiscovered roster stays
//! unknown (F11 non-negotiable behavior 3).
//!
//! # What is measured and what is designed
//!
//! The input record mirrors the pinned mech3ax v0.6.0 node layout
//! (`NodeCsC`, `Object3dCsC`, `LodCsC`; sources S02/S17) as *observed-tool*
//! evidence: the 208-byte record's `mesh_index`, parent/children slots, the
//! 144-byte object record's euler `rotation` + `scale` + stored `matrix` +
//! `translation`, and the 92-byte LOD record's `level` plus its range
//! fields (the near bound stored squared, the far bound stored twice). The
//! euler-to-matrix convention (`Rz·Ry·Rx` with negated angles) and the
//! stored-matrix precedence are the reference's rules; they are not verified
//! against the original executable. The canonical record layout, the
//! `scene_node` id scheme, the role vocabularies and the binding mechanism
//! are designed engine contracts, not original data. Recorded unknowns —
//! flag-bit semantics, roster selectability, LOD `level` meaning — are in
//! `docs/findings/2026-09-29-f11-a-node-hierarchy-bindings.md`.

use std::collections::{BTreeMap, HashSet};
use std::fmt;

use cs_formats::gamez::{GameZNodes, NodeKind as StoredNodeKind, RawLodData, RawNode};
use cs_types::content::{ContentId, ContentIdError, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::{Meters, SpaceError};

use crate::coordinates::SourceAdapter;

/// The zone id CS node records use for "no zone", per the pinned reference
/// (`ZONE_DEFAULT` in `mech3ax-nodes/src/types.rs`).
pub const ZONE_DEFAULT: u32 = 255;

// -------------------------------------------------------------- identity ---

/// The stable identity of one scene node.
///
/// Wraps a [`ContentId`] in the `scene_node` namespace. The key is the
/// owning container's key plus the node's authored name-path joined by `.`
/// (`scene_node/planes.corsair.wing_l`), so identity is semantic and stable
/// across parses — never a mesh-array or node-array position (F11
/// deliverable).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SceneNodeId(ContentId);

impl SceneNodeId {
    /// Adopts an existing content id, which must be in the `scene_node`
    /// namespace.
    ///
    /// # Errors
    ///
    /// [`SceneError::NodeKind`] when `id` names a different kind.
    pub fn from_content_id(id: ContentId) -> Result<Self, SceneError> {
        if id.kind() != ContentKind::SceneNode {
            return Err(SceneError::NodeKind { kind: id.kind() });
        }
        Ok(Self(id))
    }

    /// The id derived from a container and an authored name-path.
    ///
    /// The path uses the authored names joined by `.`; normalization
    /// (lowercasing, the key grammar) is [`ContentId`]'s, so a path that
    /// cannot form a key is refused rather than transliterated.
    ///
    /// # Errors
    ///
    /// [`SceneError::NodeId`] when the derived key violates the id grammar.
    fn for_path(container: &ContentId, path: &str, node: u32) -> Result<Self, SceneError> {
        ContentId::from_source(
            ContentKind::SceneNode,
            &format!("{}.{}", container.key(), path),
        )
        .map(Self)
        .map_err(|source| SceneError::NodeId { node, source })
    }

    /// The underlying catalog id.
    #[must_use]
    pub fn as_content_id(&self) -> &ContentId {
        &self.0
    }

    /// The normalized `container.path` key, without the namespace.
    #[must_use]
    pub fn key(&self) -> &str {
        self.0.key()
    }
}

impl fmt::Display for SceneNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A reference to one scene root inside one container — the handle an
/// airframe definition holds (F11 deliverable: "Airframe definitions
/// reference roots in PLANES.ZBD, not models selected by array position").
///
/// The constructor refuses a root id that does not live directly under the
/// named container, so a reference can never silently point at a nested node
/// or at another container's tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneRootRef {
    container: ContentId,
    root: SceneNodeId,
}

impl SceneRootRef {
    /// Binds `root` to `container`.
    ///
    /// # Errors
    ///
    /// [`SceneError::RootOutsideContainer`] when the root's key is not
    /// `<container key>.<name>`, and [`SceneError::NotARootNode`] when the
    /// remainder is itself a nested path.
    pub fn new(container: ContentId, root: SceneNodeId) -> Result<Self, SceneError> {
        let prefix = format!("{}.", container.key());
        let Some(rest) = root.key().strip_prefix(&prefix) else {
            return Err(SceneError::RootOutsideContainer {
                container: container.key().to_owned(),
                root: root.key().to_owned(),
            });
        };
        if rest.contains('.') {
            return Err(SceneError::NotARootNode {
                root: root.key().to_owned(),
            });
        }
        Ok(Self { container, root })
    }

    /// The container holding the root's node array.
    #[must_use]
    pub fn container(&self) -> &ContentId {
        &self.container
    }

    /// The root node inside that container.
    #[must_use]
    pub fn root(&self) -> &SceneNodeId {
        &self.root
    }
}

// ---------------------------------------------------- canonical transform ---

/// A canonical affine transform: a row-major 3×3 linear map plus a
/// translation, in meters.
///
/// A matrix is used instead of TRS because authored hierarchies can carry
/// negative scale: `rotation · scale` compositions with mirroring do not
/// survive a rotation/scale decomposition, and the composed result is what
/// both the render and the collision path must see. All values are checked
/// finite at the boundary ([`SpaceError::NonFinite`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanonicalTransform {
    linear: [[f64; 3]; 3],
    translation: [f64; 3],
}

const LINEAR_FIELDS: [&str; 9] = [
    "linear[0]",
    "linear[1]",
    "linear[2]",
    "linear[3]",
    "linear[4]",
    "linear[5]",
    "linear[6]",
    "linear[7]",
    "linear[8]",
];
const TRANSLATION_FIELDS: [&str; 3] = ["translation[0]", "translation[1]", "translation[2]"];

impl CanonicalTransform {
    /// The identity transform.
    pub const IDENTITY: Self = Self {
        linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        translation: [0.0, 0.0, 0.0],
    };

    /// Validates a linear map plus translation, both finite.
    ///
    /// # Errors
    ///
    /// [`SpaceError::NonFinite`] naming the first non-finite component.
    pub fn try_new(linear: [[f64; 3]; 3], translation: [f64; 3]) -> Result<Self, SpaceError> {
        check_finite(
            [
                linear[0][0],
                linear[0][1],
                linear[0][2],
                linear[1][0],
                linear[1][1],
                linear[1][2],
                linear[2][0],
                linear[2][1],
                linear[2][2],
            ],
            LINEAR_FIELDS,
        )?;
        check_finite(translation, TRANSLATION_FIELDS)?;
        Ok(Self {
            linear,
            translation,
        })
    }

    /// The row-major 3×3 linear map.
    #[must_use]
    pub const fn linear(&self) -> [[f64; 3]; 3] {
        self.linear
    }

    /// The translation, in meters.
    #[must_use]
    pub const fn translation(&self) -> [f64; 3] {
        self.translation
    }

    /// `self ∘ inner`: the transform that applies `inner` first, then `self`.
    ///
    /// Used to compose a node's local transform under its parent's world
    /// transform: `world = parent_world.compose(local)`.
    #[must_use]
    pub fn compose(&self, inner: &Self) -> Self {
        let mut linear = [[0.0; 3]; 3];
        for (row, out) in linear.iter_mut().enumerate() {
            for (col, cell) in out.iter_mut().enumerate() {
                *cell = self.linear[row][0] * inner.linear[0][col]
                    + self.linear[row][1] * inner.linear[1][col]
                    + self.linear[row][2] * inner.linear[2][col];
            }
        }
        let t = self.transform_vector(inner.translation);
        Self {
            linear,
            translation: [
                t[0] + self.translation[0],
                t[1] + self.translation[1],
                t[2] + self.translation[2],
            ],
        }
    }

    /// Maps a canonical-space point through this transform: `L·p + t`.
    #[must_use]
    pub fn apply(&self, point: [f64; 3]) -> [f64; 3] {
        let v = self.transform_vector(point);
        [
            v[0] + self.translation[0],
            v[1] + self.translation[1],
            v[2] + self.translation[2],
        ]
    }

    /// Maps a direction or offset through the linear map only: `L·v`.
    #[must_use]
    pub fn transform_vector(&self, vector: [f64; 3]) -> [f64; 3] {
        [
            self.linear[0][0] * vector[0]
                + self.linear[0][1] * vector[1]
                + self.linear[0][2] * vector[2],
            self.linear[1][0] * vector[0]
                + self.linear[1][1] * vector[1]
                + self.linear[1][2] * vector[2],
            self.linear[2][0] * vector[0]
                + self.linear[2][1] * vector[1]
                + self.linear[2][2] * vector[2],
        ]
    }

    /// The determinant of the linear map.
    #[must_use]
    pub fn determinant(&self) -> f64 {
        let l = self.linear;
        l[0][0] * (l[1][1] * l[2][2] - l[1][2] * l[2][1])
            - l[0][1] * (l[1][0] * l[2][2] - l[1][2] * l[2][0])
            + l[0][2] * (l[1][0] * l[2][1] - l[1][1] * l[2][0])
    }

    /// Whether this transform mirrors space (`det < 0`): authored negative
    /// scale survives conversion, so a mirrored subtree reports it here and
    /// every consumer sees the same flag.
    #[must_use]
    pub fn mirrored(&self) -> bool {
        self.determinant() < 0.0
    }
}

/// Rejects the first non-finite input component, naming it the way the
/// caller spelled it, before any conversion runs.
fn check_finite<const N: usize>(
    values: [f64; N],
    fields: [&'static str; N],
) -> Result<(), SpaceError> {
    for (value, field) in values.into_iter().zip(fields) {
        if !value.is_finite() {
            return Err(SpaceError::NonFinite { field });
        }
    }
    Ok(())
}

// ---------------------------------------------------------- parsed input ---

/// The authored local transform of one parsed node, in source units and the
/// source's declared convention.
///
/// `rotation` is the stored euler triple in the source's declared angle
/// unit; `matrix` is the 3×3 the
/// record stores when it disagrees with the euler-derived one — the pinned
/// reference corpus disagrees in ~0.74 % of objects, so a stored `matrix`
/// takes precedence and nothing is recomputed over it. `scale` is applied
/// first (`rotation · scale`); every measured CS scale is `1.0`, so the
/// composition order is a designed choice recorded in the findings, not an
/// observed fact.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AuthoredTransform {
    /// Stored euler rotation, in radians, in the source's convention.
    pub rotation: [f32; 3],
    /// Stored per-axis scale; a negative component mirrors.
    pub scale: [f32; 3],
    /// The stored 3×3 when it differs from the euler-derived matrix;
    /// row-major, source axes.
    pub matrix: Option<[[f32; 3]; 3]>,
    /// Stored translation, in source units.
    pub translation: [f32; 3],
}

impl AuthoredTransform {
    /// The transform of a node stored without one (the reference's
    /// `flags == 40` object records): identity.
    pub const IDENTITY: Self = Self {
        rotation: [0.0, 0.0, 0.0],
        scale: [1.0, 1.0, 1.0],
        matrix: None,
        translation: [0.0, 0.0, 0.0],
    };
}

/// The kind a parsed node declares, mirroring the CS node-type tag
/// (`NodeType` in the pinned reference: camera 1, world 2, window 3,
/// display 4, object3d 5, lod 6, light 9; `empty` is a parse error, never a
/// [`ParsedNodeKind`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParsedNodeKind {
    /// A world container node.
    World,
    /// A window node.
    Window,
    /// A display node.
    Display,
    /// A camera node.
    Camera,
    /// A light node.
    Light,
    /// An object node carrying the authored transform and (usually) a mesh.
    Object3d,
    /// One LOD variant selector: `level` is the stored boolean and
    /// `range_min`/`range_max` the near/far distances in source units.
    ///
    /// The stored LOD data record (`LodCsC`, 92 bytes) holds the near bound
    /// *squared* (`range_near_sq`) and the far bound twice (`range_far`
    /// plus `range_far_sq`, asserted equal to its square); the reader
    /// resolves the square root and the consistency check before producing
    /// this record, so the fields here are the resolved distances.
    Lod {
        /// The stored level flag.
        level: bool,
        /// Near distance, source units (the record stores it squared).
        range_min: f32,
        /// Far distance, source units.
        range_max: f32,
    },
}

/// A node's association with one mesh-array entry.
///
/// `index` is the stored `mesh_index` — the only address a mesh has, since
/// meshes carry no names. The record stores it signed with `-1` meaning
/// "no mesh"; a `MeshBinding` exists only for a non-negative value, so the
/// reader maps `-1` to [`ParsedNode::mesh`] `= None`. `mesh` resolves that
/// index to the mesh element's catalog id (kind [`ContentKind::Mesh`]) or
/// records it unresolved; the importer supplies the resolution, so this
/// record never invents an id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshBinding {
    /// The stored mesh-array index (the record's `mesh_index` was `-1` for
    /// "no mesh"; only non-negative values become a binding).
    pub index: u32,
    /// The mesh element the index resolves to.
    pub mesh: Resolved<ContentId>,
}

/// One stored node record, decoded — the typed input to
/// [`SceneGraph::build`].
///
/// `index`, `parent` and `children` are the stored array slots: they wire
/// the hierarchy, feed diagnostics and name the `mesh_index` association,
/// but they never enter the node's identity. `flags` and `zone_id` are kept
/// raw; the CS flag bits are unmeasured, so interpretation stays out of this
/// stage.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedNode {
    /// Stored array slot.
    pub index: u32,
    /// Authored display name (the stored 36-byte name field, decoded).
    pub name: String,
    /// The node's kind tag and kind-specific data.
    pub kind: ParsedNodeKind,
    /// The authored local transform.
    pub transform: AuthoredTransform,
    /// The stored parent slot, when the record has one.
    pub parent: Option<u32>,
    /// The stored children slots, in stored order.
    pub children: Vec<u32>,
    /// The mesh association, when the node has one.
    pub mesh: Option<MeshBinding>,
    /// The stored zone id; [`ZONE_DEFAULT`] means no zone.
    pub zone_id: u32,
    /// The stored node flags, uninterpreted.
    pub flags: u32,
}

impl ParsedNode {
    /// A node with identity transform, no links, no mesh and the default
    /// zone; the fields stay public so a fixture sets only what it
    /// exercises.
    #[must_use]
    pub fn new(index: u32, name: impl Into<String>, kind: ParsedNodeKind) -> Self {
        Self {
            index,
            name: name.into(),
            kind,
            transform: AuthoredTransform::IDENTITY,
            parent: None,
            children: Vec::new(),
            mesh: None,
            zone_id: ZONE_DEFAULT,
            flags: 0,
        }
    }
}

// ------------------------------------------------------- canonical nodes ---

/// A LOD variant's canonical data: the stored level flag and its distance
/// range converted to meters.
///
/// LOD is presentation state only (F11 non-negotiable behavior 4): a `Lod`
/// node selects which child gets *drawn*, and collision, weapon origins and
/// damage identity live on the bound node regardless of which variant is
/// active.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodInfo {
    /// The stored level flag.
    pub level: bool,
    /// Near distance in canonical meters.
    pub range_min: Meters,
    /// Far distance in canonical meters.
    pub range_max: Meters,
}

/// The canonical kind of a [`SceneNode`].
#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    /// A world container node.
    World,
    /// A window node.
    Window,
    /// A display node.
    Display,
    /// A camera node.
    Camera,
    /// A light node.
    Light,
    /// An object node.
    Object3d,
    /// An LOD variant selector, with its converted range.
    Lod(LodInfo),
}

/// Whether a node is drawn.
///
/// The CS node flag bits are unmeasured (`NodeBitFlagsCs` in the pinned
/// reference names every bit `UNK`), so a [`SceneNode`]'s visibility arrives
/// as [`Resolved::Unknown`] until an evidence stage reads them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeVisibility {
    /// The node is drawn.
    Visible,
    /// The node is not drawn.
    Hidden,
}

/// Whether a node's geometry participates in collision.
///
/// Designed vocabulary, assigned by an evidence-backed binding rule; the
/// authored flag/partition fields that select it in the original are
/// unmeasured, so unbound nodes carry [`Resolved::Unknown`], not a guessed
/// default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollisionRole {
    /// The node carries no collision surface.
    None,
    /// The node's geometry bounds a collider; its surface uses the node's
    /// composed [`CanonicalTransform`], the same transform the render path
    /// draws.
    Collider,
}

/// The gameplay role a semantic binding assigns to a node
/// (F11 non-negotiable behavior 2).
///
/// The vocabulary is designed engine contract; which authored nodes hold
/// which roles is discovered per container by the evidence stages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartRole {
    /// A gun mount.
    Gun,
    /// A rocket or hardpoint mount.
    RocketMount,
    /// An engine.
    Engine,
    /// A control surface.
    ControlSurface,
    /// A camera anchor.
    CameraAnchor,
    /// A damage zone.
    DamageZone,
    /// The cockpit binding.
    Cockpit,
}

impl PartRole {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Gun => "gun",
            Self::RocketMount => "rocket_mount",
            Self::Engine => "engine",
            Self::ControlSurface => "control_surface",
            Self::CameraAnchor => "camera_anchor",
            Self::DamageZone => "damage_zone",
            Self::Cockpit => "cockpit",
        }
    }
}

/// An animation channel bound to a node.
///
/// The channel record itself is F20-A's; here the binding is a resolved
/// reference (kind [`ContentKind::AnimationTrack`]) or an explicit unknown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnimationBinding {
    /// The animation channel bound to this node.
    pub channel: Resolved<ContentId>,
}

/// One evidence-backed binding rule: the authored name-path it attaches to
/// plus the semantics it assigns.
///
/// `path` is matched exactly against a node's authored name-path from its
/// root (e.g. `main.wing_l.gun`) — exact match only, no pattern or index —
/// so a rule can never land on a node the evidence did not name.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticBinding {
    /// The authored name-path this binding attaches to.
    pub path: String,
    /// The assigned gameplay role, or an explicit unknown.
    pub role: Resolved<PartRole>,
    /// The assigned collision role, or an explicit unknown.
    pub collision: Resolved<CollisionRole>,
    /// Animation channels bound to the node.
    pub animation: Vec<AnimationBinding>,
    /// Where the mapping itself comes from.
    pub provenance: Provenance,
}

/// The evidence-backed name-path → semantics table a conversion is built
/// with.
///
/// The map is input, not hard-coded: rules are supplied per container by the
/// importer's evidence and carried with provenance, so "the mapping exists"
/// is itself a checkable claim. Duplicate rules for one path are refused at
/// construction — an ambiguous mapping is an authoring error, not a runtime
/// choice.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BindingMap {
    bindings: Vec<SemanticBinding>,
}

impl BindingMap {
    /// Collects the rules, refusing duplicate target paths.
    ///
    /// # Errors
    ///
    /// [`SceneError::DuplicateBindingRule`] when two rules name the same
    /// authored path.
    pub fn new(bindings: Vec<SemanticBinding>) -> Result<Self, SceneError> {
        let mut seen = HashSet::new();
        for binding in &bindings {
            if !seen.insert(binding.path.clone()) {
                return Err(SceneError::DuplicateBindingRule {
                    path: binding.path.clone(),
                });
            }
        }
        Ok(Self { bindings })
    }

    /// The rules, in supplied order.
    #[must_use]
    pub fn rules(&self) -> &[SemanticBinding] {
        &self.bindings
    }
}

// ------------------------------------------------------------- part sockets ---

/// One semantic socket (F11 deliverable): a bound node together with the
/// gameplay role, collision role, pose and zone a runtime consumer binds to.
///
/// A socket is what a weapon mount, a camera anchor, a damage zone or an
/// engine consumer addresses. It holds **no transform of its own to edit**:
/// `pose` is the node's one composed [`CanonicalTransform`] — the same value
/// [`SceneNode::collision_transform`] returns — copied at build time so a
/// consumer can read the mount point without walking the hierarchy, and
/// copied so that it can never become a second, diverging pose owner (F11
/// non-negotiable behavior 4; `IDENTITY-CONTENT`: one pose owner).
///
/// The role stays [`Resolved`]: a rule whose role the evidence could not
/// resolve still produces a socket, carrying an explicit unknown with its
/// claim and reason, and is listed by [`SceneGraph::unresolved_sockets`]. It is
/// never given a default role.
#[derive(Clone, Debug, PartialEq)]
pub struct PartSocket {
    node: SceneNodeId,
    role: Resolved<PartRole>,
    collision: Resolved<CollisionRole>,
    pose: CanonicalTransform,
    zone_id: u32,
    animation: Vec<AnimationBinding>,
    provenance: Provenance,
}

impl PartSocket {
    /// The bound node this socket is mounted on.
    #[must_use]
    pub fn node(&self) -> &SceneNodeId {
        &self.node
    }

    /// The gameplay role, or the explicit unknown the evidence left.
    #[must_use]
    pub fn role(&self) -> &Resolved<PartRole> {
        &self.role
    }

    /// The known role, or `None` when the evidence left it unknown.
    #[must_use]
    pub fn known_role(&self) -> Option<PartRole> {
        match &self.role {
            Resolved::Known(known) => Some(known.value),
            Resolved::Unknown { .. } => None,
        }
    }

    /// The collision role, or the explicit unknown the evidence left.
    #[must_use]
    pub fn collision(&self) -> &Resolved<CollisionRole> {
        &self.collision
    }

    /// The mount pose: the node's one composed canonical transform, the same
    /// value the render and collision paths use.
    #[must_use]
    pub fn pose(&self) -> &CanonicalTransform {
        &self.pose
    }

    /// The bound node's stored zone id, uninterpreted.
    #[must_use]
    pub fn zone_id(&self) -> u32 {
        self.zone_id
    }

    /// The animation channels bound to the node.
    #[must_use]
    pub fn animation(&self) -> &[AnimationBinding] {
        &self.animation
    }

    /// The provenance of the mapping rule that produced this socket.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// One canonical scene node: a stable [`SceneNodeId`], its authored parent
/// and children, the authored transform preserved beside the canonical
/// local and world transforms, the mesh association, LOD state, and whatever
/// semantics the binding map evidenced.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneNode {
    id: SceneNodeId,
    path: String,
    name: String,
    index: u32,
    kind: NodeKind,
    parent: Option<SceneNodeId>,
    children: Vec<SceneNodeId>,
    authored: AuthoredTransform,
    local: CanonicalTransform,
    world: CanonicalTransform,
    mesh: Option<MeshBinding>,
    zone_id: u32,
    flags: u32,
    visibility: Resolved<NodeVisibility>,
    binding: Option<SemanticBinding>,
}

impl SceneNode {
    /// The stable identity.
    #[must_use]
    pub fn id(&self) -> &SceneNodeId {
        &self.id
    }

    /// The authored name-path from the root, raw and unnormalized — the
    /// string binding rules match exactly.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The authored display name, outside identity.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The stored array slot: provenance for diagnostics and the
    /// `mesh_index` association, never an identity.
    #[must_use]
    pub fn index(&self) -> u32 {
        self.index
    }

    /// The canonical kind, including converted LOD data.
    #[must_use]
    pub fn kind(&self) -> &NodeKind {
        &self.kind
    }

    /// LOD data when this node is a `Lod` selector.
    #[must_use]
    pub fn lod(&self) -> Option<&LodInfo> {
        match &self.kind {
            NodeKind::Lod(info) => Some(info),
            _ => None,
        }
    }

    /// The parent node, when this is not a root.
    #[must_use]
    pub fn parent(&self) -> Option<&SceneNodeId> {
        self.parent.as_ref()
    }

    /// The children, in stored order.
    #[must_use]
    pub fn children(&self) -> &[SceneNodeId] {
        &self.children
    }

    /// The authored local transform, preserved raw.
    #[must_use]
    pub fn authored(&self) -> &AuthoredTransform {
        &self.authored
    }

    /// The canonical local transform (converted through the declared source
    /// adapter at build time).
    #[must_use]
    pub fn local_transform(&self) -> &CanonicalTransform {
        &self.local
    }

    /// The composed canonical transform of this node: `root ∘ … ∘ local`.
    ///
    /// This is the single pose owner for the node.
    #[must_use]
    pub fn world_transform(&self) -> &CanonicalTransform {
        &self.world
    }

    /// The transform the render path draws — deliberately the same value as
    /// [`Self::collision_transform`]: presentation can never diverge from
    /// collision.
    #[must_use]
    pub fn visual_transform(&self) -> &CanonicalTransform {
        &self.world
    }

    /// The transform collision evaluates — deliberately the same value as
    /// [`Self::visual_transform`].
    #[must_use]
    pub fn collision_transform(&self) -> &CanonicalTransform {
        &self.world
    }

    /// Whether the composed world transform mirrors space (negative scale
    /// anywhere on the path).
    #[must_use]
    pub fn mirrored(&self) -> bool {
        self.world.mirrored()
    }

    /// The mesh association, when the node has one.
    #[must_use]
    pub fn mesh(&self) -> Option<&MeshBinding> {
        self.mesh.as_ref()
    }

    /// The stored zone id, uninterpreted.
    #[must_use]
    pub fn zone_id(&self) -> u32 {
        self.zone_id
    }

    /// The stored node flags, uninterpreted.
    #[must_use]
    pub fn flags(&self) -> u32 {
        self.flags
    }

    /// Whether the node is drawn. The CS flag bits are unmeasured, so this
    /// is [`Resolved::Unknown`] until an evidence stage reads them — the
    /// record has the field; the value is honestly not known yet.
    #[must_use]
    pub fn visibility(&self) -> &Resolved<NodeVisibility> {
        &self.visibility
    }

    /// The semantic binding attached to this node, when a rule named its
    /// authored path.
    #[must_use]
    pub fn binding(&self) -> Option<&SemanticBinding> {
        self.binding.as_ref()
    }
}

// ------------------------------------------------------------- the graph ---

/// A converted container: every [`SceneNode`], its roots, and the binding
/// rules that matched no node.
///
/// `nodes` is in root-first preorder (parents before children); `roots` is
/// in stored order. The authored hierarchy is preserved — a [`SceneNode`]
/// keeps its local transform and its parent/children ids — so nothing about
/// the conversion is irreversible.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneGraph {
    container: ContentId,
    nodes: Vec<SceneNode>,
    roots: Vec<SceneNodeId>,
    by_id: BTreeMap<SceneNodeId, usize>,
    sockets: BTreeMap<SceneNodeId, PartSocket>,
    unmatched_bindings: Vec<String>,
}

impl SceneGraph {
    /// Converts a parsed node array into canonical form.
    ///
    /// The hierarchy is validated before any node is produced: unique stored
    /// slots, in-range parent/child links, consistent parent↔child records,
    /// at least one root and no cycles. Each node's id derives from the
    /// container key and its authored name-path; a collision between derived
    /// ids is refused rather than disambiguated by position.
    ///
    /// `adapter` is the declared source convention every authored value is
    /// converted through exactly once; `bindings` supplies the
    /// evidence-backed semantic mapping and may be empty.
    ///
    /// # Errors
    ///
    /// [`SceneError::EmptyScene`], [`SceneError::DuplicateIndex`],
    /// [`SceneError::DanglingParent`], [`SceneError::DanglingChild`],
    /// [`SceneError::InconsistentParentage`], [`SceneError::NoRoots`],
    /// [`SceneError::Cycle`], [`SceneError::NodeId`],
    /// [`SceneError::DuplicateNodeId`], [`SceneError::Transform`],
    /// [`SceneError::LodRange`], [`SceneError::MeshKind`] and
    /// [`SceneError::AnimationChannelKind`].
    pub fn build(
        container: &ContentId,
        nodes: &[ParsedNode],
        adapter: &SourceAdapter,
        bindings: &BindingMap,
    ) -> Result<Self, SceneError> {
        if nodes.is_empty() {
            return Err(SceneError::EmptyScene);
        }

        // Stored slots must be unique before any link is trusted; they wire
        // the hierarchy but never enter identity.
        let mut position_of: BTreeMap<u32, usize> = BTreeMap::new();
        for (position, node) in nodes.iter().enumerate() {
            if position_of.insert(node.index, position).is_some() {
                return Err(SceneError::DuplicateIndex { index: node.index });
            }
        }

        // Link ranges, then both directions of parent↔child consistency: a
        // node that names a parent must appear in that parent's children,
        // and a listed child must name this node as its parent.
        for node in nodes {
            if let Some(parent) = node.parent {
                let Some(&parent_position) = position_of.get(&parent) else {
                    return Err(SceneError::DanglingParent {
                        node: node.index,
                        parent,
                    });
                };
                if !nodes[parent_position].children.contains(&node.index) {
                    return Err(SceneError::InconsistentParentage {
                        node: node.index,
                        parent,
                    });
                }
            }
            for &child in &node.children {
                let Some(&child_position) = position_of.get(&child) else {
                    return Err(SceneError::DanglingChild {
                        node: node.index,
                        child,
                    });
                };
                if nodes[child_position].parent != Some(node.index) {
                    return Err(SceneError::InconsistentParentage {
                        node: child,
                        parent: node.index,
                    });
                }
            }
        }

        let roots: Vec<usize> = nodes
            .iter()
            .enumerate()
            .filter_map(|(position, node)| node.parent.is_none().then_some(position))
            .collect();
        if roots.is_empty() {
            return Err(SceneError::NoRoots);
        }

        // Depth-first from each root in stored order, keeping stored child
        // order: assign the authored name-path and remember each node's
        // parent position. A node is reachable at most once — consistency
        // above forces a single parent — so a revisited node is an ownership
        // cycle (IDENTITY-CONTENT: invalid in parent hierarchies).
        let mut order: Vec<usize> = Vec::with_capacity(nodes.len());
        let mut paths: Vec<Option<String>> = vec![None; nodes.len()];
        let mut visited = vec![false; nodes.len()];
        for &root in &roots {
            let mut stack: Vec<(usize, String)> = vec![(root, String::new())];
            while let Some((position, prefix)) = stack.pop() {
                if visited[position] {
                    return Err(SceneError::Cycle {
                        node: nodes[position].index,
                    });
                }
                visited[position] = true;
                let path = if prefix.is_empty() {
                    nodes[position].name.clone()
                } else {
                    format!("{prefix}.{}", nodes[position].name)
                };
                paths[position] = Some(path.clone());
                order.push(position);
                for &child in nodes[position].children.iter().rev() {
                    stack.push((position_of[&child], path.clone()));
                }
            }
        }
        // A node no root reaches has a parent chain that can only loop —
        // every link was validated consistent — so it sits on a detached
        // ownership cycle.
        for (position, node) in nodes.iter().enumerate() {
            if !visited[position] {
                return Err(SceneError::Cycle { node: node.index });
            }
        }

        let mut ids: Vec<SceneNodeId> = Vec::with_capacity(nodes.len());
        let mut seen_ids: HashSet<SceneNodeId> = HashSet::new();
        for (position, path) in paths.iter().enumerate() {
            let id = SceneNodeId::for_path(
                container,
                path.as_ref().expect("every node was reached"),
                nodes[position].index,
            )?;
            if !seen_ids.insert(id.clone()) {
                return Err(SceneError::DuplicateNodeId { id });
            }
            ids.push(id);
        }

        // The node records themselves, in DFS preorder (parents before
        // children), so each parent's composed transform already exists when
        // its child is converted.
        let mut built: Vec<SceneNode> = Vec::with_capacity(nodes.len());
        let mut worlds: Vec<Option<CanonicalTransform>> = vec![None; nodes.len()];
        let mut by_id: BTreeMap<SceneNodeId, usize> = BTreeMap::new();
        let mut matched: Vec<bool> = vec![false; bindings.rules().len()];
        for &position in &order {
            let parsed = &nodes[position];
            let local = canonical_local(&parsed.transform, adapter).map_err(|source| {
                SceneError::Transform {
                    node: parsed.index,
                    source,
                }
            })?;
            let world = match parsed.parent {
                Some(parent) => {
                    let parent_position = position_of[&parent];
                    worlds[parent_position]
                        .expect("parents are converted before children")
                        .compose(&local)
                }
                None => local,
            };
            worlds[position] = Some(world);

            let kind = convert_kind(parsed, adapter)?;
            if let Some(mesh) = &parsed.mesh
                && let Resolved::Known(known) = &mesh.mesh
                && known.value.kind() != ContentKind::Mesh
            {
                return Err(SceneError::MeshKind {
                    node: parsed.index,
                    kind: known.value.kind(),
                });
            }

            let children = parsed
                .children
                .iter()
                .map(|slot| ids[position_of[slot]].clone())
                .collect();
            let parent = parsed.parent.map(|slot| ids[position_of[&slot]].clone());
            by_id.insert(ids[position].clone(), built.len());
            built.push(SceneNode {
                id: ids[position].clone(),
                path: paths[position].clone().expect("path assigned"),
                name: parsed.name.clone(),
                index: parsed.index,
                kind,
                parent,
                children,
                authored: parsed.transform,
                local,
                world,
                mesh: parsed.mesh.clone(),
                zone_id: parsed.zone_id,
                flags: parsed.flags,
                visibility: Resolved::unknown(
                    ClaimId::new("f11a.node-flags-unmeasured").expect("claim id is valid"),
                    "the CS node flag bits are unmeasured; visibility stays unknown",
                )
                .expect("the reason is not empty"),
                binding: attach_binding(
                    parsed,
                    paths[position].as_deref().expect("path assigned"),
                    bindings,
                    &mut matched,
                )?,
            });
        }

        let unmatched_bindings = bindings
            .rules()
            .iter()
            .zip(matched)
            .filter(|(_, was_matched)| !was_matched)
            .map(|(rule, _)| rule.path.clone())
            .collect();
        let roots = order
            .iter()
            .filter(|&&position| nodes[position].parent.is_none())
            .map(|&position| ids[position].clone())
            .collect();
        // One socket per bound node, keyed by the node's stable id so a
        // consumer reaches a mount by identity and never by position. The
        // pose is the node's own composed transform, so a socket cannot hold
        // a second, divergent pose.
        let mut sockets: BTreeMap<SceneNodeId, PartSocket> = BTreeMap::new();
        for node in &built {
            let Some(binding) = &node.binding else {
                continue;
            };
            sockets.insert(
                node.id.clone(),
                PartSocket {
                    node: node.id.clone(),
                    role: binding.role.clone(),
                    collision: binding.collision.clone(),
                    pose: node.world,
                    zone_id: node.zone_id,
                    animation: binding.animation.clone(),
                    provenance: binding.provenance.clone(),
                },
            );
        }
        Ok(Self {
            container: container.clone(),
            nodes: built,
            roots,
            by_id,
            sockets,
            unmatched_bindings,
        })
    }

    /// The container this graph was converted from.
    #[must_use]
    pub fn container(&self) -> &ContentId {
        &self.container
    }

    /// Every node, in root-first preorder.
    #[must_use]
    pub fn nodes(&self) -> &[SceneNode] {
        &self.nodes
    }

    /// The number of nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the graph is empty (it cannot be — a build with no nodes is
    /// refused; the predicate exists for completeness).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The roots, in stored order. A container may hold several (one per
    /// airframe in PLANES.ZBD); address one by name-path through
    /// [`Self::root`].
    #[must_use]
    pub fn roots(&self) -> &[SceneNodeId] {
        &self.roots
    }

    /// The node with this stable id.
    #[must_use]
    pub fn node(&self, id: &SceneNodeId) -> Option<&SceneNode> {
        self.by_id.get(id).map(|&position| &self.nodes[position])
    }

    /// The root whose authored name is `name`.
    ///
    /// # Errors
    ///
    /// [`SceneError::UnknownRoot`] when no root carries that authored name.
    pub fn root(&self, name: &str) -> Result<&SceneNode, SceneError> {
        self.roots
            .iter()
            .map(|id| self.node(id).expect("roots index into nodes"))
            .find(|node| node.name() == name)
            .ok_or_else(|| SceneError::UnknownRoot {
                name: name.to_owned(),
            })
    }

    /// The single root of a one-root container.
    ///
    /// # Errors
    ///
    /// [`SceneError::AmbiguousRoots`] when the container holds any other
    /// number of roots — the caller asked for *the* root and must say which
    /// one it means instead.
    pub fn single_root(&self) -> Result<&SceneNode, SceneError> {
        match self.roots.as_slice() {
            [id] => Ok(self.node(id).expect("roots index into nodes")),
            _ => Err(SceneError::AmbiguousRoots {
                roots: self.roots.len(),
            }),
        }
    }

    /// Every node of the subtree under `root`, in the graph's own root-first
    /// preorder.
    ///
    /// The set is walked by stable id from the root's children, so a subtree is
    /// the authored one and never "everything from an index onwards"; the order
    /// is the graph's, which keeps a parent ahead of its children. The one
    /// production definition of "the nodes under this root" lives here, so an
    /// importer and the roster audit cannot disagree about which nodes belong
    /// to an airframe.
    #[must_use]
    pub fn subtree(&self, root: &SceneNodeId) -> Vec<&SceneNode> {
        let mut included: HashSet<SceneNodeId> = HashSet::new();
        let mut pending = vec![root.clone()];
        while let Some(id) = pending.pop() {
            if !included.insert(id.clone()) {
                continue;
            }
            if let Some(node) = self.node(&id) {
                pending.extend(node.children().iter().rev().cloned());
            }
        }
        self.nodes
            .iter()
            .filter(|node| included.contains(node.id()))
            .collect()
    }

    /// The authored paths of the binding rules that matched no node. A rule
    /// that lands nowhere is reported, never silently dropped.
    #[must_use]
    pub fn unmatched_bindings(&self) -> &[String] {
        &self.unmatched_bindings
    }

    /// Every semantic socket in the container, ordered by stable id (which is
    /// the authored name-path, so the order is authored and deterministic).
    ///
    /// Sockets of a role the evidence left unknown are included: they are
    /// listed by [`Self::unresolved_sockets`] and never given a default role.
    pub fn sockets(&self) -> impl Iterator<Item = &PartSocket> + '_ {
        self.sockets.values()
    }

    /// The socket mounted on this node, when a rule bound it.
    #[must_use]
    pub fn socket(&self, node: &SceneNodeId) -> Option<&PartSocket> {
        self.sockets.get(node)
    }

    /// The sockets that serve one gameplay role, in stable-id order.
    pub fn sockets_of_role(&self, role: PartRole) -> impl Iterator<Item = &PartSocket> + '_ {
        self.sockets
            .values()
            .filter(move |socket| socket.known_role() == Some(role))
    }

    /// The sockets whose role the evidence left explicitly unknown, in
    /// stable-id order.
    ///
    /// A runtime consumer that needs a role must refuse these rather than
    /// assume one: the claim id and reason ride along in
    /// [`PartSocket::role`].
    pub fn unresolved_sockets(&self) -> impl Iterator<Item = &PartSocket> + '_ {
        self.sockets
            .values()
            .filter(|socket| !socket.role().is_known())
    }
}

/// The authored euler triple → source-space 3×3 convention of the CS GameZ
/// object record: `Rz·Ry·Rx` over negated angles, per the pinned reference
/// (`euler_to_matrix` in `mech3ax-nodes/src/math.rs`; observed-tool
/// evidence, not verified against the original executable).
fn euler_matrix(rotation: [f64; 3]) -> [[f64; 3]; 3] {
    let x = -rotation[0];
    let y = -rotation[1];
    let z = -rotation[2];
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    [
        [cy * cz, sx * sy * cz - cx * sz, cx * sy * cz + sx * sz],
        [cy * sz, sx * sy * sz + cx * cz, cx * sy * sz - sx * cz],
        [-sy, sx * cy, cx * cy],
    ]
}

const ROTATION_FIELDS: [&str; 3] = ["rotation[0]", "rotation[1]", "rotation[2]"];
const SCALE_FIELDS: [&str; 3] = ["scale[0]", "scale[1]", "scale[2]"];
const MATRIX_FIELDS: [&str; 9] = [
    "matrix[0]",
    "matrix[1]",
    "matrix[2]",
    "matrix[3]",
    "matrix[4]",
    "matrix[5]",
    "matrix[6]",
    "matrix[7]",
    "matrix[8]",
];
const TRANSLATION_SOURCE_FIELDS: [&str; 3] = ["translation[0]", "translation[1]", "translation[2]"];

/// Converts an authored transform into canonical space through `adapter`:
/// the linear map `rotation · scale` is built in source space — the stored
/// `matrix` wins over the euler triple when both exist — conjugated through
/// the axis map (`A·L·Aᵀ`), and the translation scaled by `meters_per_unit`.
/// The euler triple is in the source's declared angle unit, converted to
/// radians through the adapter exactly once.
fn canonical_local(
    transform: &AuthoredTransform,
    adapter: &SourceAdapter,
) -> Result<CanonicalTransform, SpaceError> {
    check_finite(transform.rotation.map(f64::from), ROTATION_FIELDS)?;
    check_finite(transform.scale.map(f64::from), SCALE_FIELDS)?;
    check_finite(
        transform.translation.map(f64::from),
        TRANSLATION_SOURCE_FIELDS,
    )?;
    if let Some(matrix) = &transform.matrix {
        let flat: [f64; 9] = [
            f64::from(matrix[0][0]),
            f64::from(matrix[0][1]),
            f64::from(matrix[0][2]),
            f64::from(matrix[1][0]),
            f64::from(matrix[1][1]),
            f64::from(matrix[1][2]),
            f64::from(matrix[2][0]),
            f64::from(matrix[2][1]),
            f64::from(matrix[2][2]),
        ];
        check_finite(flat, MATRIX_FIELDS)?;
    }

    // Source-space linear map: rotation · scale. The stored matrix wins over
    // the euler-derived one; the scale factors multiply its columns. The
    // euler triple is stored in the source's declared angle unit.
    let rotation_radians = transform.rotation.map(|angle| {
        adapter
            .angle_to_canonical(f64::from(angle))
            .expect("finiteness was checked above")
            .0
    });
    let rotation_matrix = match &transform.matrix {
        Some(matrix) => matrix.map(|row| row.map(f64::from)),
        None => euler_matrix(rotation_radians),
    };
    let mut linear = rotation_matrix;
    for (col, &scale) in transform.scale.iter().enumerate() {
        for row in &mut linear {
            row[col] *= f64::from(scale);
        }
    }

    // Conjugate through the adapter's signed permutation: the canonical
    // entry (i, j) picks the source entry at the mapped axes with both row
    // and column signs.
    let axes = adapter.source().convention().axes();
    let meters_per_unit = adapter.source().convention().meters_per_unit();
    let mut canonical = [[0.0; 3]; 3];
    let mut translation = [0.0; 3];
    for (canonical_axis, row) in axes.iter().enumerate() {
        let sign_i = row.sign.factor();
        let source_i = row.axis.index();
        for (canonical_j, col) in axes.iter().enumerate() {
            canonical[canonical_axis][canonical_j] =
                sign_i * col.sign.factor() * linear[source_i][col.axis.index()];
        }
        translation[canonical_axis] =
            meters_per_unit * sign_i * f64::from(transform.translation[source_i]);
    }

    CanonicalTransform::try_new(canonical, translation)
}

/// Converts the parsed kind into the canonical kind; LOD ranges become
/// meters through the adapter.
fn convert_kind(node: &ParsedNode, adapter: &SourceAdapter) -> Result<NodeKind, SceneError> {
    Ok(match node.kind {
        ParsedNodeKind::World => NodeKind::World,
        ParsedNodeKind::Window => NodeKind::Window,
        ParsedNodeKind::Display => NodeKind::Display,
        ParsedNodeKind::Camera => NodeKind::Camera,
        ParsedNodeKind::Light => NodeKind::Light,
        ParsedNodeKind::Object3d => NodeKind::Object3d,
        ParsedNodeKind::Lod {
            level,
            range_min,
            range_max,
        } => {
            // `range_min` arrives resolved (the record stores it squared).
            // A negative or non-finite bound is corrupt input, and a
            // reversed range is refused — stricter than the reference,
            // which bounds the near value but never compares it to the far.
            // Refusing is safe here only because no measured data has been
            // seen to rely on a reversed range; that stays a recorded
            // unknown, not a verified fact.
            if !(range_min.is_finite() && range_max.is_finite())
                || range_min < 0.0
                || range_min > range_max
            {
                return Err(SceneError::LodRange {
                    node: node.index,
                    range_min,
                    range_max,
                });
            }
            NodeKind::Lod(LodInfo {
                level,
                range_min: adapter
                    .distance_to_canonical(f64::from(range_min))
                    .map_err(|_| SceneError::LodRange {
                        node: node.index,
                        range_min,
                        range_max,
                    })?,
                range_max: adapter
                    .distance_to_canonical(f64::from(range_max))
                    .map_err(|_| SceneError::LodRange {
                        node: node.index,
                        range_min,
                        range_max,
                    })?,
            })
        }
    })
}

/// Attaches the one rule naming this node's authored path, marking it
/// matched; animation channels must resolve to `animation_track` ids.
fn attach_binding(
    node: &ParsedNode,
    path: &str,
    bindings: &BindingMap,
    matched: &mut [bool],
) -> Result<Option<SemanticBinding>, SceneError> {
    let Some((rule_index, rule)) = bindings
        .rules()
        .iter()
        .enumerate()
        .find(|(_, rule)| rule.path == path)
    else {
        return Ok(None);
    };
    for channel in &rule.animation {
        if let Resolved::Known(known) = &channel.channel
            && known.value.kind() != ContentKind::AnimationTrack
        {
            return Err(SceneError::AnimationChannelKind {
                node: node.index,
                kind: known.value.kind(),
            });
        }
    }
    matched[rule_index] = true;
    Ok(Some(rule.clone()))
}

// --------------------------------------------------------- LOD selection ---

/// How [`select_lod_variant`] arrived at its choice.
///
/// The coverage rides along with every result so a fallback is observable:
/// a caller can tell "the authored bands say so" from "the rule papered over
/// a gap" instead of silently drawing whatever came back.
///
/// The vocabulary is designed engine contract
/// (`specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`, F11-B): the
/// original's band-selection behaviour and the meaning of [`LodInfo::level`]
/// are unmeasured, so nothing here claims to reproduce them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LodCoverage {
    /// Exactly one variant's range contains the distance.
    Covered,
    /// More than one variant's range contains the distance. Adjacent bands
    /// share their boundary (`0..500` and `500..2000` both contain 500), so a
    /// distance on a shared edge covers both; the tightest band wins.
    Overlap,
    /// No variant's range contains the distance — the gap sits below, between
    /// or above the authored bands — and the nearest band was chosen, so a gap
    /// never blanks the part out of existence.
    GapFallback,
}

/// One LOD selection result: which member of a variant group is presented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LodChoice {
    /// The chosen variant's index into the slice the rule was given, in
    /// stored child order.
    pub index: usize,
    /// How the choice was reached.
    pub coverage: LodCoverage,
}

/// Why a group of LOD variants could not be resolved to one choice.
///
/// The rule refuses unusable input rather than picking something from it: a
/// NaN distance or a reversed band would otherwise decide presentation from
/// numbers that mean nothing.
#[derive(Clone, Debug, PartialEq)]
pub enum LodSelectError {
    /// The group held no variants at all.
    NoVariants,
    /// The distance was not a finite, non-negative number of metres.
    Distance,
    /// Variant `index` declares a range that is non-finite, negative or
    /// reversed (`range_min > range_max`).
    Range {
        /// The offending variant's index in the slice.
        index: usize,
    },
}

impl fmt::Display for LodSelectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoVariants => write!(f, "the LOD group has no variants to select from"),
            Self::Distance => write!(
                f,
                "the LOD selection distance must be finite and non-negative"
            ),
            Self::Range { index } => write!(
                f,
                "LOD variant {index} declares a non-finite, negative or reversed range"
            ),
        }
    }
}

impl std::error::Error for LodSelectError {}

/// Picks the one variant of a group that is presented at `distance`.
///
/// The group is the `Lod` siblings that share a parent: each stands for the
/// same physical part at a different distance, and exactly one of them may be
/// presented (F11 non-negotiable behavior 4 — LOD is presentation only).
///
/// The rule, in order:
///
/// 1. a band *covers* the distance when `range_min <= distance <=
///    range_max`;
/// 2. exactly one covering band → [`LodCoverage::Covered`];
/// 3. several covering bands → [`LodCoverage::Overlap`], the tightest band
///    wins (smallest `range_max`, then largest `range_min`, then stored
///    order);
/// 4. no covering band → [`LodCoverage::GapFallback`], the band nearest the
///    distance wins (ties → lower `range_min` → stored order).
///
/// This is designed engine contract, not measured original behaviour: the
/// original's selection rule, its edge convention and `LodInfo::level` remain
/// unmeasured (recorded in
/// `docs/findings/2026-09-29-f11-b-hierarchy-import-and-lod-selection.md`).
///
/// # Errors
///
/// [`LodSelectError::NoVariants`] when `variants` is empty,
/// [`LodSelectError::Distance`] when `distance` is not finite and
/// non-negative, and [`LodSelectError::Range`] naming the first variant whose
/// range is unusable.
pub fn select_lod_variant(
    variants: &[LodInfo],
    distance: Meters,
) -> Result<LodChoice, LodSelectError> {
    if !distance.0.is_finite() || distance.0 < 0.0 {
        return Err(LodSelectError::Distance);
    }
    if variants.is_empty() {
        return Err(LodSelectError::NoVariants);
    }
    for (index, variant) in variants.iter().enumerate() {
        let (min, max) = (variant.range_min.0, variant.range_max.0);
        if !min.is_finite() || !max.is_finite() || min < 0.0 || min > max {
            return Err(LodSelectError::Range { index });
        }
    }

    let target = distance.0;
    let covering: Vec<usize> = variants
        .iter()
        .enumerate()
        .filter(|(_, variant)| variant.range_min.0 <= target && target <= variant.range_max.0)
        .map(|(index, _)| index)
        .collect();

    match covering.as_slice() {
        [index] => Ok(LodChoice {
            index: *index,
            coverage: LodCoverage::Covered,
        }),
        [] => {
            // No band contains the distance: take the band whose range is
            // nearest it, so an authored gap cannot blank the part. Ties go
            // to the lower near bound and then to stored order, which makes
            // the fallback reproducible.
            let mut best = (
                gap_distance(variants[0], target),
                variants[0].range_min.0,
                0usize,
            );
            for (index, variant) in variants.iter().enumerate().skip(1) {
                let candidate = (gap_distance(*variant, target), variant.range_min.0, index);
                if candidate < best {
                    best = candidate;
                }
            }
            Ok(LodChoice {
                index: best.2,
                coverage: LodCoverage::GapFallback,
            })
        }
        several => {
            // Several bands contain the distance: the tightest far bound wins,
            // then the highest near bound, then stored order.
            let mut best = (
                variants[several[0]].range_max.0,
                -variants[several[0]].range_min.0,
            );
            let mut chosen = several[0];
            for &index in several.iter().skip(1) {
                let variant = variants[index];
                let candidate = (variant.range_max.0, -variant.range_min.0);
                if candidate < best {
                    best = candidate;
                    chosen = index;
                }
            }
            Ok(LodChoice {
                index: chosen,
                coverage: LodCoverage::Overlap,
            })
        }
    }
}

/// How far `target` lies outside `variant`'s band — zero inside it, since a
/// band that already contains the distance is never scored here.
fn gap_distance(variant: LodInfo, target: f64) -> f64 {
    if target < variant.range_min.0 {
        variant.range_min.0 - target
    } else if target > variant.range_max.0 {
        target - variant.range_max.0
    } else {
        0.0
    }
}

// ------------------------------------------------------------ roster audit ---

/// The claim id this stage uses when it refuses to state that an airframe is
/// player-selectable anywhere (F11 non-negotiable behavior 3).
const AVAILABILITY_UNDISCOVERED: &str = "f11d.roster-availability-undiscovered";

/// What the evidence establishes about player-selectability.
///
/// The vocabulary is designed engine contract. Nothing here claims the
/// original's roster rules: a mode's own selection list is unmeasured, so a
/// value only exists when some discovery recorded it, with its provenance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RosterAvailability {
    /// Discovered as selectable by a player in at least one mode.
    Selectable,
    /// Discovered only as an airframe a mission forces into play; **not**
    /// proven selectable in any mode.
    MissionOnly,
}

/// A mission that forces one airframe into play — the discovery that is
/// recorded **separately** from roster availability.
///
/// An assignment says "this mission puts this airframe in the air"; it never
/// says a player may choose it, and [`AirframeRoster::audit`] never promotes
/// one into [`RosterAvailability::Selectable`]. Keeping the two apart is what
/// makes a mission-only type identifiable at all (F11 non-negotiable
/// behavior 3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForcedMissionAssignment {
    mission: ContentId,
    airframe: ContentId,
    provenance: Provenance,
}

impl ForcedMissionAssignment {
    /// Records that `mission` forces `airframe` into play.
    ///
    /// # Errors
    ///
    /// [`RosterError::MissionKind`] when `mission` is not a `mission` id and
    /// [`RosterError::AirframeKind`] when `airframe` is not an `airframe` id.
    pub fn new(
        mission: ContentId,
        airframe: ContentId,
        provenance: Provenance,
    ) -> Result<Self, RosterError> {
        if mission.kind() != ContentKind::Mission {
            return Err(RosterError::MissionKind {
                kind: mission.kind(),
            });
        }
        if airframe.kind() != ContentKind::Airframe {
            return Err(RosterError::AirframeKind {
                kind: airframe.kind(),
            });
        }
        Ok(Self {
            mission,
            airframe,
            provenance,
        })
    }

    /// The mission that forces the airframe.
    #[must_use]
    pub fn mission(&self) -> &ContentId {
        &self.mission
    }

    /// The forced airframe.
    #[must_use]
    pub fn airframe(&self) -> &ContentId {
        &self.airframe
    }

    /// Where this assignment was discovered.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// One discovered airframe: the catalog element, the root its visual
/// references, what is known about its roster availability, and the gameplay
/// roles it is **declared** to carry.
///
/// `required_roles` is declared, not inferred: the audit checks that each
/// declared role really is bound to a node, and says nothing about a role
/// nobody claimed. An empty list therefore audits "this root's bindings", not
/// "this root is complete" — the difference is a finding, not a pass.
#[derive(Clone, Debug, PartialEq)]
pub struct RosterEntry {
    airframe: ContentId,
    root: Option<SceneRootRef>,
    availability: Resolved<RosterAvailability>,
    required_roles: Vec<PartRole>,
    provenance: Provenance,
}

impl RosterEntry {
    /// An airframe whose root, availability and required roles are all still
    /// to be discovered.
    ///
    /// # Errors
    ///
    /// [`RosterError::AirframeKind`] when `airframe` is not an `airframe` id.
    pub fn new(airframe: ContentId, provenance: Provenance) -> Result<Self, RosterError> {
        if airframe.kind() != ContentKind::Airframe {
            return Err(RosterError::AirframeKind {
                kind: airframe.kind(),
            });
        }
        Ok(Self {
            airframe,
            root: None,
            availability: Self::undiscovered_availability(),
            required_roles: Vec::new(),
            provenance,
        })
    }

    /// The standing explicit unknown for roster availability: a model name is
    /// not proof that a plane is selectable in any mode (F11 non-negotiable
    /// behavior 3), and an unevidenced roster is `Unknown` rather than empty.
    #[must_use]
    pub fn undiscovered_availability() -> Resolved<RosterAvailability> {
        Resolved::unknown(
            ClaimId::new(AVAILABILITY_UNDISCOVERED).expect("the claim id is valid"),
            "roster availability is a separate discovery from a forced mission \
             assignment; nothing has established it for this airframe",
        )
        .expect("the reason is not empty")
    }

    /// Records the root this airframe's visual references.
    #[must_use]
    pub fn with_root(mut self, root: SceneRootRef) -> Self {
        self.root = Some(root);
        self
    }

    /// Records what the evidence established about roster availability.
    #[must_use]
    pub fn with_availability(mut self, availability: Resolved<RosterAvailability>) -> Self {
        self.availability = availability;
        self
    }

    /// Declares that this airframe must bind `role` to a node.
    ///
    /// # Errors
    ///
    /// [`RosterError::DuplicateRequiredRole`] when the role is already
    /// declared — a repeated requirement is an authoring error, not a second
    /// required mount.
    pub fn requiring(mut self, role: PartRole) -> Result<Self, RosterError> {
        if self.required_roles.contains(&role) {
            return Err(RosterError::DuplicateRequiredRole { role });
        }
        self.required_roles.push(role);
        Ok(self)
    }

    /// The airframe element this row audits.
    #[must_use]
    pub fn airframe(&self) -> &ContentId {
        &self.airframe
    }

    /// The checked root reference, when one has been discovered.
    #[must_use]
    pub fn root(&self) -> Option<&SceneRootRef> {
        self.root.as_ref()
    }

    /// What the evidence established about roster availability, or the
    /// explicit unknown that says nothing has.
    #[must_use]
    pub fn availability(&self) -> &Resolved<RosterAvailability> {
        &self.availability
    }

    /// The roles this airframe is declared to bind, in declaration order.
    #[must_use]
    pub fn required_roles(&self) -> &[PartRole] {
        &self.required_roles
    }

    /// Where this row was discovered.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// One discovered scene container the roster audit covers, with the node
/// array facts its own header declares.
///
/// `stored_nodes` is the container header's `node_array_size` and
/// `nodes_offset` the byte offset the node array starts at — the two measured
/// words that say how much scene data exists and where. They travel with the
/// container so a blocker can quote them, and so a converted graph can be
/// checked against the number of records the container claims to hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneContainerRef {
    container: ContentId,
    stored_nodes: u32,
    nodes_offset: u32,
}

impl SceneContainerRef {
    /// Names a container and the node array its header declares.
    #[must_use]
    pub fn new(container: ContentId, stored_nodes: u32, nodes_offset: u32) -> Self {
        Self {
            container,
            stored_nodes,
            nodes_offset,
        }
    }

    /// The container's catalog id.
    #[must_use]
    pub fn container(&self) -> &ContentId {
        &self.container
    }

    /// The header's `node_array_size`: stored node records in this container.
    #[must_use]
    pub fn stored_nodes(&self) -> u32 {
        self.stored_nodes
    }

    /// The header's `nodes_offset`: where the node array starts.
    #[must_use]
    pub fn nodes_offset(&self) -> u32 {
        self.nodes_offset
    }
}

/// Why a container's scene could not be handed to the audit at all.
///
/// The variants carry the *measured* facts, not a message: a container whose
/// node array is undecoded reports how many stored node records and at what
/// offset, so the missing step is a number someone can act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContainerBlocker {
    /// The node array has not been decoded, so no root, part, mount or
    /// cockpit binding inside it exists yet.
    NodeArrayUndecoded {
        /// The container that still holds it.
        container: ContentId,
        /// The header's `node_array_size`.
        stored_nodes: u32,
        /// The header's `nodes_offset`.
        nodes_offset: u32,
    },
    /// The container's conversion was refused.
    SceneRefused {
        /// The container that was refused.
        container: ContentId,
        /// The refusal, verbatim.
        reason: String,
    },
}

impl ContainerBlocker {
    /// The container this blocker is about.
    #[must_use]
    pub fn container(&self) -> &ContentId {
        match self {
            Self::NodeArrayUndecoded { container, .. } | Self::SceneRefused { container, .. } => {
                container
            }
        }
    }
}

impl fmt::Display for ContainerBlocker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NodeArrayUndecoded {
                container,
                stored_nodes,
                nodes_offset,
            } => write!(
                f,
                "{container} declares {stored_nodes} stored node records at offset \
                 {nodes_offset}, and its node array is not decoded: no root, part, mount \
                 or cockpit binding can be mapped from it"
            ),
            Self::SceneRefused { container, reason } => {
                write!(f, "{container} could not be converted: {reason}")
            }
        }
    }
}

/// Why one airframe could not be mapped, whatever the container holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AirframeBlocker {
    /// No root reference has been discovered for this airframe, so there is
    /// nothing to walk. A model name is not a root.
    RootUndiscovered {
        /// The airframe awaiting a root.
        airframe: ContentId,
    },
    /// The container holding the root could not be converted.
    ContainerUndecoded {
        /// The airframe inside it.
        airframe: ContentId,
        /// The container's own blocker, carried verbatim.
        blocker: ContainerBlocker,
    },
    /// The referenced root is not in the converted container.
    RootMissing {
        /// The airframe that referenced it.
        airframe: ContentId,
        /// The missing root.
        root: SceneNodeId,
    },
    /// The referenced root's container is not among the audited containers, so
    /// the audit never looked at it.
    ContainerNotAudited {
        /// The airframe inside it.
        airframe: ContentId,
        /// The container the audit was not asked to cover.
        container: ContentId,
    },
}

impl AirframeBlocker {
    /// The airframe this blocker is about.
    #[must_use]
    pub fn airframe(&self) -> &ContentId {
        match self {
            Self::RootUndiscovered { airframe }
            | Self::ContainerUndecoded { airframe, .. }
            | Self::RootMissing { airframe, .. }
            | Self::ContainerNotAudited { airframe, .. } => airframe,
        }
    }
}

impl fmt::Display for AirframeBlocker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootUndiscovered { airframe } => {
                write!(f, "{airframe} has no discovered scene root")
            }
            Self::ContainerUndecoded { airframe, blocker } => {
                write!(f, "{airframe} is blocked: {blocker}")
            }
            Self::RootMissing { airframe, root } => {
                write!(
                    f,
                    "{airframe} references root {root}, which the container does not hold"
                )
            }
            Self::ContainerNotAudited {
                airframe,
                container,
            } => write!(
                f,
                "{airframe} lives in {container}, which this audit did not cover"
            ),
        }
    }
}

/// A shortfall the audit found inside an airframe it *could* walk.
///
/// A gap is not a blocker: the mapping happened, and this is what is missing
/// from it. Each variant names the affected content so it can be filed.
#[derive(Clone, Debug, PartialEq)]
pub enum AuditGap {
    /// A bound node's gameplay role is an explicit unknown, so a consumer
    /// holding that socket must refuse it.
    UnknownRole {
        /// The bound node.
        node: SceneNodeId,
        /// The claim that records the unknown.
        claim_id: ClaimId,
        /// Why it is unknown.
        reason: String,
    },
    /// A binding rule matched no node of the container.
    UnmatchedRule {
        /// The authored path that named nothing.
        path: String,
    },
    /// A role the airframe is declared to carry is bound to nothing.
    MissingRole {
        /// The root whose subtree was audited.
        root: SceneNodeId,
        /// The role nothing bound.
        role: PartRole,
    },
    /// Roster availability was never discovered for this airframe.
    AvailabilityUndiscovered {
        /// The airframe.
        airframe: ContentId,
    },
    /// Forced mission assignments are the **only** evidence this airframe
    /// exists; nothing established where a player may select it.
    ForcedAssignmentOnly {
        /// The airframe.
        airframe: ContentId,
        /// How many missions force it.
        missions: usize,
    },
    /// The converted graph holds a different number of nodes than the
    /// container's header declares, so the node array was only partly decoded.
    NodeCountMismatch {
        /// The container.
        container: ContentId,
        /// The header's `node_array_size`.
        declared: u32,
        /// How many nodes the converted graph really holds.
        decoded: usize,
    },
}

impl fmt::Display for AuditGap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownRole {
                node,
                claim_id,
                reason,
            } => write!(
                f,
                "{node} has no established gameplay role ({claim_id}): {reason}"
            ),
            Self::UnmatchedRule { path } => {
                write!(f, "the binding rule for {path:?} matched no node")
            }
            Self::MissingRole { root, role } => {
                write!(f, "no node under {root} is bound as {}", role.label())
            }
            Self::AvailabilityUndiscovered { airframe } => {
                write!(f, "{airframe} has no discovered roster availability")
            }
            Self::ForcedAssignmentOnly { airframe, missions } => write!(
                f,
                "{airframe} is only evidenced as forced by {missions} mission(s); its roster \
                 availability is still unknown"
            ),
            Self::NodeCountMismatch {
                container,
                declared,
                decoded,
            } => write!(
                f,
                "{container} declares {declared} stored node records but the converted graph \
                 holds {decoded}"
            ),
        }
    }
}

/// One socket the audit mapped: which node, which role, its collision role,
/// stored zone, bound animation channels, the pose the node's own composed
/// transform holds, and the provenance of the rule that bound it.
///
/// `pose` is a copy of the node's one composed [`CanonicalTransform`] — the
/// same value the render and collision paths use — so a mount point read out
/// of an audit report cannot drift from the scene it describes (F11
/// non-negotiable behavior 4).
#[derive(Clone, Debug, PartialEq)]
pub struct MappedSocket {
    node: SceneNodeId,
    role: PartRole,
    collision: CollisionRole,
    pose: CanonicalTransform,
    zone_id: u32,
    animation_channels: usize,
    provenance: Provenance,
}

impl MappedSocket {
    /// The bound node.
    #[must_use]
    pub fn node(&self) -> &SceneNodeId {
        &self.node
    }

    /// The established gameplay role.
    #[must_use]
    pub const fn role(&self) -> PartRole {
        self.role
    }

    /// The established collision role.
    #[must_use]
    pub const fn collision(&self) -> CollisionRole {
        self.collision
    }

    /// The node's one composed pose, shared with the render and collision
    /// paths.
    #[must_use]
    pub const fn pose(&self) -> &CanonicalTransform {
        &self.pose
    }

    /// The bound node's stored zone id, uninterpreted.
    #[must_use]
    pub const fn zone_id(&self) -> u32 {
        self.zone_id
    }

    /// How many animation channels the rule bound to the node.
    #[must_use]
    pub const fn animation_channels(&self) -> usize {
        self.animation_channels
    }

    /// The provenance of the rule that produced this mapping.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// What the audit mapped for one airframe's root.
#[derive(Clone, Debug, PartialEq)]
pub struct AirframeMapping {
    root: SceneNodeId,
    container: ContentId,
    node_count: usize,
    sockets: Vec<MappedSocket>,
}

impl AirframeMapping {
    /// The audited root.
    #[must_use]
    pub fn root(&self) -> &SceneNodeId {
        &self.root
    }

    /// The container the root lives in.
    #[must_use]
    pub fn container(&self) -> &ContentId {
        &self.container
    }

    /// How many nodes the mapped subtree holds.
    #[must_use]
    pub const fn node_count(&self) -> usize {
        self.node_count
    }

    /// Every mapped socket, in stable-id order.
    pub fn sockets(&self) -> impl Iterator<Item = &MappedSocket> + '_ {
        self.sockets.iter()
    }

    /// The sockets that serve one role, in stable-id order.
    pub fn sockets_of_role(&self, role: PartRole) -> impl Iterator<Item = &MappedSocket> + '_ {
        self.sockets
            .iter()
            .filter(move |socket| socket.role == role)
    }

    /// How many sockets serve one role.
    #[must_use]
    pub fn count_of(&self, role: PartRole) -> usize {
        self.sockets_of_role(role).count()
    }

    /// How many sockets were mapped.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.sockets.len()
    }

    /// Whether nothing was mapped.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sockets.is_empty()
    }
}

/// One audited airframe: its declared row, what the audit mapped and what it
/// could not.
#[derive(Clone, Debug, PartialEq)]
pub struct AirframeAudit {
    airframe: ContentId,
    root: Option<SceneRootRef>,
    availability: Resolved<RosterAvailability>,
    forced_missions: Vec<ContentId>,
    mapping: Option<AirframeMapping>,
    blockers: Vec<AirframeBlocker>,
    gaps: Vec<AuditGap>,
}

impl AirframeAudit {
    /// The audited airframe.
    #[must_use]
    pub fn airframe(&self) -> &ContentId {
        &self.airframe
    }

    /// The declared root reference, when the roster row carried one.
    #[must_use]
    pub fn root(&self) -> Option<&SceneRootRef> {
        self.root.as_ref()
    }

    /// What the evidence established about roster availability. A forced
    /// mission assignment never changes it.
    #[must_use]
    pub fn availability(&self) -> &Resolved<RosterAvailability> {
        &self.availability
    }

    /// Whether the evidence proved this airframe selectable in some mode.
    #[must_use]
    pub fn is_proven_selectable(&self) -> bool {
        matches!(
            &self.availability,
            Resolved::Known(known) if known.value == RosterAvailability::Selectable
        )
    }

    /// The missions that force this airframe into play, in roster order.
    #[must_use]
    pub fn forced_missions(&self) -> &[ContentId] {
        &self.forced_missions
    }

    /// What the audit mapped, when it could.
    #[must_use]
    pub fn mapping(&self) -> Option<&AirframeMapping> {
        self.mapping.as_ref()
    }

    /// Why the airframe could not be mapped.
    pub fn blockers(&self) -> impl Iterator<Item = &AirframeBlocker> + '_ {
        self.blockers.iter()
    }

    /// The first blocker, which is the one that stopped the mapping.
    #[must_use]
    pub fn first_blocker(&self) -> Option<&AirframeBlocker> {
        self.blockers.first()
    }

    /// The shortfalls found inside the mapping.
    pub fn gaps(&self) -> impl Iterator<Item = &AuditGap> + '_ {
        self.gaps.iter()
    }

    /// The first gap, for a report that names one reason.
    #[must_use]
    pub fn first_gap(&self) -> Option<&AuditGap> {
        self.gaps.first()
    }

    /// Whether this airframe was fully mapped with nothing missing.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.mapping.is_some() && self.blockers.is_empty() && self.gaps.is_empty()
    }
}

/// What the audit mapped for one container.
#[derive(Clone, Debug, PartialEq)]
pub struct ContainerMapping {
    container: ContentId,
    roots: Vec<SceneNodeId>,
    node_count: usize,
    airframes: Vec<ContentId>,
}

impl ContainerMapping {
    /// The container.
    #[must_use]
    pub fn container(&self) -> &ContentId {
        &self.container
    }

    /// The container's roots, in stored order.
    #[must_use]
    pub fn roots(&self) -> &[SceneNodeId] {
        &self.roots
    }

    /// How many nodes the converted graph holds.
    #[must_use]
    pub const fn node_count(&self) -> usize {
        self.node_count
    }

    /// The airframes whose discovered root lives in this container.
    #[must_use]
    pub fn airframes(&self) -> &[ContentId] {
        &self.airframes
    }
}

/// How one audited container turned out.
#[derive(Clone, Debug, PartialEq)]
pub enum ContainerOutcome {
    /// The container was converted and mapped.
    Mapped(ContainerMapping),
    /// The container could not be converted.
    Blocked(ContainerBlocker),
}

/// One audited container: either the mapping it produced or the blocker that
/// stopped it.
#[derive(Clone, Debug, PartialEq)]
pub struct ContainerAudit {
    container: ContentId,
    declared_nodes: u32,
    nodes_offset: u32,
    outcome: ContainerOutcome,
    gaps: Vec<AuditGap>,
}

impl ContainerAudit {
    /// The audited container.
    #[must_use]
    pub fn container(&self) -> &ContentId {
        &self.container
    }

    /// The header's `node_array_size` for this container.
    #[must_use]
    pub const fn declared_nodes(&self) -> u32 {
        self.declared_nodes
    }

    /// The header's `nodes_offset` for this container.
    #[must_use]
    pub const fn nodes_offset(&self) -> u32 {
        self.nodes_offset
    }

    /// The container's outcome.
    #[must_use]
    pub fn outcome(&self) -> &ContainerOutcome {
        &self.outcome
    }

    /// The mapping, when there is one.
    #[must_use]
    pub fn mapping(&self) -> Option<&ContainerMapping> {
        match &self.outcome {
            ContainerOutcome::Mapped(mapping) => Some(mapping),
            ContainerOutcome::Blocked(_) => None,
        }
    }

    /// The blocker, when there is one.
    #[must_use]
    pub fn blocker(&self) -> Option<&ContainerBlocker> {
        match &self.outcome {
            ContainerOutcome::Mapped(_) => None,
            ContainerOutcome::Blocked(blocker) => Some(blocker),
        }
    }

    /// Whether the container was converted.
    #[must_use]
    pub fn is_mapped(&self) -> bool {
        matches!(self.outcome, ContainerOutcome::Mapped(_))
    }

    /// The shortfalls found inside the container.
    pub fn gaps(&self) -> impl Iterator<Item = &AuditGap> + '_ {
        self.gaps.iter()
    }
}

/// The whole verdict: every audited container and every audited airframe, in
/// the order they were given.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RosterAuditReport {
    containers: Vec<ContainerAudit>,
    airframes: Vec<AirframeAudit>,
}

impl RosterAuditReport {
    /// Every audited container, in the order the audit was given them.
    #[must_use]
    pub fn containers(&self) -> &[ContainerAudit] {
        &self.containers
    }

    /// Every audited airframe, in roster order.
    #[must_use]
    pub fn airframes(&self) -> &[AirframeAudit] {
        &self.airframes
    }

    /// The containers that were converted.
    pub fn mapped_containers(&self) -> impl Iterator<Item = &ContainerAudit> + '_ {
        self.containers.iter().filter(|audit| audit.is_mapped())
    }

    /// The containers that could not be converted.
    pub fn blocked_containers(&self) -> impl Iterator<Item = &ContainerAudit> + '_ {
        self.containers.iter().filter(|audit| !audit.is_mapped())
    }

    /// The airframes that were mapped.
    pub fn mapped_airframes(&self) -> impl Iterator<Item = &AirframeAudit> + '_ {
        self.airframes
            .iter()
            .filter(|audit| audit.mapping.is_some())
    }

    /// The airframes that could not be mapped.
    pub fn blocked_airframes(&self) -> impl Iterator<Item = &AirframeAudit> + '_ {
        self.airframes
            .iter()
            .filter(|audit| audit.mapping.is_none())
    }

    /// How many containers were audited.
    #[must_use]
    pub fn container_count(&self) -> usize {
        self.containers.len()
    }

    /// How many airframes were audited.
    #[must_use]
    pub fn airframe_count(&self) -> usize {
        self.airframes.len()
    }

    /// How many roots were reached and mapped: one per mapped airframe, so two
    /// roster rows that name the same root are counted as the two mappings they
    /// are. [`Self::mapped_containers`] is the count of distinct containers.
    #[must_use]
    pub fn mapped_root_count(&self) -> usize {
        self.airframes
            .iter()
            .filter_map(|audit| audit.mapping())
            .count()
    }

    /// How many sockets were mapped across every audited airframe.
    #[must_use]
    pub fn mapped_socket_count(&self) -> usize {
        self.airframes
            .iter()
            .filter_map(|audit| audit.mapping())
            .map(AirframeMapping::len)
            .sum()
    }

    /// How many blockers the report holds, over containers and airframes.
    #[must_use]
    pub fn blocker_count(&self) -> usize {
        self.containers
            .iter()
            .filter(|audit| audit.blocker().is_some())
            .count()
            + self
                .airframes
                .iter()
                .map(|audit| audit.blockers().count())
                .sum::<usize>()
    }

    /// How many gaps the report holds, over containers and airframes.
    #[must_use]
    pub fn gap_count(&self) -> usize {
        self.containers
            .iter()
            .map(|audit| audit.gaps().count())
            .sum::<usize>()
            + self
                .airframes
                .iter()
                .map(|audit| audit.gaps().count())
                .sum::<usize>()
    }

    /// Whether the audit mapped every discovered root with nothing missing.
    ///
    /// This is deliberately strict: an audit of no container and no airframe
    /// mapped nothing and is **not** a pass, so an empty report is
    /// incomplete. Neither is a report with a blocker or a gap.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        !self.containers.is_empty()
            && !self.airframes.is_empty()
            && self.blocker_count() == 0
            && self.gap_count() == 0
    }

    /// Whether the audit looked at nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.containers.is_empty() && self.airframes.is_empty()
    }
}

/// The claim and reason behind a socket the audit refuses to map, or `None`
/// when both of its roles are established.
///
/// The gameplay role's own unknown wins when it is unknown; otherwise it is
/// the collision role's. The reported claim is always the *unresolved* one, so
/// a gap never points a reader at the rule that did resolve.
fn unresolved_socket_claim(socket: &PartSocket) -> Option<(ClaimId, String)> {
    match (socket.role(), socket.collision()) {
        (Resolved::Unknown { claim_id, reason }, _)
        | (_, Resolved::Unknown { claim_id, reason }) => Some((claim_id.clone(), reason.clone())),
        (Resolved::Known(_), Resolved::Known(_)) => None,
    }
}

/// The declared roster: every discovered airframe and every forced mission
/// assignment, validated as one set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AirframeRoster {
    entries: Vec<RosterEntry>,
    assignments: Vec<ForcedMissionAssignment>,
}

impl AirframeRoster {
    /// Collects the roster, refusing contradictions.
    ///
    /// # Errors
    ///
    /// [`RosterError::DuplicateAirframe`] when two rows audit the same
    /// airframe, [`RosterError::DuplicateAssignment`] when one mission
    /// forces the same airframe twice, and
    /// [`RosterError::UnknownAirframe`] when an assignment names an airframe
    /// no row audits — a forced plane that is missing from the roster is a
    /// discovery gap, and hiding it behind an assignment would make the
    /// roster look complete.
    pub fn new(
        entries: Vec<RosterEntry>,
        assignments: Vec<ForcedMissionAssignment>,
    ) -> Result<Self, RosterError> {
        let mut known: HashSet<ContentId> = HashSet::new();
        for entry in &entries {
            if !known.insert(entry.airframe.clone()) {
                return Err(RosterError::DuplicateAirframe {
                    airframe: entry.airframe.clone(),
                });
            }
        }
        let mut seen: HashSet<(ContentId, ContentId)> = HashSet::new();
        for assignment in &assignments {
            if !known.contains(&assignment.airframe) {
                return Err(RosterError::UnknownAirframe {
                    airframe: assignment.airframe.clone(),
                });
            }
            if !seen.insert((assignment.mission.clone(), assignment.airframe.clone())) {
                return Err(RosterError::DuplicateAssignment {
                    mission: assignment.mission.clone(),
                    airframe: assignment.airframe.clone(),
                });
            }
        }
        Ok(Self {
            entries,
            assignments,
        })
    }

    /// The roster rows, in supplied order.
    #[must_use]
    pub fn entries(&self) -> &[RosterEntry] {
        &self.entries
    }

    /// The forced mission assignments, in supplied order.
    #[must_use]
    pub fn assignments(&self) -> &[ForcedMissionAssignment] {
        &self.assignments
    }

    /// The row auditing this airframe.
    #[must_use]
    pub fn entry(&self, airframe: &ContentId) -> Option<&RosterEntry> {
        self.entries
            .iter()
            .find(|entry| &entry.airframe == airframe)
    }

    /// The missions that force one airframe into play, in roster order.
    pub fn missions_forcing<'a>(
        &'a self,
        airframe: &'a ContentId,
    ) -> impl Iterator<Item = &'a ContentId> + 'a {
        self.assignments
            .iter()
            .filter(move |assignment| &assignment.airframe == airframe)
            .map(|assignment| &assignment.mission)
    }

    /// The airframes one mission forces into play, in roster order.
    pub fn airframes_forced_in<'a>(
        &'a self,
        mission: &'a ContentId,
    ) -> impl Iterator<Item = &'a ContentId> + 'a {
        self.assignments
            .iter()
            .filter(move |assignment| &assignment.mission == mission)
            .map(|assignment| &assignment.airframe)
    }

    /// Audits the roster against the discovered containers.
    ///
    /// `graph_of` hands the audit the converted graph of one container, or the
    /// typed reason it cannot. The closure is the seam between this contract
    /// and the format layer: today no production path decodes a GameZ node
    /// array, so a retail caller answers
    /// [`ContainerBlocker::NodeArrayUndecoded`] with the measured
    /// `node_array_size` and `nodes_offset`; when a reader exists it hands
    /// over the converted graph instead and the same audit maps it. Nothing
    /// else changes, which is what keeps "blocked" and "mapped" the same
    /// verdict with a different input rather than two different reports.
    ///
    /// It is called about a container once for that container's own verdict and
    /// once more for every airframe whose root lives in it, so the source must
    /// answer the same way each time: a graph that is there on the first call
    /// and gone on the second is a contract violation, and the audit trips its
    /// own consistency check instead of reporting a pass.
    ///
    /// It is asked about a container once for that container's own verdict and
    /// once more for every airframe whose root lives in it, so a source must
    /// answer the same way every time: a graph that is there on the first call
    /// and gone on the second is a contract violation, and the audit trips its
    /// own consistency check rather than reporting a pass.
    ///
    /// The verdict is per airframe and per container, and both are kept: a
    /// container can be converted while one of its airframes still has no
    /// root, and a blocked container blocks every airframe inside it.
    pub fn audit<'g, G>(&self, containers: &[SceneContainerRef], graph_of: G) -> RosterAuditReport
    where
        G: Fn(&ContentId) -> Result<&'g SceneGraph, ContainerBlocker>,
    {
        let mut audits: Vec<ContainerAudit> = Vec::with_capacity(containers.len());
        for reference in containers {
            let mut gaps = Vec::new();
            let outcome = match graph_of(&reference.container) {
                Ok(graph) => {
                    // The header declares how many node records the container
                    // holds; a converted graph that disagrees decoded only
                    // part of them, and a partial decode must not read as a
                    // complete mapping.
                    if graph.len() != reference.stored_nodes() as usize {
                        gaps.push(AuditGap::NodeCountMismatch {
                            container: reference.container.clone(),
                            declared: reference.stored_nodes(),
                            decoded: graph.len(),
                        });
                    }
                    // A rule that bound no node of the container is a
                    // container-level shortfall: it belongs to no single
                    // root, so no airframe may claim it mapped cleanly.
                    gaps.extend(
                        graph
                            .unmatched_bindings()
                            .iter()
                            .map(|path| AuditGap::UnmatchedRule { path: path.clone() }),
                    );
                    ContainerOutcome::Mapped(ContainerMapping {
                        container: reference.container.clone(),
                        roots: graph.roots().to_vec(),
                        node_count: graph.len(),
                        airframes: self
                            .entries
                            .iter()
                            .filter(|entry| {
                                entry
                                    .root()
                                    .is_some_and(|root| root.container() == &reference.container)
                            })
                            .map(|entry| entry.airframe.clone())
                            .collect(),
                    })
                }
                Err(blocker) => ContainerOutcome::Blocked(blocker),
            };
            audits.push(ContainerAudit {
                container: reference.container.clone(),
                declared_nodes: reference.stored_nodes(),
                nodes_offset: reference.nodes_offset(),
                outcome,
                gaps,
            });
        }

        let mut airframes = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            airframes.push(self.audit_entry(entry, &audits, &graph_of));
        }
        RosterAuditReport {
            containers: audits,
            airframes,
        }
    }

    /// Audits one roster row against the already-audited containers.
    fn audit_entry<'g, G>(
        &self,
        entry: &RosterEntry,
        containers: &[ContainerAudit],
        graph_of: &G,
    ) -> AirframeAudit
    where
        G: Fn(&ContentId) -> Result<&'g SceneGraph, ContainerBlocker>,
    {
        let mut gaps = Vec::new();
        let mut blockers = Vec::new();
        let forced_missions: Vec<ContentId> =
            self.missions_forcing(&entry.airframe).cloned().collect();

        if !entry.availability().is_known() {
            gaps.push(AuditGap::AvailabilityUndiscovered {
                airframe: entry.airframe.clone(),
            });
            if !forced_missions.is_empty() {
                gaps.push(AuditGap::ForcedAssignmentOnly {
                    airframe: entry.airframe.clone(),
                    missions: forced_missions.len(),
                });
            }
        }

        let mapping = match entry.root() {
            None => {
                blockers.push(AirframeBlocker::RootUndiscovered {
                    airframe: entry.airframe.clone(),
                });
                None
            }
            Some(root) => match containers
                .iter()
                .find(|audit| audit.container() == root.container())
            {
                None => {
                    blockers.push(AirframeBlocker::ContainerNotAudited {
                        airframe: entry.airframe.clone(),
                        container: root.container().clone(),
                    });
                    None
                }
                Some(container) => match container.blocker() {
                    Some(blocker) => {
                        blockers.push(AirframeBlocker::ContainerUndecoded {
                            airframe: entry.airframe.clone(),
                            blocker: blocker.clone(),
                        });
                        None
                    }
                    None => {
                        // The container converted, so the graph is in hand.
                        // A root that is not in it is a wrong reference, not a
                        // fallback to "the first root".
                        let graph = graph_of(root.container()).expect(
                            "an audited container with no blocker was produced from a graph",
                        );
                        if graph.node(root.root()).is_none() {
                            blockers.push(AirframeBlocker::RootMissing {
                                airframe: entry.airframe.clone(),
                                root: root.root().clone(),
                            });
                            None
                        } else {
                            let subtree = graph.subtree(root.root());
                            let mut sockets: Vec<MappedSocket> = Vec::new();
                            let mut unknown: Vec<AuditGap> = Vec::new();
                            for node in &subtree {
                                let Some(socket) = graph.socket(node.id()) else {
                                    continue;
                                };
                                match (socket.known_role(), socket.collision()) {
                                    (Some(role), Resolved::Known(collision)) => {
                                        sockets.push(MappedSocket {
                                            node: node.id().clone(),
                                            role,
                                            collision: collision.value,
                                            pose: *socket.pose(),
                                            zone_id: socket.zone_id(),
                                            animation_channels: socket.animation().len(),
                                            provenance: socket.provenance().clone(),
                                        });
                                    }
                                    _ => {
                                        let (claim_id, reason) = unresolved_socket_claim(socket)
                                            .expect(
                                                "a socket with an unresolved role carries its \
                                                 claim",
                                            );
                                        unknown.push(AuditGap::UnknownRole {
                                            node: node.id().clone(),
                                            claim_id,
                                            reason,
                                        });
                                    }
                                }
                            }
                            // Stable-id order, the graph's own socket-table
                            // order: the report does not depend on how the
                            // subtree happened to be walked.
                            sockets.sort_by(|left, right| left.node.cmp(&right.node));
                            gaps.extend(unknown);
                            for role in entry.required_roles() {
                                if !sockets.iter().any(|socket| socket.role == *role) {
                                    gaps.push(AuditGap::MissingRole {
                                        root: root.root().clone(),
                                        role: *role,
                                    });
                                }
                            }
                            Some(AirframeMapping {
                                root: root.root().clone(),
                                container: root.container().clone(),
                                node_count: subtree.len(),
                                sockets,
                            })
                        }
                    }
                },
            },
        };

        AirframeAudit {
            airframe: entry.airframe.clone(),
            root: entry.root().cloned(),
            availability: entry.availability().clone(),
            forced_missions,
            mapping,
            blockers,
            gaps,
        }
    }
}

// ---------------------------------------------------------------- errors ---

/// Why a roster record was refused.
///
/// Every variant is an authoring contradiction — two rows claiming one
/// airframe, a required role declared twice, a forced plane that is not in
/// the roster — refused at construction rather than resolved by the audit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RosterError {
    /// The id named something other than an airframe.
    AirframeKind {
        /// The kind the id actually names.
        kind: ContentKind,
    },
    /// The id named something other than a campaign mission.
    MissionKind {
        /// The kind the id actually names.
        kind: ContentKind,
    },
    /// Two roster rows audit the same airframe.
    DuplicateAirframe {
        /// The repeated airframe.
        airframe: ContentId,
    },
    /// One mission forces the same airframe twice.
    DuplicateAssignment {
        /// The mission.
        mission: ContentId,
        /// The airframe it forces.
        airframe: ContentId,
    },
    /// A forced mission assignment names an airframe no roster row audits.
    UnknownAirframe {
        /// The missing airframe.
        airframe: ContentId,
    },
    /// A role is declared required twice for one airframe.
    DuplicateRequiredRole {
        /// The repeated role.
        role: PartRole,
    },
}

impl fmt::Display for RosterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AirframeKind { kind } => {
                write!(f, "a roster row must audit an airframe, got {kind}")
            }
            Self::MissionKind { kind } => {
                write!(
                    f,
                    "a forced mission assignment must name a mission, got {kind}"
                )
            }
            Self::DuplicateAirframe { airframe } => {
                write!(f, "the roster audits {airframe} twice")
            }
            Self::DuplicateAssignment { mission, airframe } => {
                write!(f, "{mission} forcing {airframe} is recorded twice")
            }
            Self::UnknownAirframe { airframe } => write!(
                f,
                "{airframe} is forced by a mission but no roster row audits it"
            ),
            Self::DuplicateRequiredRole { role } => {
                write!(f, "the role {} is required twice", role.label())
            }
        }
    }
}

impl std::error::Error for RosterError {}

/// Why a scene conversion or a scene contract record was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum SceneError {
    /// The parsed array held no nodes.
    EmptyScene,
    /// Two stored records claimed the same array slot.
    DuplicateIndex {
        /// The repeated slot.
        index: u32,
    },
    /// A node's parent slot has no record.
    DanglingParent {
        /// The node that named the missing parent.
        node: u32,
        /// The missing slot.
        parent: u32,
    },
    /// A node's child slot has no record.
    DanglingChild {
        /// The node that named the missing child.
        node: u32,
        /// The missing slot.
        child: u32,
    },
    /// The parent and child records disagree: a node names a parent whose
    /// children do not list it, or a listed child names a different parent.
    InconsistentParentage {
        /// The node whose parent link is inconsistent.
        node: u32,
        /// The parent slot in question.
        parent: u32,
    },
    /// No record has an empty parent, so there is no root to start from.
    NoRoots,
    /// The parent/child links form an ownership cycle — reachable from a
    /// root or detached; both are refused (IDENTITY-CONTENT).
    Cycle {
        /// A stored slot on the cycle.
        node: u32,
    },
    /// A caller asked for *the* root of a container that does not have
    /// exactly one.
    AmbiguousRoots {
        /// How many roots the container holds.
        roots: usize,
    },
    /// No root carries the requested authored name.
    UnknownRoot {
        /// The requested name.
        name: String,
    },
    /// An authored name-path could not form a `scene_node` id.
    NodeId {
        /// The stored slot of the offending node.
        node: u32,
        /// The id rejection.
        source: ContentIdError,
    },
    /// Two nodes derived the same stable id — same-name siblings, or names
    /// that collide after normalization. Ambiguity is refused, never
    /// silently disambiguated by position.
    DuplicateNodeId {
        /// The colliding id.
        id: SceneNodeId,
    },
    /// A [`SceneNodeId`] was built from a content id of another kind.
    NodeKind {
        /// The kind the id actually names.
        kind: ContentKind,
    },
    /// A [`SceneRootRef`] named a root that does not live in the container.
    RootOutsideContainer {
        /// The container key.
        container: String,
        /// The root's key.
        root: String,
    },
    /// A [`SceneRootRef`] named a nested node, not a root.
    NotARootNode {
        /// The offending key.
        root: String,
    },
    /// An authored transform component was non-finite.
    Transform {
        /// The stored slot of the offending node.
        node: u32,
        /// Which component failed.
        source: SpaceError,
    },
    /// An LOD range was non-finite or reversed.
    LodRange {
        /// The stored slot of the offending node.
        node: u32,
        /// The stored near distance.
        range_min: f32,
        /// The stored far distance.
        range_max: f32,
    },
    /// A mesh binding resolved to a catalog id of the wrong kind.
    MeshKind {
        /// The stored slot of the offending node.
        node: u32,
        /// The kind the resolved id names.
        kind: ContentKind,
    },
    /// An animation channel resolved to a catalog id of the wrong kind.
    AnimationChannelKind {
        /// The stored slot of the offending node.
        node: u32,
        /// The kind the resolved id names.
        kind: ContentKind,
    },
    /// Two binding rules name the same authored path.
    DuplicateBindingRule {
        /// The duplicated path.
        path: String,
    },
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyScene => write!(f, "the node array is empty"),
            Self::DuplicateIndex { index } => {
                write!(f, "two node records claim array slot {index}")
            }
            Self::DanglingParent { node, parent } => {
                write!(f, "node {node} names missing parent slot {parent}")
            }
            Self::DanglingChild { node, child } => {
                write!(f, "node {node} lists missing child slot {child}")
            }
            Self::InconsistentParentage { node, parent } => write!(
                f,
                "node {node} and parent slot {parent} disagree on the link"
            ),
            Self::NoRoots => write!(f, "the node array has no root"),
            Self::Cycle { node } => {
                write!(f, "the parent/child links form a cycle through node {node}")
            }
            Self::AmbiguousRoots { roots } => {
                write!(f, "the container holds {roots} roots, not exactly one")
            }
            Self::UnknownRoot { name } => write!(f, "no root is named {name:?}"),
            Self::NodeId { node, source } => {
                write!(f, "node {node} cannot form a scene id: {source}")
            }
            Self::DuplicateNodeId { id } => {
                write!(f, "two nodes derive the same scene id {id}")
            }
            Self::NodeKind { kind } => {
                write!(f, "a scene node id must be a scene_node, got {kind}")
            }
            Self::RootOutsideContainer { container, root } => write!(
                f,
                "scene root {root} does not live in container {container}"
            ),
            Self::NotARootNode { root } => {
                write!(f, "{root} names a nested node, not a root")
            }
            Self::Transform { node, source } => {
                write!(f, "node {node} transform: {source}")
            }
            Self::LodRange {
                node,
                range_min,
                range_max,
            } => write!(
                f,
                "node {node} LOD range {range_min}..{range_max} is not usable"
            ),
            Self::MeshKind { node, kind } => {
                write!(
                    f,
                    "node {node} mesh binding resolves to a {kind}, not a mesh"
                )
            }
            Self::AnimationChannelKind { node, kind } => write!(
                f,
                "node {node} animation binding resolves to a {kind}, not an animation_track"
            ),
            Self::DuplicateBindingRule { path } => {
                write!(f, "two binding rules name the path {path:?}")
            }
        }
    }
}

impl std::error::Error for SceneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NodeId { source, .. } => Some(source),
            Self::Transform { source, .. } => Some(source),
            _ => None,
        }
    }
}

// ------------------------------------------ the GameZ node array to records ---

/// The claim a node's stored `mesh_index` is refused under when the container's
/// mesh catalog cannot answer it.
///
/// A `mesh_index` is the *only* address a mesh has — the mesh array carries no
/// names — so a slot this stage cannot resolve stays an explicit unknown with
/// this claim and a reason, and never becomes an invented id.
const MESH_SLOT_UNRESOLVED: &str = "f11-node-array.mesh-slot-unresolved";

/// One mesh-array slot's catalog element: the id a node's stored `mesh_index`
/// resolves to, and the provenance of that id.
///
/// The table is **input**, not something this module derives: which catalog
/// element a mesh-array slot stands for is F10-C.03's discovery, and carrying
/// the id together with its own [`Provenance`] is what keeps "the mapping
/// exists" a checkable claim instead of an assumption made here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshSlot {
    id: ContentId,
    provenance: Provenance,
}

impl MeshSlot {
    /// Binds one mesh-array slot to a catalog element.
    ///
    /// # Errors
    ///
    /// [`GameZSceneError::MeshKind`] when `id` is not in the `mesh` namespace.
    /// A node's `mesh_index` can only ever name a mesh, so a slot that names
    /// anything else is a catalog error, refused where it is declared rather
    /// than at the first node that happens to use it.
    pub fn new(id: ContentId, provenance: Provenance) -> Result<Self, GameZSceneError> {
        if id.kind() != ContentKind::Mesh {
            return Err(GameZSceneError::MeshKind {
                node: u32::MAX,
                index: u32::MAX,
                kind: id.kind(),
            });
        }
        Ok(Self { id, provenance })
    }

    /// The catalog element this slot stands for.
    #[must_use]
    pub fn id(&self) -> &ContentId {
        &self.id
    }

    /// Where that element was discovered.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// Why a decoded GameZ node array could not become [`ParsedNode`] records.
///
/// The variants carry node indices and counts only, never archive bytes. A
/// refusal here is about *one record*; a refusal from
/// [`SceneGraph::build`] is about the *hierarchy* and arrives as
/// [`GameZSceneError::Build`], so the two are never confused.
#[derive(Clone, Debug, PartialEq)]
pub enum GameZSceneError {
    /// A mesh slot's catalog element is not in the `mesh` namespace.
    MeshKind {
        /// The node that used the slot, or [`u32::MAX`] when the slot itself
        /// was refused at construction.
        node: u32,
        /// The stored `mesh_index`, or [`u32::MAX`] for the same reason.
        index: u32,
        /// The namespace the element really is in.
        kind: ContentKind,
    },
    /// A LOD record's near bound is stored as a negative square, so it has no
    /// real root and the near distance does not exist.
    LodNearBound {
        /// The node's stored array slot.
        node: u32,
        /// The stored value.
        found: f32,
    },
    /// The hierarchy was refused by [`SceneGraph::build`], verbatim. The
    /// records above were still produced; this is the conversion's own verdict
    /// on them, kept as its own variant so a caller can report "read, not
    /// converted" instead of "not read".
    Build(SceneError),
}

impl GameZSceneError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::MeshKind { .. } => "mesh_kind",
            Self::LodNearBound { .. } => "lod_near_bound",
            Self::Build(_) => "build",
        }
    }

    /// The node's stored array slot, when the failure is about one node.
    #[must_use]
    pub const fn node(&self) -> Option<u32> {
        match self {
            Self::MeshKind { node, .. } | Self::LodNearBound { node, .. } => Some(*node),
            Self::Build(_) => None,
        }
    }
}

impl fmt::Display for GameZSceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MeshKind { node, index, kind } => write!(
                f,
                "mesh slot {index} (used by node {node}) names a {kind} element, not a mesh"
            ),
            Self::LodNearBound { node, found } => write!(
                f,
                "node {node} stores a near LOD bound of {found}, which has no real root"
            ),
            Self::Build(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for GameZSceneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Build(error) => Some(error),
            _ => None,
        }
    }
}

impl From<SceneError> for GameZSceneError {
    fn from(error: SceneError) -> Self {
        Self::Build(error)
    }
}

/// Converts one decoded GameZ node array into [`ParsedNode`] records.
///
/// The input is `cs_formats::gamez::read_gamez_nodes`'s output: the store's own
/// records, every unmeasured word still on
/// [`RawNode::info`](cs_formats::gamez::RawNode::info) and every record's bytes
/// still addressable. This function is the *typed input* half of F11-A and adds
/// no interpretation the store has not earned:
///
/// * a **kind** is the store's own tag, one for one;
/// * an **object node** carries its authored transform verbatim, with the
///   stored `matrix` kept only when it disagrees with the one its own euler
///   triple derives ([`AuthoredTransform::matrix`]) — the reference corpus
///   disagrees in a small, measured fraction of objects, and where they differ
///   the stored matrix is what the file holds; a record the store flags as
///   holding no transform becomes [`AuthoredTransform::IDENTITY`] **only when it
///   really is the identity**, so a flagged record that stores something else
///   keeps its own words rather than having them discarded here;
/// * a **LOD node** carries the resolved near and far distances, the near bound
///   being the root of the square the record stores;
/// * every **other kind** stores no transform in its record at all, so its
///   authored transform is [`AuthoredTransform::IDENTITY`] — the identity is
///   stated as a fact about the record, not invented;
/// * `flags`, `zone_id`, the parent slot and the child slots cross over
///   untouched, and a node's **name** crosses over exactly as stored,
///   including a name the id grammar will later refuse. Whether a name can
///   form a `scene_node` key is [`SceneGraph::build`]'s verdict, not this
///   function's: transliterating it here would invent an identity the store
///   never had;
/// * a `mesh_index` of `-1` means no mesh and produces no binding; a
///   non-negative one resolves through `meshes`, or stays an explicit
///   [`Resolved::Unknown`] under claim `f11-node-array.mesh-slot-unresolved`
///   with the index and the slot count in its reason.
///
/// `meshes` is the container's mesh catalog in **mesh-array slot order**, so
/// `meshes[index]` is the element the store's `mesh_index` names. An index past
/// the table is **not** a refusal: the association and its stored index are
/// kept and the resolution is an explicit unknown, because the caller may simply
/// not have catalogued that slot yet, and refusing the whole record would throw
/// away a hierarchy that is otherwise exact. An empty table is legal input.
///
/// # Errors
///
/// [`GameZSceneError::LodNearBound`] for a LOD record whose near bound has no
/// real root. The hierarchy is **not** checked here: a cycle, a dangling
/// parent, an inconsistent link, an unusable name or a derived-id collision is
/// [`SceneGraph::build`]'s refusal and is reported by
/// [`scene_graph_from_gamez`], so the records survive a conversion that failed.
pub fn parsed_nodes_from_gamez(
    records: &GameZNodes,
    meshes: &[MeshSlot],
) -> Result<Vec<ParsedNode>, GameZSceneError> {
    records
        .nodes
        .iter()
        .map(|node| parsed_node_from_gamez(node, meshes))
        .collect()
}

/// Converts one stored node record into one [`ParsedNode`].
fn parsed_node_from_gamez(
    node: &RawNode,
    meshes: &[MeshSlot],
) -> Result<ParsedNode, GameZSceneError> {
    let kind = parsed_kind(node)?;
    let mesh = match u32::try_from(node.mesh_index()) {
        Err(_) => None,
        Ok(index) => Some(MeshBinding {
            index,
            mesh: match meshes.get(index as usize) {
                Some(slot) => Resolved::Known(Known::new(slot.id.clone(), slot.provenance.clone())),
                None => Resolved::unknown(
                    ClaimId::new(MESH_SLOT_UNRESOLVED).expect("the claim id is valid"),
                    &format!(
                        "the node stores mesh index {index} and the container's mesh catalog holds \
                         {} slot(s), so no element answers it",
                        meshes.len()
                    ),
                )
                .expect("the reason is not empty"),
            },
        }),
    };
    Ok(ParsedNode {
        index: node.index,
        name: node.name.clone(),
        kind,
        transform: authored_transform(node),
        parent: node.parent,
        children: node.children.clone(),
        mesh,
        zone_id: node.zone_id(),
        flags: node.flags(),
    })
}

/// The authored transform a stored record carries, or the identity for a kind
/// whose record carries none.
fn authored_transform(node: &RawNode) -> AuthoredTransform {
    match node.object3d() {
        // A record the store flagged as storing no transform holds exactly the
        // identity, so the identity is what crosses over rather than four
        // words that happen to be zero. The check is on the flag **and** on the
        // record really being the identity: a record whose flag says "no
        // transform" while storing something else is reported by the reader as
        // `ObjectIdentityNotIdentity`, and discarding its numbers here would
        // throw away the only trace of the disagreement before any caller
        // could see it. The store's own words cross over in that case.
        Some(object)
            if object.stores_identity()
                && object.rotation == [0.0; 3]
                && object.translation == [0.0; 3]
                && object.scale == [1.0; 3] =>
        {
            AuthoredTransform::IDENTITY
        }
        Some(object) => AuthoredTransform {
            rotation: object.rotation,
            scale: object.scale,
            matrix: object.matrix_disagrees().then_some(object.matrix),
            translation: object.translation,
        },
        None => AuthoredTransform::IDENTITY,
    }
}

/// The typed kind of one stored record, with a LOD node's near bound resolved.
fn parsed_kind(node: &RawNode) -> Result<ParsedNodeKind, GameZSceneError> {
    Ok(match node.kind {
        StoredNodeKind::World(_) => ParsedNodeKind::World,
        StoredNodeKind::Camera => ParsedNodeKind::Camera,
        StoredNodeKind::Window => ParsedNodeKind::Window,
        StoredNodeKind::Display => ParsedNodeKind::Display,
        StoredNodeKind::Light => ParsedNodeKind::Light,
        StoredNodeKind::Object3d(_) => ParsedNodeKind::Object3d,
        StoredNodeKind::Lod(RawLodData {
            level,
            range_far,
            range_near_sq,
            ..
        }) => {
            let range_min = range_near_sq.sqrt();
            if !range_min.is_finite() {
                return Err(GameZSceneError::LodNearBound {
                    node: node.index,
                    found: range_near_sq,
                });
            }
            ParsedNodeKind::Lod {
                // The record stores a boolean and nothing says which value means
                // what, so it crosses over as the stored bit and no more.
                level: level != 0,
                range_min,
                range_max: range_far,
            }
        }
    })
}

/// Decodes a container's node array and converts it into a [`SceneGraph`].
///
/// This is the whole path the F11 stages were waiting for: the store's own
/// reader, this crate's [`ParsedNode`] records, and the canonical conversion
/// with its rejections. The three steps stay separate on purpose —
/// [`parsed_nodes_from_gamez`] produces the records and
/// [`SceneGraph::build`] judges the hierarchy — so a container whose records
/// read but whose hierarchy does not convert is reported as exactly that.
///
/// # Errors
///
/// Every [`GameZSceneError`]: the record-level refusals from
/// [`parsed_nodes_from_gamez`], and [`GameZSceneError::Build`] carrying
/// [`SceneGraph::build`]'s own typed refusal verbatim.
pub fn scene_graph_from_gamez(
    container: &ContentId,
    records: &GameZNodes,
    meshes: &[MeshSlot],
    adapter: &SourceAdapter,
    bindings: &BindingMap,
) -> Result<SceneGraph, GameZSceneError> {
    let nodes = parsed_nodes_from_gamez(records, meshes)?;
    Ok(SceneGraph::build(container, &nodes, adapter, bindings)?)
}
