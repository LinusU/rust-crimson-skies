//! ECS binding records for canonical scene nodes
//! (`specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`, stage
//! `### F11-A`; shared contract `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! These are the typed *outputs* a scene consumer binds into the ECS — the
//! records later stages attach to spawned entities. Stage `### F11-B` below
//! adds the import path and the LOD selection system; stage `### F11-C` wires
//! them into the running app: the load/unload request producer and consumer,
//! the socket bindings, damage visuals and teardown.
//!
//! [`SceneNodeBinding`] ties an entity to its canonical
//! [`cs_content::scene::SceneNode`] by [`ContentId`], stamped with the
//! [`SceneGeneration`] that produced it, so a reload under a new generation
//! can never leave a stale binding looking live (F11 non-negotiable
//! behavior 5: session-generation ownership; `IDENTITY-CONTENT`: session
//! generations).
//!
//! [`NodeVisualTransform`] carries a node's composed
//! [`CanonicalTransform`] as a Bevy [`GlobalTransform`]. `GlobalTransform`
//! wraps a full affine matrix, so a mirrored subtree (negative scale)
//! survives intact — Bevy's `Transform` TRS could not hold it. The render
//! path and the collision path both read the node's one composed transform,
//! so neither can diverge from the other across an LOD switch (F11
//! non-negotiable behavior 4).
//!
//! # F11-B: hierarchy import and LOD selection
//!
//! [`import_scene`] turns a converted [`SceneGraph`] into entities: one per
//! node, wired with [`ChildOf`]/`Children`, stamped with the stable
//! [`SceneNodeBinding`] id and its [`SceneGeneration`], carrying the node's
//! [`NodeVisualTransform`] and — for a `Lod` node — its [`NodeLodVariant`]
//! band. Every transform is converted *before* the first spawn, so a
//! hierarchy that cannot be represented leaves the world untouched.
//! [`import_airframe`] starts from an [`AirframeVisual`]'s checked root
//! reference and imports only that subtree: roots are named, never indexed.
//!
//! [`select_lod_presentation`] is the LOD system. It recomputes exactly one
//! component — [`NodePresentation`] (`Drawn`, `LodCulled` or `Disabled`) —
//! from the [`LodDistance`] the viewer stage supplies and the
//! [`NodeDisabled`] markers damage owns. A destroyed node and everything
//! under it stays `Disabled` whichever band becomes active, and a mesh under
//! a band the distance did not choose is `LodCulled` with it, so two LOD
//! levels of one part are never reported drawn at the same time. The system
//! never writes an identity, a transform, a binding or a disable marker, so
//! collision, weapon origins and damage identity cannot move with distance
//! (F11 non-negotiable behavior 4; AC02).
//!
//! # F11-C: producer and consumer wiring, teardown and damage visuals
//!
//! The import path and the LOD system are now driven by a request resource
//! ([`AirframeSceneRequest`]) that the producer inserts and
//! [`process_airframe_scene_request`] consumes **once per run**:
//!
//! * [`AirframeSceneRequest::Load`] prepares first and commits second. The
//!   subtree is imported under a fresh [`SceneGeneration`], its
//!   [`PartSocket`]s are bound as [`PartBinding`]s, and only then is a
//!   superseded scene released — so a refused load leaves the running
//!   aircraft untouched and retryable, and a reload leaves no hidden old
//!   root behind (F11 non-negotiable behavior 5). The refusal itself is
//!   reported in [`AirframeSceneLog`] as [`SceneEvent::Refused`].
//! * [`AirframeSceneRequest::Unload`] releases every entity the live
//!   [`LiveAirframeScene`] owns, and is a no-op when nothing is live, so
//!   loading and unloading the same airframe a hundred times leaves the live
//!   entity count unchanged (AC03).
//!
//! [`apply_airframe_damage`] is the consumer of the bound parts: the
//! recorded [`AirframeDamageState`] — part identities as stable
//! [`SceneNodeId`]s, never positions — is made equal to the
//! [`NodeDisabled`] markers of the live generation, so a destroyed part and
//! everything mounted under it (its gun, its mesh variants) stop being
//! presented, a repair brings it back, and the damage survives a reload
//! without being re-decided. An id that names no node of the live scene is
//! reported as [`SceneEvent::UnknownDamage`] rather than ignored.
//!
//! The three systems have a required order, and a schedule that gets it
//! wrong is wrong visibly rather than subtly:
//! [`process_airframe_scene_request`] first (it publishes the
//! [`LiveAirframeScene`] the other two read), then
//! [`apply_airframe_damage`] (it writes the markers), then
//! [`select_lod_presentation`] (it reads them). Damage that runs before the
//! load in a frame is applied on the next one, and a distance that runs
//! before the damage leaves that frame's presentation one verdict behind.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use bevy::ecs::component::Component;
use bevy::math::Mat4;
use bevy::prelude::{ChildOf, Entity, GlobalTransform, Query, Res, Resource, With, World};
use cs_content::scene::{
    CanonicalTransform, CollisionRole, LodInfo, LodSelectError, PartRole, PartSocket, SceneGraph,
    SceneNode, SceneNodeId, select_lod_variant,
};
use cs_types::content::{ContentId, Provenance, Resolved};
use cs_types::space::Meters;

use crate::airframe_visual::AirframeVisual;

/// The generation of a scene load: a monotonically increasing stamp that
/// makes stale entity bindings detectable after a reload.
///
/// Generations count up from 1; generation 0 is never produced by
/// [`Self::next`], so a default-constructed binding is visibly unspawned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SceneGeneration(pub u64);

impl SceneGeneration {
    /// The generation after this one.
    #[must_use]
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

/// Component: marks an entity as bound to one canonical scene node of one
/// scene generation.
///
/// `node` is the stable content id (`scene_node/<container>.<path>`); a
/// reload stamps new bindings with the new [`SceneGeneration`], so old
/// entities are identified by the generation mismatch rather than by
/// surviving pointers.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct SceneNodeBinding {
    /// The canonical scene node this entity presents.
    pub node: ContentId,
    /// The scene generation that spawned the binding.
    pub generation: SceneGeneration,
}

/// Component: the node's composed canonical transform as the ECS-side
/// affine.
///
/// The canonical matrix is arbitrary affine — rotation, scale and shear —
/// so it maps to a [`GlobalTransform`], never to Bevy's `Transform`: a
/// mirrored subtree's negative determinant would be silently lost by a TRS
/// decomposition.
#[derive(Component, Clone, Copy, Debug)]
pub struct NodeVisualTransform(pub GlobalTransform);

/// Why a canonical transform could not be carried into the ECS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeTransformError {
    /// A component did not survive the f64 → f32 cast (overflow to
    /// infinity); the canonical value is outside the renderable range.
    NotRepresentable,
}

impl core::fmt::Display for NodeTransformError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotRepresentable => {
                write!(f, "canonical transform does not fit the f32 render affine")
            }
        }
    }
}

impl std::error::Error for NodeTransformError {}

impl NodeVisualTransform {
    /// Converts a node's composed canonical transform (meters, row-major
    /// 3×3 + translation) into the ECS affine.
    ///
    /// # Errors
    ///
    /// [`NodeTransformError::NotRepresentable`] when an f64 component does
    /// not survive the f32 cast.
    pub fn from_canonical(transform: &CanonicalTransform) -> Result<Self, NodeTransformError> {
        let l = transform.linear();
        let t = transform.translation();
        // Mat4 is column-major; the affine's columns are the linear map's
        // columns and the translation.
        let mut columns = [
            [l[0][0] as f32, l[1][0] as f32, l[2][0] as f32, 0.0],
            [l[0][1] as f32, l[1][1] as f32, l[2][1] as f32, 0.0],
            [l[0][2] as f32, l[1][2] as f32, l[2][2] as f32, 0.0],
            [t[0] as f32, t[1] as f32, t[2] as f32, 1.0],
        ];
        for column in &mut columns {
            if !column.iter().all(|value| value.is_finite()) {
                return Err(NodeTransformError::NotRepresentable);
            }
        }
        Ok(Self(GlobalTransform::from(Mat4::from_cols_array_2d(
            &columns,
        ))))
    }

    /// The ECS affine.
    #[must_use]
    pub fn global(&self) -> GlobalTransform {
        self.0
    }

    /// Whether the composed transform mirrors space — the same flag the
    /// canonical [`CanonicalTransform::mirrored`] reports, read back off the
    /// f32 affine.
    #[must_use]
    pub fn mirrored(&self) -> bool {
        self.0.affine().matrix3.determinant() < 0.0
    }
}

// --------------------------------------------------------------- import ---

/// Why a hierarchy import was refused.
///
/// An import is all-or-nothing: nothing is spawned until every node's
/// transform has been converted, so a failure never leaves half a hierarchy
/// live in the world.
#[derive(Clone, Debug, PartialEq)]
pub enum SceneImportError {
    /// A node's composed canonical transform does not fit the f32 render
    /// affine, so the node (and with it the whole import) was refused.
    Transform {
        /// The stable id of the offending node.
        node: SceneNodeId,
        /// The conversion failure.
        source: NodeTransformError,
    },
    /// An airframe's root reference names a container other than the one the
    /// graph was converted from.
    ForeignContainer {
        /// The container key of the graph the import reads.
        expected: String,
        /// The container key the reference points at.
        found: String,
    },
    /// The referenced root does not exist in this graph.
    UnknownNode {
        /// The missing id.
        id: SceneNodeId,
    },
}

impl core::fmt::Display for SceneImportError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Transform { node, source } => {
                write!(f, "node {node} cannot be imported: {source}")
            }
            Self::ForeignContainer { expected, found } => write!(
                f,
                "the airframe root references container {found}, but the graph is {expected}"
            ),
            Self::UnknownNode { id } => write!(f, "the scene graph has no node {id}"),
        }
    }
}

impl std::error::Error for SceneImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transform { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// What one import produced: the entity of every imported node, keyed by its
/// stable [`SceneNodeId`], plus the [`SceneGeneration`] that stamped them.
///
/// The map is the handle a later stage uses to reach a node by identity
/// instead of by array position (F11 deliverable). It also records what
/// [`process_airframe_scene_request`] must give back on teardown: the
/// entities are listed in stable-id order and releasing them is the only way
/// a scene leaves the world, so a repeated load/unload cycle cannot grow the
/// live entity count (AC03).
#[derive(Clone, Debug, PartialEq)]
pub struct SceneImport {
    entities: BTreeMap<SceneNodeId, Entity>,
    generation: SceneGeneration,
}

impl SceneImport {
    /// The entity a node was imported as, by stable id.
    #[must_use]
    pub fn entity(&self, id: &SceneNodeId) -> Option<Entity> {
        self.entities.get(id).copied()
    }

    /// The generation every entity in this import is stamped with.
    #[must_use]
    pub fn generation(&self) -> SceneGeneration {
        self.generation
    }

    /// How many nodes were imported.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Whether the import produced no entities (it never can: an empty
    /// [`SceneGraph`] is refused at build time).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Every imported node with the entity it owns, in stable-id order.
    ///
    /// This is the teardown list: releasing exactly these entities (and
    /// nothing else) is what keeps repeated loads from accumulating nodes.
    pub fn entities(&self) -> impl Iterator<Item = (SceneNodeId, Entity)> + '_ {
        self.entities
            .iter()
            .map(|(id, entity)| (id.clone(), *entity))
    }
}

/// Imports every node of `graph` into `world`, in root-first preorder.
///
/// Each node becomes one entity carrying its stable [`SceneNodeBinding`], its
/// composed [`NodeVisualTransform`], an initial [`NodePresentation`] of
/// [`PresentationState::Drawn`] and — for a `Lod` node — its
/// [`NodeLodVariant`] band. The authored parent link becomes [`ChildOf`], so
/// the hierarchy is a real ECS hierarchy rather than a lookup table.
///
/// # Errors
///
/// [`SceneImportError::Transform`] naming the first node whose composed
/// transform does not fit f32; nothing is spawned in that case.
pub fn import_scene(
    world: &mut World,
    graph: &SceneGraph,
    generation: SceneGeneration,
) -> Result<SceneImport, SceneImportError> {
    let nodes: Vec<&SceneNode> = graph.nodes().iter().collect();
    spawn_nodes(world, &nodes, generation)
}

