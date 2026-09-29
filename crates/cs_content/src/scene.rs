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

use cs_types::content::{ContentId, ContentIdError, ContentKind, Provenance, Resolved};
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
#[derive(Clone, Debug)]
pub struct SceneGraph {
    container: ContentId,
    nodes: Vec<SceneNode>,
    roots: Vec<SceneNodeId>,
    by_id: BTreeMap<SceneNodeId, usize>,
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
        Ok(Self {
            container: container.clone(),
            nodes: built,
            roots,
            by_id,
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

    /// The authored paths of the binding rules that matched no node. A rule
    /// that lands nowhere is reported, never silently dropped.
    #[must_use]
    pub fn unmatched_bindings(&self) -> &[String] {
        &self.unmatched_bindings
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

// ---------------------------------------------------------------- errors ---

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
