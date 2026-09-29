//! ECS binding records for canonical scene nodes
//! (`specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`, stage
//! `### F11-A`; shared contract `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! These are the typed *outputs* a scene consumer binds into the ECS — the
//! records later stages attach to spawned entities. No system runs here yet;
//! F11-B/C own import, LOD selection and spawn wiring.
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
//! under it stays `Disabled` whichever band becomes active, and the system
//! never writes an identity, a transform, a binding or a disable marker, so
//! collision, weapon origins and damage identity cannot move with distance
//! (F11 non-negotiable behavior 4; AC02).

use std::collections::{BTreeMap, HashMap, HashSet};

use bevy::ecs::component::Component;
use bevy::math::Mat4;
use bevy::prelude::{ChildOf, Entity, GlobalTransform, Query, Res, Resource, With, World};
use cs_content::scene::{
    CanonicalTransform, LodInfo, LodSelectError, SceneGraph, SceneNode, SceneNodeId,
    select_lod_variant,
};
use cs_types::content::ContentId;
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
/// instead of by array position (F11 deliverable); it records what was
/// spawned and never spawns or despawns by itself — teardown and reload
/// lifetime are F11-C's.
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
    /// Another variant of the node's LOD group is presented at this distance.
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
/// ancestor carries [`NodeDisabled`], otherwise `LodCulled` when another
/// band of its group was chosen, otherwise `Drawn`.
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

    // One pass: the parent links, the variant groups (siblings of one parent)
    // and the node list the second pass walks.
    let mut parent_of: HashMap<Entity, Entity> = HashMap::new();
    let mut groups: HashMap<Option<Entity>, Vec<(Entity, LodInfo)>> = HashMap::new();
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
    // disables the gun mounted under it without naming the gun twice.
    let mut affected: HashSet<Entity> = HashSet::new();
    for (entity, _) in &imported {
        let mut cursor = Some(*entity);
        let mut steps = 0usize;
        while let Some(current) = cursor {
            if disabled.contains(current) {
                affected.insert(*entity);
                break;
            }
            steps += 1;
            if steps > parent_of.len() {
                break;
            }
            cursor = parent_of.get(&current).copied();
        }
    }

    // The only write in the system.
    for (entity, variant) in imported {
        let Ok(mut node) = presentation.get_mut(entity) else {
            continue;
        };
        node.0 = if affected.contains(&entity) {
            PresentationState::Disabled
        } else if variant.is_some() {
            let parent = parent_of.get(&entity).copied();
            if selected.get(&parent) == Some(&entity) {
                PresentationState::Drawn
            } else {
                PresentationState::LodCulled
            }
        } else {
            PresentationState::Drawn
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::{Children, Schedule};
    use cs_content::coordinates::SourceAdapter;
    use cs_content::scene::{AuthoredTransform, BindingMap, ParsedNode, ParsedNodeKind};
    use cs_types::content::ContentKind;

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
        assert_eq!(snapshot(&world, &graph, &import), before);
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
}