/// Imports only the subtree under an airframe's checked root reference.
///
/// The reference's container must be the graph's container and its root must
/// exist in the graph, so an airframe visual can never pull a tree out of the
/// wrong container or fall back to "whatever the first root is" (F11
/// deliverable: roots are referenced, never selected by array position).
///
/// # Errors
///
/// [`SceneImportError::ForeignContainer`] when the reference names another
/// container, [`SceneImportError::UnknownNode`] when the root is not in the
/// graph, and [`SceneImportError::Transform`] when a node of the subtree
/// cannot be represented — the same all-or-nothing conversion
/// [`import_scene`] performs.
pub fn import_airframe(
    world: &mut World,
    graph: &SceneGraph,
    visual: &AirframeVisual,
    generation: SceneGeneration,
) -> Result<SceneImport, SceneImportError> {
    let container = visual.root().container();
    if container != graph.container() {
        return Err(SceneImportError::ForeignContainer {
            expected: graph.container().key().to_owned(),
            found: container.key().to_owned(),
        });
    }
    let root_id = visual.root().root();
    if graph.node(root_id).is_none() {
        return Err(SceneImportError::UnknownNode {
            id: root_id.clone(),
        });
    }

    // Collect the subtree's ids, then let the graph's root-first preorder
    // decide the spawn order so a parent always exists before its child.
    let mut included: HashSet<SceneNodeId> = HashSet::new();
    let mut pending = vec![root_id.clone()];
    while let Some(id) = pending.pop() {
        if !included.insert(id.clone()) {
            continue;
        }
        if let Some(node) = graph.node(&id) {
            pending.extend(node.children().iter().rev().cloned());
        }
    }
    let subtree: Vec<&SceneNode> = graph
        .nodes()
        .iter()
        .filter(|node| included.contains(node.id()))
        .collect();
    spawn_nodes(world, &subtree, generation)
}

/// Spawns `nodes` (already in root-first order) as one generation-stamped
/// hierarchy, converting every transform before the first spawn.
fn spawn_nodes(
    world: &mut World,
    nodes: &[&SceneNode],
    generation: SceneGeneration,
) -> Result<SceneImport, SceneImportError> {
    let mut transforms = Vec::with_capacity(nodes.len());
    for node in nodes {
        let transform =
            NodeVisualTransform::from_canonical(node.visual_transform()).map_err(|source| {
                SceneImportError::Transform {
                    node: node.id().clone(),
                    source,
                }
            })?;
        transforms.push(transform);
    }

    let mut entities: BTreeMap<SceneNodeId, Entity> = BTreeMap::new();
    for (node, transform) in nodes.iter().zip(transforms) {
        let mut spawned = world.spawn((
            SceneNodeBinding {
                node: node.id().as_content_id().clone(),
                generation,
            },
            transform,
            NodePresentation(PresentationState::Drawn),
        ));
        if let Some(info) = node.lod() {
            let variant = NodeLodVariant::new(*info)
                .expect("SceneGraph::build rejects unusable LOD ranges before a node exists");
            spawned.insert(variant);
        }
        if let Some(parent_id) = node.parent()
            && let Some(&parent) = entities.get(parent_id)
        {
            spawned.insert(ChildOf(parent));
        }
        entities.insert(node.id().clone(), spawned.id());
    }
    Ok(SceneImport {
        entities,
        generation,
    })
}

// ------------------------------------------------------- LOD presentation ---

/// Component: this node is one LOD variant, carrying its converted band.
///
/// The band is private and validated in [`Self::new`] with the same rule the
/// selection system runs, so every value the system can see is usable and
/// presentation can never be decided from an unusable range.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct NodeLodVariant(LodInfo);

impl NodeLodVariant {
    /// Adopts a converted LOD band.
    ///
    /// # Errors
    ///
    /// [`LodSelectError::Range`] when the band's range is non-finite,
    /// negative or reversed — the index is the band's own position in the
    /// one-element check slice.
    pub fn new(info: LodInfo) -> Result<Self, LodSelectError> {
        select_lod_variant(std::slice::from_ref(&info), Meters(0.0))?;
        Ok(Self(info))
    }

    /// The band: its stored level flag and its range in metres.
    #[must_use]
    pub const fn info(&self) -> LodInfo {
        self.0
    }
}

/// Component: the node is destroyed or otherwise disabled, and so is
/// everything under it.
///
/// This stage only *reads* the marker: inserting and clearing it is damage
/// work (F11-C / F29), and LOD selection may never remove it. A marker on a
/// node disables the whole subtree, which is how one damage identity covers
/// a part and the gun mounted on it.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeDisabled;

/// How a node is presented right now: the single field LOD selection owns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentationState {
    /// The node's geometry is presented.
    Drawn,
    /// The node is an LOD variant its group did not choose, or it hangs under
    /// such a band: another variant of that group is presented at this
    /// distance, so a descendant of a culled band is culled with it and two
    /// LOD levels of one part are never reported drawn together.
    LodCulled,
    /// The node or an ancestor is [`NodeDisabled`]; LOD can never bring it
    /// back.
    Disabled,
}

/// Component: the node's current presentation verdict.
///
/// Render wiring maps this to what is drawn (F17); collision, weapon origins
/// and damage identity read their own records, never this one.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodePresentation(pub PresentationState);

/// Why a [`LodDistance`] was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LodDistanceError {
    /// The value was not finite.
    NonFinite,
    /// The value was negative.
    Negative,
}

impl core::fmt::Display for LodDistanceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NonFinite => write!(f, "the LOD distance must be finite"),
            Self::Negative => write!(f, "the LOD distance must not be negative"),
        }
    }
}

impl std::error::Error for LodDistanceError {}

/// Resource: the viewer distance LOD selection presents at.
///
/// The value is supplied by the caller (the camera/viewer stage, F11-C/F21
/// wiring): this stage deliberately computes no camera distance. The field is
/// private and [`Self::new`] validates it, which is what lets the selection
/// system treat the rule's `Result` as an invariant rather than a failure.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct LodDistance(Meters);

impl LodDistance {
    /// Accepts a finite, non-negative distance in metres.
    ///
    /// # Errors
    ///
    /// [`LodDistanceError::NonFinite`] or [`LodDistanceError::Negative`].
    pub fn new(distance: Meters) -> Result<Self, LodDistanceError> {
        if !distance.0.is_finite() {
            return Err(LodDistanceError::NonFinite);
        }
        if distance.0 < 0.0 {
            return Err(LodDistanceError::Negative);
        }
        Ok(Self(distance))
    }

    /// The distance, in metres.
    #[must_use]
    pub const fn meters(self) -> Meters {
        self.0
    }
}

/// Presentation-only LOD selection over every imported node.
///
/// For each group of `Lod` siblings the system picks one band with
/// [`select_lod_variant`] at the [`LodDistance`], then writes
/// [`NodePresentation`] for every node: `Disabled` when the node or any
/// ancestor carries [`NodeDisabled`], otherwise `LodCulled` when the node or
/// any ancestor is a band the group did not choose, otherwise `Drawn`.
/// `Disabled` wins over `LodCulled` at any depth, so a destroyed wing stays
/// destroyed whichever band its parent group selects.
///
/// It writes nothing else — not the binding, not the transform, not the
/// marker, not the entity set — so a distance change can only ever change
/// which variant is drawn (F11 non-negotiable behavior 4; AC02: a destroyed
/// wing and its gun stay disabled across an LOD transition).
pub fn select_lod_presentation(
    distance: Res<LodDistance>,
    nodes: Query<(Entity, Option<&NodeLodVariant>), With<SceneNodeBinding>>,
    parents: Query<&ChildOf>,
    disabled: Query<(), With<NodeDisabled>>,
    mut presentation: Query<&mut NodePresentation>,
) {
    let target = distance.meters();

    // One pass: the parent links, the variant groups (siblings of one parent),
    // each variant's group key and the node list the second pass walks.
    let mut parent_of: HashMap<Entity, Entity> = HashMap::new();
    let mut groups: HashMap<Option<Entity>, Vec<(Entity, LodInfo)>> = HashMap::new();
    let mut variant_parent: HashMap<Entity, Option<Entity>> = HashMap::new();
    let mut imported: Vec<(Entity, Option<LodInfo>)> = Vec::new();
    for (entity, variant) in nodes.iter() {
        let parent = parents.get(entity).ok().map(|parent| parent.0);
        if let Some(parent) = parent {
            parent_of.insert(entity, parent);
        }
        if let Some(variant) = variant {
            groups
                .entry(parent)
                .or_default()
                .push((entity, variant.info()));
            variant_parent.insert(entity, parent);
        }
        imported.push((entity, variant.map(|variant| variant.info())));
    }

    // One choice per group. `LodDistance` and `NodeLodVariant` both validate
    // what they hold, so the rule's refusals cannot occur here.
    let mut selected: HashMap<Option<Entity>, Entity> = HashMap::new();
    for (parent, members) in &groups {
        let bands: Vec<LodInfo> = members.iter().map(|(_, info)| *info).collect();
        let choice = select_lod_variant(&bands, target)
            .expect("LodDistance and NodeLodVariant are validated at construction");
        selected.insert(*parent, members[choice.index].0);
    }

    // A node is disabled when it or any ancestor is: destroying a part
    // disables the gun mounted under it without naming the gun twice. It is
    // culled when it — or an ancestor band — is a variant its group did not
    // choose, so a mesh under a culled band is culled with it; otherwise every
    // mesh of every band of one part would report `Drawn` at the same time.
    // `Disabled` wins, and the walk stops there.
    let mut disabled_nodes: HashSet<Entity> = HashSet::new();
    let mut culled_nodes: HashSet<Entity> = HashSet::new();
    for (entity, _) in &imported {
        let mut cursor = Some(*entity);
        let mut steps = 0usize;
        let mut culled = false;
        while let Some(current) = cursor {
            if disabled.contains(current) {
                disabled_nodes.insert(*entity);
                break;
            }
            if variant_parent
                .get(&current)
                .is_some_and(|group| selected.get(group) != Some(&current))
            {
                culled = true;
            }
            steps += 1;
            if steps > parent_of.len() {
                break;
            }
            cursor = parent_of.get(&current).copied();
        }
        if culled {
            culled_nodes.insert(*entity);
        }
    }

    // The only write in the system.
    for (entity, _) in imported {
        let Ok(mut node) = presentation.get_mut(entity) else {
            continue;
        };
        node.0 = if disabled_nodes.contains(&entity) {
            PresentationState::Disabled
        } else if culled_nodes.contains(&entity) {
            PresentationState::LodCulled
        } else {
            PresentationState::Drawn
        };
    }
}

// ------------------------------------------------- load / unload wiring ---

/// Resource: the request the scene loader processes on its next run.
///
/// This is the producer → consumer hand-off of stage `### F11-C`. The producer
/// (the asset pipeline, `cs_app::loading`) inserts a request; the exclusive
/// system [`process_airframe_scene_request`] consumes it **once**, so a
/// request is never applied twice and never survives into a later run where
/// it would be stale. A request is never a stored wish: it is either served or
/// it is reported as refused, and the caller inspects [`AirframeSceneLog`].
///
/// The load arm carries the converted container graph. It is a shared
/// `Arc`, not a copy: the real producer hands over the same record from its
/// [`crate::loading::ReadyBundle`] once a retail reader exists (see the
/// recorded GameZ node-array blocker in the findings doc), and until then the
/// graph is what an already converted hierarchy looks like.
#[derive(Resource, Clone, Debug, PartialEq)]
pub enum AirframeSceneRequest {
    /// Import one airframe's visual subtree, replacing the live scene.
    Load {
        /// The airframe → scene-root reference to import from.
        airframe: AirframeVisual,
        /// The converted container the root lives in.
        graph: Arc<SceneGraph>,
    },
    /// Release the live scene.
    Unload,
}

impl AirframeSceneRequest {
    /// A request to import `airframe`'s visual subtree out of `graph`.
    #[must_use]
    pub fn load(airframe: AirframeVisual, graph: Arc<SceneGraph>) -> Self {
        Self::Load { airframe, graph }
    }

    /// A request to release the live scene.
    #[must_use]
    pub const fn unload() -> Self {
        Self::Unload
    }
}

/// Resource: the scene generation counter.
///
/// Generations only ever count up and are never reused, not even after an
/// unload or a refused load: a stamp a stale entity could still carry must
/// never be handed out again (F11 non-negotiable behavior 5; `IDENTITY-CONTENT`:
/// session generations). The counter is consumed by the load path alone.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SceneGenerations(SceneGeneration);

impl SceneGenerations {
    /// The next generation, consuming it. A load that is later refused has
    /// still burned its number, so the retry is distinguishable.
    pub fn take_next(&mut self) -> SceneGeneration {
        self.0 = self.0.next();
        self.0
    }

    /// The most recently consumed generation (`0` before any load).
    #[must_use]
    pub const fn latest(&self) -> SceneGeneration {
        self.0
    }
}

/// Component: this node is an aircraft part/socket with an evidenced
/// gameplay role.
///
/// The role, the collision role and the rule's provenance ride on the entity
/// so a query can find a gun mount or a damage zone without consulting the
/// content graph. The socket's **pose is not here**: the node entity's
/// [`NodeVisualTransform`] is the one pose owner, and it is the same value
/// collision evaluates, so a weapon origin cannot drift away from the visual
/// (F11 non-negotiable behavior 4). A socket whose role the evidence left
/// explicitly unknown is *not* given this component — the loader reports it
/// in [`SceneEvent::Loaded`]'s `unresolved` list instead of defaulting a
/// role.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct PartBinding {
    role: Resolved<PartRole>,
    collision: Resolved<CollisionRole>,
    zone_id: u32,
    provenance: Provenance,
}

impl PartBinding {
    /// The gameplay role with the rule's provenance, or the explicit unknown
    /// the evidence left.
    #[must_use]
    pub fn role(&self) -> &Resolved<PartRole> {
        &self.role
    }

    /// The known role, or `None` when the rule's role was unknown.
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

    /// The bound node's stored zone id, uninterpreted.
    #[must_use]
    pub const fn zone_id(&self) -> u32 {
        self.zone_id
    }

    /// The provenance of the rule that produced this binding.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// Resource: the one live airframe scene, owned by its generation.
///
/// It is the ownership record of the imported subtree: the airframe and root
/// it serves, the generation that stamped every entity, the imported entity
/// of every node, which of those nodes are sockets, and the converted graph
/// the sockets and mesh associations come from. A reload replaces the whole
/// record; an unload removes it. Nothing outside this record is allowed to
/// remember a node entity, so nothing can address a scene that is gone.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct LiveAirframeScene {
    airframe: ContentId,
    root: SceneNodeId,
    generation: SceneGeneration,
    import: SceneImport,
    sockets: BTreeSet<SceneNodeId>,
    graph: Arc<SceneGraph>,
}

impl LiveAirframeScene {
    /// The airframe this scene visualizes.
    #[must_use]
    pub fn airframe(&self) -> &ContentId {
        &self.airframe
    }

    /// The scene root the import started from.
    #[must_use]
    pub fn root(&self) -> &SceneNodeId {
        &self.root
    }

    /// The generation that stamped every entity of this scene.
    #[must_use]
    pub const fn generation(&self) -> SceneGeneration {
        self.generation
    }

    /// The entity a node of this scene was imported as.
    #[must_use]
    pub fn entity(&self, node: &SceneNodeId) -> Option<Entity> {
        self.import.entity(node)
    }

    /// The evidence-backed socket record of one of this scene's nodes.
    ///
    /// The node must be part of this scene and a rule must have bound it. A
    /// socket whose role is an explicit unknown is still returned — the
    /// record exists and refusing to look at it would hide the gap; the
    /// *bound* sockets are the ones with a [`PartBinding`], listed by
    /// [`Self::sockets`].
    #[must_use]
    pub fn socket(&self, node: &SceneNodeId) -> Option<&PartSocket> {
        self.import.entity(node)?;
        self.graph.socket(node)
    }

    /// The nodes that were bound as [`PartBinding`]s, in stable-id order.
    pub fn sockets(&self) -> impl Iterator<Item = &SceneNodeId> + '_ {
        self.sockets.iter()
    }

    /// The import that produced the scene's entities.
    #[must_use]
    pub const fn import(&self) -> &SceneImport {
        &self.import
    }

    /// The converted container the scene was imported from. The live record
    /// shares it with the producer's request; it is dropped on unload.
    #[must_use]
    pub fn graph(&self) -> &SceneGraph {
        &self.graph
    }

    /// How many nodes the scene holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.import.len()
    }

    /// Whether the scene holds no nodes (it never can: an import of an empty
    /// subtree is refused).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.import.is_empty()
    }
}

/// What one processed scene request or damage pass did.
#[derive(Clone, Debug, PartialEq)]
pub enum SceneEvent {
    /// An airframe's visual subtree was imported, bound and made live.
    Loaded {
        /// The airframe that was loaded.
        airframe: ContentId,
        /// The root the import started from.
        root: SceneNodeId,
        /// The generation every entity was stamped with.
        generation: SceneGeneration,
        /// How many nodes were imported.
        nodes: usize,
        /// How many sockets were bound as [`PartBinding`]s.
        sockets: usize,
        /// Sockets inside the subtree whose role is an explicit unknown; they
        /// are reported here instead of being given a default role.
        unresolved: Vec<SceneNodeId>,
    },
    /// A load request was refused. Nothing was spawned and the running scene
    /// was left exactly as it was, so the request can be retried.
    Refused {
        /// The airframe that was requested.
        airframe: ContentId,
        /// The root the request named.
        root: SceneNodeId,
        /// The generation the refused attempt consumed. It is not reused.
        generation: SceneGeneration,
        /// The import error, propagated verbatim.
        error: SceneImportError,
    },
    /// A scene was released: by an explicit unload or by a reload that
    /// superseded it.
    Released {
        /// The airframe the scene served.
        airframe: ContentId,
        /// The generation that owned the released entities.
        generation: SceneGeneration,
        /// How many of the scene's entities were still live. Entities another
        /// stage had already despawned are not counted.
        entities: usize,
    },
    /// Recorded damage named nodes that are not part of the live scene; they
    /// were not applied and are not silently dropped.
    UnknownDamage {
        /// The unresolvable part identities, in the order recorded.
        ids: Vec<SceneNodeId>,
    },
}

impl SceneEvent {
    /// The generation this event concerns.
    #[must_use]
    pub fn generation(&self) -> Option<SceneGeneration> {
        match self {
            Self::Loaded { generation, .. }
            | Self::Refused { generation, .. }
            | Self::Released { generation, .. } => Some(*generation),
            Self::UnknownDamage { .. } => None,
        }
    }
}

/// Resource: the append-only record of what the scene systems did, oldest
/// first.
///
/// This is the error-propagation channel: a refused load, a released scene, a
/// socket whose role is unknown and damage that named no node are all visible
/// here instead of being logged away or defaulted. It grows with the number of
/// requests and is meant to be drained by a diagnostic surface, not to grow
/// without bound in a long session.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct AirframeSceneLog {
    events: Vec<SceneEvent>,
}

impl AirframeSceneLog {
    /// An empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A log holding one event.
    #[must_use]
    pub fn with(event: SceneEvent) -> Self {
        Self {
            events: vec![event],
        }
    }

    /// Appends one event.
    pub fn push(&mut self, event: SceneEvent) {
        self.events.push(event);
    }

    /// Every event, oldest first.
    #[must_use]
    pub fn events(&self) -> &[SceneEvent] {
        &self.events
    }

    /// The most recent event.
    #[must_use]
    pub fn last(&self) -> Option<&SceneEvent> {
        self.events.last()
    }

    /// How many events the log holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the log is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Resource: the recorded damage of the live scene's parts, keyed by stable
/// [`SceneNodeId`].
///
/// Damage names a *part identity*, never an array position or an entity, so
/// the record is a different engine's decision (F29: zones, armor and system
/// disablement) that this stage only reflects visually. The state outlives a
/// scene: it is not cleared by an unload, so a reloaded airframe starts with
/// the same damage instead of silently healing, and it is not cleared by a
/// refused load either.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct AirframeDamageState {
    destroyed: BTreeSet<SceneNodeId>,
}

impl AirframeDamageState {
    /// An empty damage state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a part as destroyed.
    pub fn destroy(&mut self, node: SceneNodeId) {
        self.destroyed.insert(node);
    }

    /// Records a part as repaired; reports whether it was destroyed before.
    pub fn repair(&mut self, node: &SceneNodeId) -> bool {
        self.destroyed.remove(node)
    }

    /// Whether a part is currently destroyed.
    #[must_use]
    pub fn is_destroyed(&self, node: &SceneNodeId) -> bool {
        self.destroyed.contains(node)
    }

    /// The destroyed parts, in stable-id order.
    pub fn destroyed(&self) -> impl Iterator<Item = &SceneNodeId> + '_ {
        self.destroyed.iter()
    }

    /// How many parts are destroyed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.destroyed.len()
    }

    /// Whether nothing is destroyed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.destroyed.is_empty()
    }
}

/// The producer → consumer hand-off: serves the queued [`AirframeSceneRequest`]
/// once.
///
/// It is an exclusive system because the import path it drives is
/// `&mut World`-shaped and because the whole load is one transaction: nothing
/// observes a half-loaded scene, and no other system can run between the
/// import and the release of the scene it supersedes.
///
/// * [`AirframeSceneRequest::Unload`] releases every entity of the live
///   [`LiveAirframeScene`] and removes the record. Unloading when nothing is
///   live is a no-op, so a repeated teardown or a teardown after a refused
///   load is safe.
/// * [`AirframeSceneRequest::Load`] takes the next generation, imports the
///   referenced subtree, binds its sockets, releases the scene it supersedes
///   and only then publishes the new [`LiveAirframeScene`]. A refusal
///   (foreign container, unknown root, unrepresentable transform) is reported
///   as [`SceneEvent::Refused`] and changes nothing: the running scene keeps
///   its entities and its generation, and the request can be retried.
///
/// The [`LodDistance`] resource is *not* created here: the viewer distance is
/// the camera stage's (F21/F17) input, and a scene that has none has no
/// meaningful LOD selection.
pub fn process_airframe_scene_request(world: &mut World) {
    let Some(request) = world.remove_resource::<AirframeSceneRequest>() else {
        return;
    };
    match request {
        AirframeSceneRequest::Unload => unload_airframe_scene(world),
        AirframeSceneRequest::Load { airframe, graph } => {
            load_airframe_scene(world, &airframe, graph);
        }
    }
}

/// Applies [`AirframeDamageState`] to the live scene's [`NodeDisabled`]
/// markers.
///
/// The damage state is the **single owner** of the marker inside the live
/// generation: every node of the live scene ends up carrying the marker
/// exactly when the damage state names it, so the system is convergent and
/// idempotent (running it twice changes nothing), a stale marker cannot
/// survive a repair, and a fresh generation inherits the recorded damage
/// without the damage being decided twice. [`select_lod_presentation`] then
/// propagates the marker to the part's descendants, which is how one damage
/// identity covers a destroyed wing, both of its LOD bands and the gun mounted
/// under it (AC02).
///
/// A recorded part that is not a node of the live scene is reported as
/// [`SceneEvent::UnknownDamage`]. With no live scene at all the pass is a
/// no-op and the damage state is kept for the next load — a part cannot be
/// damaged into a scene that does not exist yet, and the state must not be
/// lost while one is loading.
///
/// It is an exclusive system because it writes structural markers on the
/// entity set the live record owns: the plan is computed from that record
/// first, so no marker is written from a half-read table, and the pass stays
/// convergent whatever else the schedule does in the same frame.
pub fn apply_airframe_damage(world: &mut World) {
    // The plan is computed into owned locals first, so the world's borrow has
    // ended by the time a marker is written.
    let Some(plan) = damage_plan(world) else {
        return;
    };
    for (entity, destroyed) in plan.markers {
        // An entity another stage already despawned is not resurrected and
        // not counted; the live record is about to be replaced or removed.
        if world.get_entity(entity).is_err() {
            continue;
        }
        if destroyed {
            world.entity_mut(entity).insert(NodeDisabled);
        } else {
            world.entity_mut(entity).remove::<NodeDisabled>();
        }
    }
    if !plan.unknown.is_empty() {
        log_scene_event(world, SceneEvent::UnknownDamage { ids: plan.unknown });
    }
}

/// Which markers the live scene should carry, and which recorded part
/// identities name no node of it.
///
/// `None` when there is no damage state or no live scene: with nothing live
/// there is nothing to reflect onto, and the recorded state is kept for the
/// next load.
fn damage_plan(world: &World) -> Option<DamagePlan> {
    let damage = world.get_resource::<AirframeDamageState>()?;
    let live = world.get_resource::<LiveAirframeScene>()?;
    let unknown = damage
        .destroyed()
        .filter(|node| live.entity(node).is_none())
        .cloned()
        .collect();
    let plan = live
        .import
        .entities()
        .map(|(node, entity)| (entity, damage.is_destroyed(&node)))
        .collect();
    Some(DamagePlan {
        unknown,
        markers: plan,
    })
}

/// One damage pass's work: the part identities that name no node, and the
/// entity/`NodeDisabled` verdict every imported node must end up with.
struct DamagePlan {
    unknown: Vec<SceneNodeId>,
    markers: Vec<(Entity, bool)>,
}

/// Appends one event to the log, creating the resource if it is absent.
fn log_scene_event(world: &mut World, event: SceneEvent) {
    let mut log = world
        .remove_resource::<AirframeSceneLog>()
        .unwrap_or_default();
    log.push(event);
    world.insert_resource(log);
}

/// Despawns every entity `live` owns and reports how many were still there.
///
/// The list comes from the import's own record, so a release can never take
/// another generation's entities with it, and Bevy takes the descendants of
/// each despawned entity with it. An entity some other stage already
/// despawned is simply absent and is not counted.
fn release_scene(world: &mut World, live: &LiveAirframeScene) -> usize {
    let present = live
        .import
        .entities()
        .filter(|(_, entity)| world.get_entity(*entity).is_ok())
        .count();
    for (_, entity) in live.import.entities() {
        // Descendants go with their parent, so an entity an earlier
        // iteration already released is just gone.
        let _ = world.try_despawn(entity);
    }
    present
}

/// Serves an [`AirframeSceneRequest::Load`]: prepare, release, publish.
fn load_airframe_scene(world: &mut World, visual: &AirframeVisual, graph: Arc<SceneGraph>) {
    let airframe = visual.airframe().clone();
    let root = visual.root().root().clone();
    // The generation is consumed before the attempt, so a refused load does
    // not hand its stamp to the retry.
    let generation = {
        let mut generations = world
            .remove_resource::<SceneGenerations>()
            .unwrap_or_default();
        let next = generations.take_next();
        world.insert_resource(generations);
        next
    };

    // Prepare: `import_airframe` converts every transform before it spawns
    // anything, so a refusal leaves the world untouched.
    let import = match import_airframe(world, &graph, visual, generation) {
        Ok(import) => import,
        Err(error) => {
            log_scene_event(
                world,
                SceneEvent::Refused {
                    airframe,
                    root,
                    generation,
                    error,
                },
            );
            return;
        }
    };

    // Bind the sockets of the imported subtree. A socket outside the subtree
    // belongs to another airframe; a socket whose role is an explicit unknown
    // is reported instead of being given a role.
    let mut sockets: BTreeSet<SceneNodeId> = BTreeSet::new();
    let mut unresolved: Vec<SceneNodeId> = Vec::new();
    for socket in graph.sockets() {
        let Some(entity) = import.entity(socket.node()) else {
            // The socket is on another root of the container: another
            // airframe's part, not this scene's.
            continue;
        };
        if world.get_entity(entity).is_err() {
            continue;
        }
        match socket.role() {
            Resolved::Known(_) => {
                world.entity_mut(entity).insert(PartBinding {
                    role: socket.role().clone(),
                    collision: socket.collision().clone(),
                    zone_id: socket.zone_id(),
                    provenance: socket.provenance().clone(),
                });
                sockets.insert(socket.node().clone());
            }
            Resolved::Unknown { .. } => unresolved.push(socket.node().clone()),
        }
    }

    // Commit: the superseded scene goes only now, so a load that succeeded is
    // never the reason an aircraft loses its model.
    if let Some(previous) = world.remove_resource::<LiveAirframeScene>() {
        let released = release_scene(world, &previous);
        log_scene_event(
            world,
            SceneEvent::Released {
                airframe: previous.airframe().clone(),
                generation: previous.generation(),
                entities: released,
            },
        );
    }

    let nodes = import.len();
    let bound = sockets.len();
    world.insert_resource(LiveAirframeScene {
        airframe: airframe.clone(),
        root: root.clone(),
        generation,
        import,
        sockets,
        graph,
    });
    log_scene_event(
        world,
        SceneEvent::Loaded {
            airframe,
            root,
            generation,
            nodes,
            sockets: bound,
            unresolved,
        },
    );
}

/// Serves an [`AirframeSceneRequest::Unload`].
fn unload_airframe_scene(world: &mut World) {
    let Some(previous) = world.remove_resource::<LiveAirframeScene>() else {
        // Nothing is live: a repeated teardown, or a teardown after a
        // refused load, has nothing to release and is not an error.
        return;
    };
    let released = release_scene(world, &previous);
    log_scene_event(
        world,
        SceneEvent::Released {
            airframe: previous.airframe().clone(),
            generation: previous.generation(),
            entities: released,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::schedule::IntoScheduleConfigs;
    use bevy::prelude::{Children, Schedule};
    use cs_content::coordinates::SourceAdapter;
    use cs_content::scene::{
        AnimationBinding, AuthoredTransform, BindingMap, MeshBinding, ParsedNode, ParsedNodeKind,
        SemanticBinding,
    };
    use cs_types::content::{ContentKind, Known};
    use cs_types::evidence::ClaimId;

    /// The declared synthetic left-handed-centimeters-degrees adapter from
    /// F16-A's registry, the same one F11-A's acceptance fixture converts
    /// through: canonical X ← source +Y, canonical Y ← source +Z, canonical
    /// Z ← source −X, 0.01 m per unit.
    fn fixture_adapter() -> SourceAdapter {
        SourceAdapter::declared()
            .into_iter()
            .find(|adapter| {
                adapter.source().label() == "fixture.left-handed-z-up-centimeters-degrees"
            })
            .expect("the F16-A registry declares the left-handed centimeters fixture")
    }

    fn cid(kind: ContentKind, key: &str) -> ContentId {
        ContentId::from_source(kind, key).expect("test id is valid")
    }

    /// The stable id of a fixture node: `scene_node/<container>.<path>`.
    fn node_id(path: &str) -> SceneNodeId {
        SceneNodeId::from_content_id(cid(ContentKind::SceneNode, path)).expect("scene node id")
    }

    fn object(index: u32, name: &str, parent: u32) -> ParsedNode {
        let mut node = ParsedNode::new(index, name, ParsedNodeKind::Object3d);
        node.parent = Some(parent);
        node
    }

    /// The F11-B fixture: `main` → `body`, `wing` (whose `wing_lod0` /
    /// `wing_lod1` bands are its children, with the wing's gun beside them)
    /// and `tail` (its own band pair). Bands are authored in centimetres, so
    /// `10_000..30_000` is a 100..300 m band after conversion.
    fn lod_fixture() -> Vec<ParsedNode> {
        let mut main = ParsedNode::new(0, "main", ParsedNodeKind::World);
        main.children = vec![1, 2, 8];

        let body = object(1, "body", 0);

        let mut wing = object(2, "wing", 0);
        wing.children = vec![3, 5, 7];
        wing.transform.translation = [200.0, 0.0, 0.0];

        let mut wing_lod0 = ParsedNode::new(
            3,
            "wing_lod0",
            ParsedNodeKind::Lod {
                level: false,
                range_min: 0.0,
                range_max: 10_000.0,
            },
        );
        wing_lod0.parent = Some(2);
        wing_lod0.children = vec![4];
        let wing_mesh_near = object(4, "wing_mesh_near", 3);

        let mut wing_lod1 = ParsedNode::new(
            5,
            "wing_lod1",
            ParsedNodeKind::Lod {
                level: true,
                range_min: 10_000.0,
                range_max: 30_000.0,
            },
        );
        wing_lod1.parent = Some(2);
        wing_lod1.children = vec![6];
        let wing_mesh_far = object(6, "wing_mesh_far", 5);

        let gun = object(7, "gun", 2);

        let mut tail = object(8, "tail", 0);
        tail.children = vec![9, 10];
        let mut tail_lod0 = ParsedNode::new(
            9,
            "tail_lod0",
            ParsedNodeKind::Lod {
                level: false,
                range_min: 0.0,
                range_max: 10_000.0,
            },
        );
        tail_lod0.parent = Some(8);
        let mut tail_lod1 = ParsedNode::new(
            10,
            "tail_lod1",
            ParsedNodeKind::Lod {
                level: true,
                range_min: 10_000.0,
                range_max: 50_000.0,
            },
        );
        tail_lod1.parent = Some(8);

        vec![
            main,
            body,
            wing,
            wing_lod0,
            wing_mesh_near,
            wing_lod1,
            wing_mesh_far,
            gun,
            tail,
            tail_lod0,
            tail_lod1,
        ]
    }

    fn build_fixture_graph() -> SceneGraph {
        SceneGraph::build(
            &cid(ContentKind::InstallFile, "fix_planes"),
            &lod_fixture(),
            &fixture_adapter(),
            &BindingMap::default(),
        )
        .expect("the F11-B fixture converts")
    }

    /// The presentation verdict one imported node holds right now.
    fn presentation(world: &World, entity: Entity) -> PresentationState {
        world
            .get::<NodePresentation>(entity)
            .expect("an imported node carries a presentation record")
            .0
    }

    /// How many live entities the import produced. Counted off the binding
    /// rather than off `World`'s own bookkeeping, so the number is exactly
    /// what an import spawned.
    fn imported_count(world: &mut World) -> usize {
        let mut imported = world.query_filtered::<Entity, With<SceneNodeBinding>>();
        imported.iter(world).count()
    }

    /// Everything LOD selection must leave alone: the entity, the binding's
    /// stable id and generation, and the composed transform.
    fn snapshot(
        world: &World,
        graph: &SceneGraph,
        import: &SceneImport,
    ) -> Vec<(Entity, ContentId, SceneGeneration, [f32; 16])> {
        graph
            .nodes()
            .iter()
            .filter_map(|node| {
                let entity = import.entity(node.id())?;
                let binding = world.get::<SceneNodeBinding>(entity)?;
                let transform = world.get::<NodeVisualTransform>(entity)?;
                Some((
                    entity,
                    binding.node.clone(),
                    binding.generation,
                    transform.global().to_matrix().to_cols_array(),
                ))
            })
            .collect()
    }

    // ------------------------------------------------ F11-C test fixtures ---

    fn claim(id: &str) -> ClaimId {
        ClaimId::new(id).expect("test claim id is valid")
    }

    /// Every fixture rule is explicitly *designed* evidence: a synthetic
    /// mapping authored here, not something read from original data.
    fn designed(id: &str) -> Provenance {
        Provenance::designed(claim(id))
    }

    fn known<T>(value: T) -> Resolved<T> {
        Resolved::Known(Known::new(value, designed("f11c.test.rule")))
    }

    /// An explicit unknown: the fixture stands in for evidence that could not
    /// resolve the value, and nothing may default it.
    fn unmeasured<T>(id: &str, reason: &str) -> Resolved<T> {
        Resolved::unknown(claim(id), reason).expect("the unknown carries a reason")
    }

    fn rule(
        path: &str,
        role: Resolved<PartRole>,
        collision: Resolved<CollisionRole>,
    ) -> SemanticBinding {
        SemanticBinding {
            path: path.to_owned(),
            role,
            collision,
            animation: Vec::new(),
            provenance: designed("f11c.test.rule"),
        }
    }

    /// The F11-C fixture: the F11-B hierarchy plus the bindings F11-C binds —
    /// an engine, a damage zone with its own two LOD bands, a gun mounted
    /// under the wing, a pod whose role the evidence left unknown, a control
    /// surface and a camera anchor — and a second root `beta` with a gun of
    /// its own, so a load of `main` must not bind another airframe's socket.
    ///
    /// Stored slots: the F11-B fixture's 0..=10, then `pod` 11, `camera` 12,
    /// and the `beta` root 13 with `beta_gun` 14.
    fn bound_fixture() -> Vec<ParsedNode> {
        let mut nodes = lod_fixture();
        // `pod` is a second part under the wing, inside the subtree a
        // destroyed wing covers.
        let mut pod = object(11, "pod", 2);
        pod.zone_id = 9;
        nodes.push(pod);
        nodes[2].children = vec![3, 5, 7, 11];
        // A camera anchor under the root, and a mesh on the gun so the
        // association the renderer needs is reachable from the live record.
        let mut camera = ParsedNode::new(12, "camera", ParsedNodeKind::Camera);
        camera.parent = Some(0);
        nodes.push(camera);
        nodes[0].children = vec![1, 2, 8, 12];
        nodes[7].mesh = Some(MeshBinding {
            index: 7,
            mesh: known(cid(ContentKind::Mesh, "fix_planes.7")),
        });
        nodes[7].zone_id = 7;
        nodes[2].zone_id = 4;
        // A second root: another airframe in the same container.
        let mut beta = ParsedNode::new(13, "beta", ParsedNodeKind::World);
        beta.children = vec![14];
        nodes.push(beta);
        nodes.push(object(14, "beta_gun", 13));
        nodes
    }

    /// The binding table for [`bound_fixture`]: six evidenced rules inside
    /// `main` (one of them with an explicit unknown role), one rule on
    /// `beta`'s gun and one rule that names no node at all.
    fn bound_bindings() -> BindingMap {
        BindingMap::new(vec![
            rule(
                "main.body",
                known(PartRole::Engine),
                known(CollisionRole::Collider),
            ),
            rule(
                "main.wing",
                known(PartRole::DamageZone),
                known(CollisionRole::Collider),
            ),
            SemanticBinding {
                animation: vec![AnimationBinding {
                    channel: known(cid(ContentKind::AnimationTrack, "recoil")),
                }],
                ..rule(
                    "main.wing.gun",
                    known(PartRole::Gun),
                    known(CollisionRole::Collider),
                )
            },
            rule(
                "main.wing.pod",
                unmeasured(
                    "f11c.test.pod-role-unmeasured",
                    "the fixture's pod role was never evidenced",
                ),
                unmeasured(
                    "f11c.test.pod-collision-unmeasured",
                    "the fixture's pod collision role was never evidenced",
                ),
            ),
            rule(
                "main.tail",
                known(PartRole::ControlSurface),
                known(CollisionRole::None),
            ),
            rule(
                "main.camera",
                known(PartRole::CameraAnchor),
                known(CollisionRole::None),
            ),
            rule(
                "beta.beta_gun",
                known(PartRole::Gun),
                known(CollisionRole::Collider),
            ),
            rule(
                "main.absent",
                known(PartRole::DamageZone),
                known(CollisionRole::None),
            ),
        ])
        .expect("the fixture rules name distinct paths")
    }

    /// The converted container the F11-C tests load from: 15 nodes, of which
    /// 13 hang under the `main` root.
    fn build_bound_graph() -> SceneGraph {
        let nodes = bound_fixture();
        assert_eq!(nodes.len(), 15, "the fixture holds 15 nodes");
        SceneGraph::build(
            &cid(ContentKind::InstallFile, "fix_planes"),
            &nodes,
            &fixture_adapter(),
            &bound_bindings(),
        )
        .expect("the F11-C fixture converts")
    }

    /// The airframe visual of the `main` root, the one every load test uses.
    fn main_visual() -> AirframeVisual {
        AirframeVisual::new(
            cid(ContentKind::Airframe, "alpha"),
            cid(ContentKind::InstallFile, "fix_planes"),
            node_id("fix_planes.main"),
        )
        .expect("the main root reference is well formed")
    }

    /// A world with the viewer distance the LOD system needs, an empty
    /// recorded damage state and the scene systems installed in the order
    /// they must run in: the request first (a load publishes the live record
    /// the damage pass reads), then the damage markers, then presentation.
    fn scene_world() -> (World, Schedule) {
        let mut world = World::new();
        world.insert_resource(LodDistance::new(Meters(50.0)).expect("50 m is usable"));
        world.insert_resource(AirframeDamageState::new());
        let mut schedule = Schedule::default();
        schedule.add_systems(
            (
                process_airframe_scene_request,
                apply_airframe_damage,
                select_lod_presentation,
            )
                .chain(),
        );
        (world, schedule)
    }

    fn load(
        world: &mut World,
        schedule: &mut Schedule,
        visual: &AirframeVisual,
        graph: &Arc<SceneGraph>,
    ) {
        world.insert_resource(AirframeSceneRequest::load(
            visual.clone(),
            Arc::clone(graph),
        ));
        schedule.run(world);
    }

    fn unload(world: &mut World, schedule: &mut Schedule) {
        world.insert_resource(AirframeSceneRequest::unload());
        schedule.run(world);
    }

    /// How many entities the world holds in total, counted off the world
    /// itself.
    ///
    /// Bevy keeps entities of its own here — the placeholder, one per resource
    /// and the schedule's own bookkeeping — so the number is not the scene's
    /// size on its own; it is the *difference* between two moments that
    /// proves a leak, which is why the tests compare it with a baseline
    /// instead of with zero. The scene's own entities are counted exactly by
    /// [`imported_count`].
    fn live_entities(world: &World) -> usize {
        world.iter_entities().count()
    }

    /// The node ids the live scene's entities are bound to, sorted, with their
    /// generations.
    fn live_bindings(world: &mut World) -> Vec<(String, SceneGeneration)> {
        let mut bound: Vec<(String, SceneGeneration)> = world
            .query_filtered::<(Entity, &SceneNodeBinding), With<SceneNodeBinding>>()
            .iter(world)
            .map(|(_, binding)| (binding.node.key().to_owned(), binding.generation))
            .collect();
        bound.sort();
        bound
    }

    /// AC02's import half: every node becomes one generation-stamped entity,
    /// the authored hierarchy becomes `ChildOf`/`Children`, the composed
    /// canonical transform reaches the ECS affine, and LOD bands arrive in
    /// metres.
    #[test]
    fn accept_f11_b_import_scene_spawns_the_hierarchy_with_stable_ids() {
        let graph = build_fixture_graph();
        let mut world = World::new();
        let generation = SceneGeneration::default().next();
        let import = import_scene(&mut world, &graph, generation).expect("the fixture imports");

        assert_eq!(import.len(), graph.len());
        assert_eq!(import.len(), 11);
        assert_eq!(import.generation(), generation);
        assert!(!import.is_empty());
        assert_eq!(imported_count(&mut world), graph.len());

        let main = node_id("fix_planes.main");
        let wing = node_id("fix_planes.main.wing");
        let wing_lod0 = node_id("fix_planes.main.wing.wing_lod0");
        let wing_mesh_near = node_id("fix_planes.main.wing.wing_lod0.wing_mesh_near");
        let gun = node_id("fix_planes.main.wing.gun");

        let main_entity = import.entity(&main).expect("root entity");
        let wing_entity = import.entity(&wing).expect("wing entity");
        let wing_lod0_entity = import.entity(&wing_lod0).expect("band entity");
        let gun_entity = import.entity(&gun).expect("gun entity");

        // The root hangs from nothing; its descendants hang off their parent.
        assert!(world.get::<ChildOf>(main_entity).is_none());
        assert_eq!(
            world.get::<ChildOf>(wing_entity),
            Some(&ChildOf(main_entity))
        );
        assert_eq!(
            world.get::<ChildOf>(wing_lod0_entity),
            Some(&ChildOf(wing_entity))
        );
        let wing_children: Vec<Entity> = world
            .get::<Children>(wing_entity)
            .expect("wing has children")
            .iter()
            .copied()
            .collect();
        assert_eq!(wing_children.len(), 3, "two bands and the gun");
        assert!(wing_children.contains(&wing_lod0_entity));
        assert!(wing_children.contains(&gun_entity));

        // Identity and generation ride on every entity.
        for node in graph.nodes() {
            let entity = import.entity(node.id()).expect("every node is imported");
            let binding = world
                .get::<SceneNodeBinding>(entity)
                .expect("binding present");
            assert_eq!(binding.node, *node.id().as_content_id());
            assert_eq!(binding.generation, generation);
        }

        // Hand-computed canonical transform: the wing translates 200 cm along
        // source X, which the fixture adapter maps to −2 m canonical Z.
        let wing_transform = world
            .get::<NodeVisualTransform>(wing_entity)
            .expect("transform present")
            .global()
            .to_matrix();
        assert_eq!(
            wing_transform.w_axis.truncate().to_array(),
            [0.0, 0.0, -2.0],
            "the composed canonical translation reaches the ECS affine"
        );

        // LOD bands arrive with their converted range; non-LOD nodes carry
        // no band at all.
        let band = world
            .get::<NodeLodVariant>(wing_lod0_entity)
            .expect("a Lod node carries its band");
        assert!(!band.info().level);
        assert_eq!(band.info().range_min.0, 0.0);
        assert_eq!(band.info().range_max.0, 100.0);
        assert!(world.get::<NodeLodVariant>(wing_entity).is_none());
        assert!(
            world
                .get::<NodeLodVariant>(import.entity(&wing_mesh_near).expect("mesh entity"))
                .is_none()
        );

        // Presentation starts drawn; nothing is disabled by an import.
        for node in graph.nodes() {
            let entity = import.entity(node.id()).expect("imported");
            assert_eq!(presentation(&world, entity), PresentationState::Drawn);
            assert!(world.get::<NodeDisabled>(entity).is_none());
        }
    }

    /// The import refuses a hierarchy it cannot represent, and refuses it
    /// before spawning anything: a half-imported scene would leave orphaned
    /// entities with stale generations behind.
    #[test]
    fn accept_f11_b_import_scene_refuses_a_transform_that_cannot_be_rendered() {
        let mut main = ParsedNode::new(0, "main", ParsedNodeKind::World);
        main.children = vec![1];
        main.transform = AuthoredTransform {
            scale: [1e38, 1.0, 1.0],
            ..AuthoredTransform::IDENTITY
        };
        let mut child = object(1, "child", 0);
        child.transform = AuthoredTransform {
            scale: [1e38, 1.0, 1.0],
            ..AuthoredTransform::IDENTITY
        };

        let container = cid(ContentKind::InstallFile, "fix_planes");
        let graph = SceneGraph::build(
            &container,
            &[main, child],
            &fixture_adapter(),
            &BindingMap::default(),
        )
        .expect("both transforms are finite in f64");
        let mut world = World::new();

        // The root's composed scale still fits f32; the child's does not.
        assert_eq!(
            import_scene(&mut world, &graph, SceneGeneration::default().next()).map(|_| ()),
            Err(SceneImportError::Transform {
                node: node_id("fix_planes.main.child"),
                source: NodeTransformError::NotRepresentable,
            })
        );
        assert_eq!(
            imported_count(&mut world),
            0,
            "a refused import spawns nothing at all"
        );
    }

    /// The airframe half: an `AirframeVisual` names a root, and only that
    /// root's subtree is imported — never "the first root in the container".
    #[test]
    fn accept_f11_b_import_airframe_starts_from_the_root_reference() {
        let mut alpha = ParsedNode::new(0, "alpha", ParsedNodeKind::World);
        alpha.children = vec![1];
        let alpha_child = object(1, "alpha_child", 0);
        let mut beta = ParsedNode::new(2, "beta", ParsedNodeKind::World);
        beta.children = vec![3];
        let beta_child = object(3, "beta_child", 2);

        let container = cid(ContentKind::InstallFile, "fix_planes");
        let graph = SceneGraph::build(
            &container,
            &[alpha, alpha_child, beta, beta_child],
            &fixture_adapter(),
            &BindingMap::default(),
        )
        .expect("two roots convert");
        let airframe = cid(ContentKind::Airframe, "alpha");
        let root = node_id("fix_planes.alpha");
        let visual = AirframeVisual::new(airframe.clone(), container.clone(), root.clone())
            .expect("a root reference in this container");

        let mut world = World::new();
        let generation = SceneGeneration::default().next();
        let import = import_airframe(&mut world, &graph, &visual, generation)
            .expect("the referenced subtree imports");

        assert_eq!(import.len(), 2, "only alpha and its child");
        let alpha_entity = import.entity(&root).expect("root entity");
        let child_entity = import
            .entity(&node_id("fix_planes.alpha.alpha_child"))
            .expect("child entity");
        assert!(world.get::<ChildOf>(alpha_entity).is_none());
        assert_eq!(
            world.get::<ChildOf>(child_entity),
            Some(&ChildOf(alpha_entity))
        );
        assert!(
            import.entity(&node_id("fix_planes.beta")).is_none(),
            "the sibling root is not pulled in by position"
        );

        // A reference into another container is refused, not reinterpreted.
        let other = cid(ContentKind::InstallFile, "other");
        let foreign = AirframeVisual::new(airframe.clone(), other, node_id("other.alpha"))
            .expect("a root reference in that container");
        assert_eq!(
            import_airframe(&mut world, &graph, &foreign, generation).map(|_| ()),
            Err(SceneImportError::ForeignContainer {
                expected: "fix_planes".to_owned(),
                found: "other".to_owned(),
            })
        );

        // A well-formed reference to a node this graph does not hold is
        // refused by name.
        let missing = node_id("fix_planes.gamma");
        let unknown = AirframeVisual::new(airframe, container, missing.clone())
            .expect("a root reference in this container");
        assert_eq!(
            import_airframe(&mut world, &graph, &unknown, generation).map(|_| ()),
            Err(SceneImportError::UnknownNode { id: missing })
        );
        assert_eq!(
            imported_count(&mut world),
            2,
            "refused imports spawn nothing"
        );
    }

    /// AC02, the minimum scenario: a destroyed wing and its gun remain
    /// disabled across an LOD transition.
    ///
    /// The healthy `tail` group proves the transition really happens (its far
    /// band takes over at 150 m), while the destroyed wing — parent, both
    /// bands, both band meshes and the gun — stays `Disabled` at every
    /// distance, and the identity, generation and transform of every node are
    /// byte-identical before and after.
    #[test]
    fn accept_f11_b_destroyed_wing_and_gun_remain_disabled_across_lod_transition() {
        let graph = build_fixture_graph();
        let mut world = World::new();
        let generation = SceneGeneration::default().next();
        let import = import_scene(&mut world, &graph, generation).expect("the fixture imports");
        let mut schedule = Schedule::default();
        schedule.add_systems(select_lod_presentation);

        let entity = |path: &str| {
            import
                .entity(&node_id(path))
                .unwrap_or_else(|| panic!("fixture node {path} was imported"))
        };
        let wing = entity("fix_planes.main.wing");
        let wing_lod0 = entity("fix_planes.main.wing.wing_lod0");
        let wing_lod1 = entity("fix_planes.main.wing.wing_lod1");
        let wing_mesh_near = entity("fix_planes.main.wing.wing_lod0.wing_mesh_near");
        let wing_mesh_far = entity("fix_planes.main.wing.wing_lod1.wing_mesh_far");
        let gun = entity("fix_planes.main.wing.gun");
        let body = entity("fix_planes.main.body");
        let tail_lod0 = entity("fix_planes.main.tail.tail_lod0");
        let tail_lod1 = entity("fix_planes.main.tail.tail_lod1");

        // Baseline at 50 m: each group presents its near band.
        world.insert_resource(
            LodDistance::new(Meters(50.0)).expect("a finite non-negative distance"),
        );
        schedule.run(&mut world);
        assert_eq!(presentation(&world, wing_lod0), PresentationState::Drawn);
        assert_eq!(
            presentation(&world, wing_lod1),
            PresentationState::LodCulled
        );
        assert_eq!(presentation(&world, gun), PresentationState::Drawn);
        assert_eq!(presentation(&world, tail_lod0), PresentationState::Drawn);
        assert_eq!(
            presentation(&world, tail_lod1),
            PresentationState::LodCulled
        );
        assert_eq!(presentation(&world, body), PresentationState::Drawn);

        // The band's own verdict reaches the mesh that hangs under it: the
        // near mesh under the chosen band is drawn, the far mesh under the
        // culled band is culled with it, so two LOD levels of one wing are
        // never reported drawn at the same time.
        assert_eq!(
            presentation(&world, wing_mesh_near),
            PresentationState::Drawn
        );
        assert_eq!(
            presentation(&world, wing_mesh_far),
            PresentationState::LodCulled
        );

        let before = snapshot(&world, &graph, &import);

        // Destroy the wing and its gun.
        world.entity_mut(wing).insert(NodeDisabled);
        world.entity_mut(gun).insert(NodeDisabled);
        schedule.run(&mut world);

        // Damage wins over the band that is currently drawn, and it disables
        // the *other* band too — otherwise the next transition would bring
        // the wing back.
        assert_eq!(presentation(&world, wing), PresentationState::Disabled);
        assert_eq!(presentation(&world, wing_lod0), PresentationState::Disabled);
        assert_eq!(presentation(&world, wing_lod1), PresentationState::Disabled);
        assert_eq!(
            presentation(&world, wing_mesh_near),
            PresentationState::Disabled
        );
        assert_eq!(
            presentation(&world, wing_mesh_far),
            PresentationState::Disabled
        );
        assert_eq!(presentation(&world, gun), PresentationState::Disabled);
        assert_eq!(presentation(&world, body), PresentationState::Drawn);

        // The LOD transition: cross the 100 m edge between the two bands.
        world.insert_resource(
            LodDistance::new(Meters(150.0)).expect("a finite non-negative distance"),
        );
        schedule.run(&mut world);

        // The healthy group demonstrably switched bands, so the transition
        // above was real and the assertions below are not vacuous.
        assert_eq!(
            presentation(&world, tail_lod0),
            PresentationState::LodCulled
        );
        assert_eq!(presentation(&world, tail_lod1), PresentationState::Drawn);

        // A destroyed wing and its gun stay disabled across it — whichever
        // band the distance selects.
        assert_eq!(presentation(&world, wing), PresentationState::Disabled);
        assert_eq!(presentation(&world, wing_lod0), PresentationState::Disabled);
        assert_eq!(presentation(&world, wing_lod1), PresentationState::Disabled);
        assert_eq!(
            presentation(&world, wing_mesh_far),
            PresentationState::Disabled
        );
        assert_eq!(presentation(&world, gun), PresentationState::Disabled);

        // Nothing else moved: same entities, same stable ids, same
        // generation, same composed transform (collision and weapon origins
        // cannot jump with distance).
        assert_eq!(
            snapshot(&world, &graph, &import),
            before,
            "LOD selection may only change the presentation record"
        );
        assert!(world.get::<NodeDisabled>(wing).is_some());
        assert!(world.get::<NodeDisabled>(gun).is_some());
        let gun_node = graph
            .node(&node_id("fix_planes.main.wing.gun"))
            .expect("gun node");
        assert_eq!(
            world
                .get::<NodeVisualTransform>(gun)
                .expect("gun transform")
                .global()
                .to_matrix(),
            NodeVisualTransform::from_canonical(gun_node.collision_transform())
                .expect("representable")
                .global()
                .to_matrix(),
            "the render transform still equals the collision transform"
        );

        // Failure case: the state is recomputed every run, not sticky —
        // repairing the damage brings the current band back, not the stale
        // one.
        world.entity_mut(wing).remove::<NodeDisabled>();
        world.entity_mut(gun).remove::<NodeDisabled>();
        schedule.run(&mut world);
        assert_eq!(presentation(&world, wing_lod1), PresentationState::Drawn);
        assert_eq!(
            presentation(&world, wing_lod0),
            PresentationState::LodCulled
        );
        assert_eq!(presentation(&world, gun), PresentationState::Drawn);
        // Repaired wing, far band now chosen: the meshes swap verdicts with
        // their bands and neither is reported drawn twice.
        assert_eq!(
            presentation(&world, wing_mesh_near),
            PresentationState::LodCulled
        );
        assert_eq!(
            presentation(&world, wing_mesh_far),
            PresentationState::Drawn
        );
        assert_eq!(snapshot(&world, &graph, &import), before);
    }

    /// A mesh is presented exactly when the band it hangs under is the one
    /// its group chose: otherwise every band's mesh of the same part reports
    /// `Drawn` together and two LOD levels of one wing are drawn at once.
    #[test]
    fn accept_f11_b_a_mesh_under_a_culled_band_is_culled_with_it() {
        let graph = build_fixture_graph();
        let mut world = World::new();
        let generation = SceneGeneration::default().next();
        let import = import_scene(&mut world, &graph, generation).expect("the fixture imports");
        let mut schedule = Schedule::default();
        schedule.add_systems(select_lod_presentation);

        let near = import
            .entity(&node_id("fix_planes.main.wing.wing_lod0.wing_mesh_near"))
            .expect("near mesh entity");
        let far = import
            .entity(&node_id("fix_planes.main.wing.wing_lod1.wing_mesh_far"))
            .expect("far mesh entity");

        for (distance, expected_near, expected_far) in [
            (50.0, PresentationState::Drawn, PresentationState::LodCulled),
            (
                150.0,
                PresentationState::LodCulled,
                PresentationState::Drawn,
            ),
        ] {
            world.insert_resource(
                LodDistance::new(Meters(distance)).expect("a finite non-negative distance"),
            );
            schedule.run(&mut world);
            assert_eq!(
                presentation(&world, near),
                expected_near,
                "at {distance} m the near band is {expected_near:?}"
            );
            assert_eq!(
                presentation(&world, far),
                expected_far,
                "at {distance} m the far band is {expected_far:?}"
            );
        }
    }

    /// The validated inputs the system trusts: a distance that is not a
    /// usable number of metres, and a band whose range cannot decide
    /// anything, are refused at construction instead of reaching selection.
    #[test]
    fn accept_f11_b_lod_inputs_are_validated_at_construction() {
        for distance in [f64::NAN, f64::INFINITY] {
            assert_eq!(
                LodDistance::new(Meters(distance)).map(|_| ()),
                Err(LodDistanceError::NonFinite),
                "a distance of {distance} is not a number of metres"
            );
        }
        assert_eq!(
            LodDistance::new(Meters(-1.0)).map(|_| ()),
            Err(LodDistanceError::Negative)
        );
        let usable = LodDistance::new(Meters(12.5)).expect("finite non-negative");
        assert_eq!(usable.meters(), Meters(12.5));

        let good = LodInfo {
            level: false,
            range_min: Meters(0.0),
            range_max: Meters(100.0),
        };
        assert_eq!(NodeLodVariant::new(good).expect("usable band").info(), good);
        assert_eq!(
            NodeLodVariant::new(LodInfo {
                range_min: Meters(200.0),
                range_max: Meters(100.0),
                ..good
            })
            .map(|_| ()),
            Err(LodSelectError::Range { index: 0 })
        );
    }

    /// The binding record holds its stable id and generation; a generation
    /// bump marks a reload.
    #[test]
    fn accept_f11_a_scene_binding_carries_node_id_and_generation() {
        let node = ContentId::from_source(ContentKind::SceneNode, "planes.corsair.wing_l")
            .expect("valid id");
        let generation = SceneGeneration::default().next();
        let binding = SceneNodeBinding {
            node: node.clone(),
            generation,
        };
        assert_eq!(binding.node, node);
        assert_eq!(binding.generation, SceneGeneration(1));
        assert_eq!(generation.next(), SceneGeneration(2));
        // A reload's bindings are distinguishable from the previous scene's.
        assert_ne!(binding.generation, generation.next());
    }

    /// A mirrored canonical transform must reach the ECS intact: Bevy's
    /// `GlobalTransform` is a full affine, so the negative determinant and
    /// the authored translation survive the f64 → f32 conversion.
    #[test]
    fn accept_f11_a_visual_transform_preserves_mirror_and_translation() {
        // Mirror across canonical X, shifted +2 m in Y.
        let mirrored = CanonicalTransform::try_new(
            [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            [0.0, 2.0, 0.0],
        )
        .expect("finite transform");
        let component =
            NodeVisualTransform::from_canonical(&mirrored).expect("representable transform");
        assert!(component.mirrored());
        let matrix = component.global().to_matrix();
        // Column-major: x_axis is column 0; translation is column 3.
        assert_eq!(matrix.x_axis.truncate().to_array(), [-1.0, 0.0, 0.0]);
        assert_eq!(
            matrix.w_axis.truncate().to_array(),
            [0.0, 2.0, 0.0],
            "the authored translation survives"
        );

        // A transform that cannot fit f32 is refused, not silently made
        // infinite.
        let too_large =
            CanonicalTransform::try_new(CanonicalTransform::IDENTITY.linear(), [1e300, 0.0, 0.0])
                .expect("finite f64 transform");
        assert_eq!(
            NodeVisualTransform::from_canonical(&too_large).map(|_| ()),
            Err(NodeTransformError::NotRepresentable)
        );
    }

    // ------------------------------------------------- F11-C: AC03 wiring ---

    /// AC03, the minimum scenario: loading and unloading the same airframe a
    /// hundred times leaves the live entity count unchanged.
    ///
    /// Every round loads the referenced subtree, reflects the recorded damage
    /// and selects presentation, then unloads. The scene's own entities are
    /// counted exactly (off the binding) and the *world's* total is compared
    /// with the counts the first round established, so a leak anywhere —
    /// including one the scene's bookkeeping does not know about — shows up as
    /// growth. The generations count up, so no stamp is ever reused, and the
    /// wing is destroyed in round 1 and repaired in round 50, so the marker
    /// lifecycle runs inside the loop and the damage state has to survive both
    /// teardown boundaries.
    #[test]
    fn accept_f11_c_load_and_unload_the_same_airframe_hundred_times_without_leaking_entities() {
        let graph = Arc::new(build_bound_graph());
        let visual = main_visual();
        let (mut world, mut schedule) = scene_world();
        let wing = node_id("fix_planes.main.wing");
        let gun = node_id("fix_planes.main.wing.gun");
        let body = node_id("fix_planes.main.body");
        let subtree = 13;
        // The counts round 1 establishes; every later round must reproduce
        // them exactly.
        let mut loaded_total: Option<usize> = None;
        let mut resting_total: Option<usize> = None;

        for round in 1..=100u64 {
            if round == 1 {
                world
                    .get_resource_mut::<AirframeDamageState>()
                    .expect("the damage state is installed")
                    .destroy(wing.clone());
            }
            if round == 50 {
                assert!(
                    world
                        .get_resource_mut::<AirframeDamageState>()
                        .expect("the damage state is installed")
                        .repair(&wing),
                    "the wing was still destroyed before the repair"
                );
            }

            load(&mut world, &mut schedule, &visual, &graph);
            let live = world
                .get_resource::<LiveAirframeScene>()
                .expect("a load publishes a live scene")
                .clone();
            assert_eq!(live.generation(), SceneGeneration(round));
            assert_eq!(live.len(), subtree);
            assert_eq!(
                imported_count(&mut world),
                subtree,
                "round {round}: the load spawned exactly the referenced subtree"
            );
            match loaded_total {
                Some(expected) => assert_eq!(
                    live_entities(&world),
                    expected,
                    "round {round}: the world holds no more entities than after round 1's load"
                ),
                None => loaded_total = Some(live_entities(&world)),
            }
            assert_eq!(
                live_bindings(&mut world)
                    .iter()
                    .filter(|(_, generation)| *generation != live.generation())
                    .count(),
                0,
                "round {round}: no entity of an older generation survives"
            );

            // The damage really is reflected in the visuals, so the loop is
            // not just spawning and dropping identical trees.
            let wing_entity = live.entity(&wing).expect("wing entity");
            let gun_entity = live.entity(&gun).expect("gun entity");
            let body_entity = live.entity(&body).expect("body entity");
            if round < 50 {
                assert_eq!(
                    presentation(&world, wing_entity),
                    PresentationState::Disabled
                );
                assert_eq!(
                    presentation(&world, gun_entity),
                    PresentationState::Disabled
                );
            } else {
                assert_eq!(presentation(&world, wing_entity), PresentationState::Drawn);
                assert_eq!(presentation(&world, gun_entity), PresentationState::Drawn);
            }
            assert_eq!(presentation(&world, body_entity), PresentationState::Drawn);

            unload(&mut world, &mut schedule);
            assert_eq!(
                imported_count(&mut world),
                0,
                "round {round}: the unload released every node of the scene"
            );
            match resting_total {
                Some(expected) => assert_eq!(
                    live_entities(&world),
                    expected,
                    "round {round}: the world returned to its resting entity count"
                ),
                None => resting_total = Some(live_entities(&world)),
            }
            assert!(
                world.get_resource::<LiveAirframeScene>().is_none(),
                "round {round}: no live record outlives the unload"
            );
            assert!(
                world.get_resource::<AirframeDamageState>().is_some(),
                "round {round}: teardown does not clear recorded damage"
            );
        }

        let log = world
            .get_resource::<AirframeSceneLog>()
            .expect("the systems reported what they did");
        assert_eq!(log.len(), 200, "one Loaded and one Released per round");
        assert_eq!(
            log.events()
                .iter()
                .filter(|event| matches!(
                    event,
                    SceneEvent::Loaded {
                        generation: SceneGeneration(100),
                        ..
                    }
                ))
                .count(),
            1,
            "exactly one load published generation 100"
        );
        assert_eq!(
            world.resource::<SceneGenerations>().latest(),
            SceneGeneration(100),
            "every load consumed its own generation"
        );
        // A hundred cycles leave the world exactly where one cycle did: the
        // scene's own entities are all gone and the rest is unchanged.
        let resting = resting_total.expect("the loop ran");
        assert_eq!(live_entities(&world), resting);
        assert_eq!(
            loaded_total.expect("the loop ran") - resting,
            subtree,
            "a loaded scene is exactly the subtree more than a resting world"
        );
    }

    /// A reload is the session-generation hand-over (F11 non-negotiable
    /// behavior 5): the superseded scene is released, no hidden old root
    /// survives, the recorded damage follows the new generation instead of
    /// being re-decided, and a second unload after the reload is a no-op.
    #[test]
    fn accept_f11_c_reload_releases_the_superseded_generation_and_leaves_no_old_root() {
        let graph = Arc::new(build_bound_graph());
        let visual = main_visual();
        let (mut world, mut schedule) = scene_world();
        let wing = node_id("fix_planes.main.wing");
        let gun = node_id("fix_planes.main.wing.gun");

        world
            .get_resource_mut::<AirframeDamageState>()
            .expect("the damage state is installed")
            .destroy(wing.clone());
        load(&mut world, &mut schedule, &visual, &graph);
        let old_root = world
            .get_resource::<LiveAirframeScene>()
            .and_then(|live| live.entity(&node_id("fix_planes.main")))
            .expect("the root entity of generation 1");
        assert_eq!(imported_count(&mut world), 13);

        // Reload the same airframe: a new generation, a fresh tree.
        load(&mut world, &mut schedule, &visual, &graph);
        let live = world
            .get_resource::<LiveAirframeScene>()
            .expect("the reload published a live scene")
            .clone();
        assert_eq!(live.generation(), SceneGeneration(2));
        assert_eq!(imported_count(&mut world), 13, "the old tree is gone");
        assert!(
            world.get_entity(old_root).is_err(),
            "the superseded root entity is despawned, not hidden"
        );
        assert_eq!(
            live_bindings(&mut world)
                .iter()
                .map(|(_, generation)| *generation)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([SceneGeneration(2)]),
            "every live entity carries the new generation only"
        );
        assert!(
            live.entity(&node_id("fix_planes.main")) != Some(old_root),
            "the reload spawned a new root instead of reusing the old one"
        );

        // The damage recorded against the first generation still holds on the
        // second: the part does not silently heal across a reload.
        let wing_entity = live.entity(&wing).expect("wing entity");
        assert_eq!(
            presentation(&world, wing_entity),
            PresentationState::Disabled
        );
        assert_eq!(
            presentation(&world, live.entity(&gun).expect("gun entity")),
            PresentationState::Disabled
        );

        let log = world.resource::<AirframeSceneLog>().clone();
        assert_eq!(
            log.events(),
            &[
                SceneEvent::Loaded {
                    airframe: cid(ContentKind::Airframe, "alpha"),
                    root: node_id("fix_planes.main"),
                    generation: SceneGeneration(1),
                    nodes: 13,
                    sockets: 5,
                    unresolved: vec![node_id("fix_planes.main.wing.pod")],
                },
                SceneEvent::Released {
                    airframe: cid(ContentKind::Airframe, "alpha"),
                    generation: SceneGeneration(1),
                    entities: 13,
                },
                SceneEvent::Loaded {
                    airframe: cid(ContentKind::Airframe, "alpha"),
                    root: node_id("fix_planes.main"),
                    generation: SceneGeneration(2),
                    nodes: 13,
                    sockets: 5,
                    unresolved: vec![node_id("fix_planes.main.wing.pod")],
                },
            ],
            "the reload released the superseded generation and then published the new one"
        );

        // Teardown is idempotent: the explicit unload releases the live tree
        // and a second one has nothing to do and does not fail.
        let before_teardown = world.resource::<AirframeSceneLog>().len();
        unload(&mut world, &mut schedule);
        assert_eq!(
            world.resource::<AirframeSceneLog>().len(),
            before_teardown + 1,
            "the unload released the live scene"
        );
        assert_eq!(imported_count(&mut world), 0);
        unload(&mut world, &mut schedule);
        assert_eq!(
            world.resource::<AirframeSceneLog>().len(),
            before_teardown + 1,
            "unloading an empty world reports nothing and changes nothing"
        );
        assert_eq!(imported_count(&mut world), 0);
    }

    /// Error propagation and retry: a refused load says exactly what went
    /// wrong, spawns nothing, leaves the running scene exactly as it was, and
    /// a corrected request then succeeds under a generation of its own.
    #[test]
    fn accept_f11_c_a_refused_load_propagates_its_error_and_leaves_the_running_scene_alone() {
        let graph = Arc::new(build_bound_graph());
        let visual = main_visual();
        let (mut world, mut schedule) = scene_world();
        let foreign = AirframeVisual::new(
            cid(ContentKind::Airframe, "gamma"),
            cid(ContentKind::InstallFile, "other"),
            node_id("other.gamma"),
        )
        .expect("a well-formed reference into another container");
        let missing = AirframeVisual::new(
            cid(ContentKind::Airframe, "delta"),
            cid(ContentKind::InstallFile, "fix_planes"),
            node_id("fix_planes.delta"),
        )
        .expect("a well-formed reference to an absent root");
        // A hierarchy whose composed scale overflows the render affine: the
        // root and its child each scale by 1e38, so the composition is 1e76.
        let mut main = ParsedNode::new(0, "main", ParsedNodeKind::World);
        main.children = vec![1];
        main.transform = AuthoredTransform {
            scale: [1e38, 1.0, 1.0],
            ..AuthoredTransform::IDENTITY
        };
        let mut child = object(1, "child", 0);
        child.transform = AuthoredTransform {
            scale: [1e38, 1.0, 1.0],
            ..AuthoredTransform::IDENTITY
        };
        let unrepresentable = AirframeVisual::new(
            cid(ContentKind::Airframe, "epsilon"),
            cid(ContentKind::InstallFile, "fix_planes"),
            node_id("fix_planes.main"),
        )
        .expect("a well-formed reference into the broken container");
        let broken = Arc::new(
            SceneGraph::build(
                &cid(ContentKind::InstallFile, "fix_planes"),
                &[main, child],
                &fixture_adapter(),
                &BindingMap::default(),
            )
            .expect("the transform is finite in f64"),
        );

        // A refusal before anything is live leaves nothing behind at all.
        load(&mut world, &mut schedule, &foreign, &graph);
        assert_eq!(imported_count(&mut world), 0);
        assert!(world.get_resource::<LiveAirframeScene>().is_none());

        load(&mut world, &mut schedule, &visual, &graph);
        let live = world
            .get_resource::<LiveAirframeScene>()
            .expect("a load publishes a live scene")
            .clone();
        assert_eq!(live.generation(), SceneGeneration(2));

        // Three refusals: a foreign container, an unknown root and a
        // transform the render affine cannot hold. Each propagates its own
        // error, spawns nothing and keeps the running scene. The first two
        // generations were spent by the refusal above and the load below, so
        // these attempts take 3, 4 and 5.
        for (expected_generation, (request_graph, request_visual, error)) in (3..).zip([
            (
                Arc::clone(&graph),
                &foreign,
                SceneImportError::ForeignContainer {
                    expected: "fix_planes".to_owned(),
                    found: "other".to_owned(),
                },
            ),
            (
                Arc::clone(&graph),
                &missing,
                SceneImportError::UnknownNode {
                    id: node_id("fix_planes.delta"),
                },
            ),
            (
                Arc::clone(&broken),
                &unrepresentable,
                SceneImportError::Transform {
                    node: node_id("fix_planes.main.child"),
                    source: NodeTransformError::NotRepresentable,
                },
            ),
        ]) {
            let before = live_bindings(&mut world);
            world.insert_resource(AirframeSceneRequest::load(
                request_visual.clone(),
                request_graph,
            ));
            schedule.run(&mut world);
            let log = world.resource::<AirframeSceneLog>();
            let event = log.last().expect("the refusal was reported");
            assert_eq!(
                event.generation(),
                Some(SceneGeneration(expected_generation)),
                "a refused attempt consumes a generation of its own"
            );
            assert!(
                matches!(event, SceneEvent::Refused { error: reported, .. } if *reported == error),
                "the refusal propagates {error:?}, got {event:?}"
            );
            let still_live = world
                .get_resource::<LiveAirframeScene>()
                .expect("the running scene survives a refused load");
            assert_eq!(still_live, &live, "the live record is untouched");
            assert_eq!(
                live_bindings(&mut world),
                before,
                "a refused load spawns nothing and removes nothing"
            );
            assert_eq!(imported_count(&mut world), 13);
        }

        // The retry succeeds, and under a generation no earlier attempt used.
        load(&mut world, &mut schedule, &visual, &graph);
        let retried = world
            .get_resource::<LiveAirframeScene>()
            .expect("the retry published a live scene");
        assert_eq!(retried.generation(), SceneGeneration(6));
        assert_eq!(retried.airframe(), live.airframe());
        assert_eq!(retried.root(), live.root());
        assert_eq!(retried.len(), live.len());
        assert_eq!(
            retried.sockets().collect::<Vec<_>>(),
            live.sockets().collect::<Vec<_>>(),
            "the retry bound the same parts"
        );
        assert!(
            live_bindings(&mut world)
                .iter()
                .all(|(_, generation)| *generation == SceneGeneration(6)),
            "the retry's entities all carry its own generation"
        );
        assert!(matches!(
            world.resource::<AirframeSceneLog>().last(),
            Some(SceneEvent::Loaded { .. })
        ));
    }

    /// Parts and sockets are bound by identity, not by position: the gun of
    /// the loaded airframe carries its evidenced role, the pod whose role was
    /// never evidenced is reported instead of being given one, the other
    /// root's gun is not bound at all, and an LOD transition changes the
    /// presentation of a socket without moving its identity or its pose.
    #[test]
    fn accept_f11_c_sockets_are_bound_by_identity_and_survive_an_lod_transition() {
        let graph = Arc::new(build_bound_graph());
        let visual = main_visual();
        let (mut world, mut schedule) = scene_world();
        let gun = node_id("fix_planes.main.wing.gun");
        let pod = node_id("fix_planes.main.wing.pod");
        let body = node_id("fix_planes.main.body");
        let beta_gun = node_id("fix_planes.beta.beta_gun");

        load(&mut world, &mut schedule, &visual, &graph);
        let live = world
            .get_resource::<LiveAirframeScene>()
            .expect("a load publishes a live scene");
        assert_eq!(
            live.sockets().cloned().collect::<Vec<_>>(),
            vec![
                node_id("fix_planes.main.body"),
                node_id("fix_planes.main.camera"),
                node_id("fix_planes.main.tail"),
                node_id("fix_planes.main.wing"),
                gun.clone(),
            ],
            "only the evidenced sockets of the loaded subtree are bound"
        );

        // The gun is a gun: role, collision role, zone and the rule's
        // provenance all come from the content record.
        let gun_entity = live.entity(&gun).expect("the gun is imported");
        let binding = world
            .get::<PartBinding>(gun_entity)
            .expect("a bound socket carries its part binding");
        assert_eq!(binding.known_role(), Some(PartRole::Gun));
        assert_eq!(*binding.role(), known(PartRole::Gun));
        assert_eq!(*binding.collision(), known(CollisionRole::Collider));
        assert_eq!(binding.zone_id(), 7);
        assert_eq!(binding.provenance(), &designed("f11c.test.rule"));
        assert_eq!(
            live.socket(&gun).map(|socket| socket.known_role()),
            Some(Some(PartRole::Gun)),
            "the live record reaches the socket by stable id"
        );
        assert_eq!(
            live.socket(&gun)
                .expect("the gun's socket")
                .animation()
                .len(),
            1,
            "the animation channel the rule bound is reachable from the live record"
        );
        assert_eq!(
            live.graph()
                .node(&gun)
                .and_then(|node| node.mesh().cloned()),
            live.graph()
                .sockets()
                .find(|socket| socket.node() == &gun)
                .map(|_| live
                    .graph()
                    .node(&gun)
                    .and_then(|node| node.mesh().cloned()))
                .unwrap(),
            "the mesh association a renderer needs stays on the content node"
        );
        assert!(live.graph().node(&gun).expect("gun node").mesh().is_some());

        // The pose is the node's one composed transform, the same value
        // collision evaluates: a socket never carries a second pose.
        let socket_pose = live
            .socket(&gun)
            .expect("the gun's socket")
            .pose()
            .translation();
        let node_pose = live
            .graph()
            .node(&gun)
            .expect("gun node")
            .collision_transform()
            .translation();
        assert_eq!(socket_pose, node_pose);
        assert_eq!(
            world
                .get::<NodeVisualTransform>(gun_entity)
                .expect("transform")
                .global()
                .to_matrix()
                .w_axis
                .truncate()
                .to_array(),
            [
                node_pose[0] as f32,
                node_pose[1] as f32,
                node_pose[2] as f32
            ],
            "the ECS affine is that same pose"
        );

        // The pod's role was never evidenced: no binding, and the loader
        // reported it instead of defaulting a role.
        let pod_entity = live.entity(&pod).expect("the pod is imported");
        assert!(world.get::<PartBinding>(pod_entity).is_none());
        let unresolved = live
            .socket(&pod)
            .expect("the pod is still a socket record")
            .role();
        assert!(!unresolved.is_known());
        assert!(
            live.socket(&pod)
                .and_then(|socket| socket.known_role())
                .is_none(),
            "an unmeasured role is not defaulted"
        );
        // Another airframe's socket is not bound into this scene, and that
        // root's nodes are not imported at all.
        assert!(live.entity(&beta_gun).is_none());
        assert!(live.socket(&beta_gun).is_none());

        // An LOD transition changes presentation only: identity, generation,
        // role and pose of every socket are byte-identical across it.
        let before: Vec<(SceneNodeId, PartRole, [f32; 16], SceneGeneration)> = live
            .sockets()
            .map(|node| {
                let entity = live.entity(node).expect("a socket is imported");
                let part = world.get::<PartBinding>(entity).expect("bound socket");
                (
                    node.clone(),
                    part.known_role().expect("a bound socket has a role"),
                    world
                        .get::<NodeVisualTransform>(entity)
                        .expect("transform")
                        .global()
                        .to_matrix()
                        .to_cols_array(),
                    world
                        .get::<SceneNodeBinding>(entity)
                        .expect("binding")
                        .generation,
                )
            })
            .collect();
        world.insert_resource(LodDistance::new(Meters(150.0)).expect("150 m is usable"));
        schedule.run(&mut world);
        let live = world
            .get_resource::<LiveAirframeScene>()
            .expect("the live scene is untouched by a distance change");
        let after: Vec<(SceneNodeId, PartRole, [f32; 16], SceneGeneration)> = live
            .sockets()
            .map(|node| {
                let entity = live.entity(node).expect("a socket is imported");
                let part = world.get::<PartBinding>(entity).expect("bound socket");
                (
                    node.clone(),
                    part.known_role().expect("a bound socket has a role"),
                    world
                        .get::<NodeVisualTransform>(entity)
                        .expect("transform")
                        .global()
                        .to_matrix()
                        .to_cols_array(),
                    world
                        .get::<SceneNodeBinding>(entity)
                        .expect("binding")
                        .generation,
                )
            })
            .collect();
        assert_eq!(after, before, "a distance moves no socket's identity");

        // The transition itself is real: the tail's far band took over at
        // 150 m, and the gun mounted under the wing is still drawn with it.
        let tail_lod0 = live
            .entity(&node_id("fix_planes.main.tail.tail_lod0"))
            .expect("tail band");
        let tail_lod1 = live
            .entity(&node_id("fix_planes.main.tail.tail_lod1"))
            .expect("tail band");
        assert_eq!(
            presentation(&world, tail_lod0),
            PresentationState::LodCulled
        );
        assert_eq!(presentation(&world, tail_lod1), PresentationState::Drawn);
        assert_eq!(presentation(&world, gun_entity), PresentationState::Drawn);
        assert_eq!(
            presentation(&world, live.entity(&body).expect("engine")),
            PresentationState::Drawn
        );
    }

    /// Damage visuals: the recorded state owns the `NodeDisabled` markers of
    /// the live generation, a destroyed part disables its whole subtree
    /// through presentation, a repair clears the marker and brings the part
    /// back, the pass is idempotent, an id that names no node is reported, and
    /// damage recorded before the first load is applied by it.
    #[test]
    fn accept_f11_c_damage_marks_and_repairs_the_bound_part_subtree() {
        let graph = Arc::new(build_bound_graph());
        let visual = main_visual();
        let (mut world, mut schedule) = scene_world();
        let wing = node_id("fix_planes.main.wing");
        let gun = node_id("fix_planes.main.wing.gun");
        let pod = node_id("fix_planes.main.wing.pod");
        let mesh_far = node_id("fix_planes.main.wing.wing_lod1.wing_mesh_far");
        let body = node_id("fix_planes.main.body");
        let unknown = node_id("fix_planes.omega");

        // Damage recorded before anything is loaded is not lost: the first
        // load's damage pass applies it to the fresh generation.
        world
            .get_resource_mut::<AirframeDamageState>()
            .expect("the damage state is installed")
            .destroy(wing.clone());
        load(&mut world, &mut schedule, &visual, &graph);
        let live = world
            .get_resource::<LiveAirframeScene>()
            .expect("a load publishes a live scene")
            .clone();
        let wing_entity = live.entity(&wing).expect("wing entity");
        let gun_entity = live.entity(&gun).expect("gun entity");
        let pod_entity = live.entity(&pod).expect("pod entity");
        let mesh_far_entity = live.entity(&mesh_far).expect("far mesh entity");
        let body_entity = live.entity(&body).expect("body entity");

        // The marker is the damage identity on the part that was named; the
        // *visual* disablement is subtree-wide, which is how one destroyed
        // wing covers its pod, both LOD bands and the gun mounted on it.
        assert!(world.get::<NodeDisabled>(wing_entity).is_some());
        assert!(
            world.get::<NodeDisabled>(gun_entity).is_none(),
            "the gun is not named by the damage; it is disabled through its ancestor"
        );
        for entity in [wing_entity, pod_entity, mesh_far_entity, gun_entity] {
            assert_eq!(presentation(&world, entity), PresentationState::Disabled);
        }
        assert!(world.get::<NodeDisabled>(body_entity).is_none());
        assert_eq!(presentation(&world, body_entity), PresentationState::Drawn);

        // Idempotent: running the pass again over the same state changes
        // nothing. The request resource is gone, so this run is exactly the
        // damage pass plus presentation.
        let damaged: Vec<PresentationState> = live
            .import()
            .entities()
            .map(|(_, entity)| presentation(&world, entity))
            .collect();
        let events = world.resource::<AirframeSceneLog>().len();
        schedule.run(&mut world);
        assert_eq!(
            live.import()
                .entities()
                .map(|(_, entity)| presentation(&world, entity))
                .collect::<Vec<_>>(),
            damaged,
            "the damage pass is convergent"
        );
        assert_eq!(
            world.resource::<AirframeSceneLog>().len(),
            events,
            "a converged pass reports nothing"
        );

        // An id that names no node of the live scene is reported, and it does
        // not disable anything.
        world
            .get_resource_mut::<AirframeDamageState>()
            .expect("the damage state is installed")
            .destroy(unknown.clone());
        schedule.run(&mut world);
        assert_eq!(
            world.resource::<AirframeSceneLog>().last(),
            Some(&SceneEvent::UnknownDamage {
                ids: vec![unknown.clone()]
            })
        );
        assert_eq!(
            presentation(&world, wing_entity),
            PresentationState::Disabled
        );
        assert!(
            world
                .get_resource::<AirframeDamageState>()
                .expect("state")
                .is_destroyed(&unknown),
            "the unresolvable id is kept, not dropped"
        );

        // Repairing the wing clears the marker and brings the subtree back at
        // the distance the viewer is at.
        assert!(
            world
                .get_resource_mut::<AirframeDamageState>()
                .expect("the damage state is installed")
                .repair(&wing)
        );
        schedule.run(&mut world);
        assert!(
            world.get::<NodeDisabled>(wing_entity).is_none(),
            "a stale marker cannot survive a repair"
        );
        for entity in [wing_entity, pod_entity, gun_entity] {
            assert_eq!(
                presentation(&world, entity),
                PresentationState::Drawn,
                "a repaired part is presented again"
            );
        }
        // The far band is culled because the viewer is at 50 m, not because
        // of damage: the damage is gone and the distance verdict is back.
        assert_eq!(
            presentation(&world, mesh_far_entity),
            PresentationState::LodCulled
        );

        // With no live scene the pass is a no-op: a part cannot be damaged
        // into a scene that does not exist, and the state is kept for the
        // next load rather than cleared.
        world
            .get_resource_mut::<AirframeDamageState>()
            .expect("the damage state is installed")
            .destroy(wing.clone());
        unload(&mut world, &mut schedule);
        let events = world.resource::<AirframeSceneLog>().len();
        apply_airframe_damage(&mut world);
        assert_eq!(imported_count(&mut world), 0);
        assert_eq!(
            world.resource::<AirframeSceneLog>().len(),
            events,
            "damage with no live scene reports nothing"
        );
        assert!(
            world
                .get_resource::<AirframeDamageState>()
                .expect("the state is kept")
                .is_destroyed(&wing)
        );
    }
}
