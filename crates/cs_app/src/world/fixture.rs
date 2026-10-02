//! The synthetic world fixtures and the swept probe (F18-A, F18-B, F18-C).
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stages `### F18-A`, `### F18-B` and `### F18-C`.
//!
//! Like [`crate::synthetic`] and [`crate::physics::fixture`], this is
//! **production bootstrap code**, not a test-only reimplementation: it authors
//! a [`WorldDefinition`] through the same records a real importer will
//! produce, and [`spawn_swept_probe`] spawns a body through the same Avian
//! components the runtime uses. The acceptance tests drive these functions;
//! they do not carry their own world builder.
//!
//! # The arch
//!
//! ```text
//!        z
//!        ^      +------------------+
//!        |      |      lintel      |        y
//!        |      +----+      +------+        ^
//!        |      |    |      |    |          |
//!   -----+------+------+----+----+-----> x  |
//!        |  L |      open- |  R |           |
//!        |    |     ing    |    |           |
//!   -----+----+------------+----+-----      |
//!              |    ground slab    |
//! ```
//!
//! The opening spans `z ∈ (-1, 1)` and `y ∈ (0, 3)`; the legs and the lintel
//! are 1 m thick along the flight axis `x`. The opening is defined by the
//! *same* box records the colliders are built from, so "the hole in the
//! render" and "the hole in the collision" cannot be authored separately: a
//! body that fits through the visual gap also fits through the collision gap.
//!
//! # The harbor world
//!
//! [`harbor_world`] is the F18-B fixture: the arch **as one stored mesh**, so
//! the only way its opening can survive the world path is if the collision
//! really is built from the object's own geometry. A convex hull of the same
//! corners is a closed box, so a substituted shape would be visible in the
//! triangle count. Its other objects exercise the rest of the load: a bounded
//! water patch, a sensor volume, a non-colliding banner, a ground slab that
//! belongs to two sectors, and one object whose mesh this source deliberately
//! does not hold.
//!
//! # The twin hangar world
//!
//! [`twin_harbor_world`] is the F18-B fixture for asset **sharing**: four object
//! records name **one** stored mesh — two solids, a presentation-only banner and
//! a trigger volume — while a fifth names a mesh of its own and a sixth is a
//! cuboid that needs none. It is the only way "one engine asset per named mesh"
//! is a claim about the world and not about one fixture, because it puts all four
//! mesh consumers on the same reference at once.
//!
//! # The depot world
//!
//! [`depot_world`] is the F18-C fixture: the same arch, with a **door panel**
//! that fills its opening, a **sensor volume** in front of the panel to be an
//! overlay's trigger, and a third sector in the east that holds nothing
//! gameplay needs. The panel is a cuboid, so its visual and its collider are
//! two entities and an overlay that moved one and not the other is visible;
//! the east sector is what a visibility pass is allowed to stream away, next to
//! a sector it is not.
//!
//! # Units
//!
//! A stored mesh's vertex positions are in **stored** units
//! (`cs_content::mesh` is explicit that it applies no scale), and the original's
//! world-vertex scale is unmeasured. These fixtures author their geometry in
//! metres by construction, so a position and the object's authored transform
//! speak the same unit; that is a property of the fixture, not a conversion this
//! stage claims to perform. See
//! `docs/findings/2026-09-30-f18-b-world-import-and-static-collision.md`.
//!
//! Everything here is newly authored synthetic fixture content
//! (`Origin::SyntheticFixture`); it never claims to be original geometry.
//! Which geometry the original worlds contain, and how they store sectors, is
//! unmeasured — see
//! `docs/findings/2026-09-30-f18-a-world-instances-sectors-and-collision-roles.md`.

use std::collections::BTreeSet;
use std::time::Duration;

use avian3d::prelude::{
    AngularVelocity, Collider, CollisionEventsEnabled, CollisionLayers as AvianCollisionLayers,
    Gravity, LinearVelocity, Mass, Position, RigidBody, Rotation, SpeculativeMargin, SubstepCount,
    SweptCcd,
};
use bevy::prelude::{App, Entity, Transform, Vec3};
use bevy::time::{Real, Time, TimeUpdateStrategy};
use cs_content::mesh::{MeshPresentationUnknown, RenderMesh};
use cs_content::scene::CanonicalTransform;
use cs_content::world::{
    Aabb, MissionOverlay, OverlayEffect, Sector, SectorId, SurfaceRole, WorldBoundary,
    WorldCollisionRole, WorldCollisionShape, WorldDefinition, WorldError, WorldId, WorldInstance,
    WorldObjectId, WorldObjectInstance, WorldPopulation,
};
use cs_formats::gamez::{PrimitiveKind, RawCorner, RawMesh, RawPolygon};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

use super::meshes::WorldMeshes;
use super::spawn::{SpawnedWorld, avian_layers};
use cs_sim::collision::{CollisionLayer, CollisionLayers};

// ------------------------------------------------------------- arch metrics ---

/// Half extents of one arch leg: 1 m thick along the flight axis, 3 m tall,
/// 1 m deep.
pub const ARCH_LEG_HALF_M: [f64; 3] = [0.5, 1.5, 0.5];

/// How far each leg's centre sits from the opening's centreline in `z`.
///
/// The opening therefore spans `z ∈ (-1, 1)`: `ARCH_LEG_Z_M - ARCH_LEG_HALF_M[2]`.
pub const ARCH_LEG_Z_M: f64 = 1.5;

/// Half extents of the lintel that closes the arch above the opening.
pub const ARCH_LINTEL_HALF_M: [f64; 3] = [0.5, 0.5, 2.0];

/// The lintel's centre: its underside is exactly [`ARCH_OPENING_TOP_M`].
pub const ARCH_LINTEL_POS_M: [f64; 3] = [0.0, 3.5, 0.0];

/// The opening's half-width in `z`.
pub const ARCH_OPENING_HALF_Z_M: f64 = 1.0;

/// The opening's height: the ground surface is `y = 0`, the lintel's
/// underside is `y = 3`.
pub const ARCH_OPENING_TOP_M: f64 = 3.0;

/// The arch's half-thickness along the flight axis `x`.
pub const ARCH_HALF_X_M: f64 = 0.5;

/// Half extents of the ground slab under the arch.
pub const GROUND_HALF_M: [f64; 3] = [20.0, 0.5, 15.0];

/// The ground slab's centre: its top face is `y = 0`.
pub const GROUND_POS_M: [f64; 3] = [0.0, -0.5, 0.0];

/// Half extents of the rotated water patch, deliberately bounded: water is a
/// *role on a patch*, never an infinite collision plane (F18 non-negotiable
/// behavior 2).
pub const WATER_HALF_M: [f64; 3] = [4.0, 0.1, 4.0];

/// The water patch's centre, away from the flight path.
pub const WATER_POS_M: [f64; 3] = [0.0, 0.05, -16.0];

/// The water patch's rotation about `+y`, in radians. A rotated instance
/// keeps the transform check honest: a translation-only fixture could not
/// tell a rotation bug from correct placement.
pub const WATER_ROTATION_RAD: f64 = std::f64::consts::FRAC_PI_6;

/// Half extents of the sensor volume. It is deliberately long along the
/// flight axis (`x`): at 400 m/s the probe moves 3.33 m per tick, so a
/// volume the probe could step out of between two samples would need a sweep
/// to be noticed at all, and this fixture wants the *role* measured, not the
/// sweep again.
pub const SENSOR_HALF_M: [f64; 3] = [4.0, 0.75, 0.75];

/// The sensor volume's centre: above the ground slab, off the arch's own
/// flight line, inside the `beyond` sector.
pub const SENSOR_POS_M: [f64; 3] = [10.0, 1.5, -4.0];

/// Half extents of the non-colliding banner. It has no collider, so the box
/// exists only as the record's own claim of size.
pub const NON_COLLIDING_HALF_M: [f64; 3] = [1.5, 1.0, 0.1];

/// The banner's centre, inside the `approach` sector and clear of every
/// probe path.
pub const NON_COLLIDING_POS_M: [f64; 3] = [-15.0, 6.0, -4.0];

// ------------------------------------------------------- harbor (F18-B) ---

/// The harbor world's id key.
pub const HARBOR_WORLD_KEY: &str = "synthetic.harbor_world";

/// The west sector of the harbor world: the approach to the yard, `x < -1`.
pub const HARBOR_SECTOR_APPROACH: &str = "approach";
/// The yard: the hangar and the trigger volume, `x ∈ [-1, 14]`.
pub const HARBOR_SECTOR_YARD: &str = "yard";

/// The damaged objective: one stored mesh, an arch with an opening.
pub const HARBOR_OBJECT_HANGAR: &str = "objective.hangar";
/// The trigger volume, a mesh with the same role semantics as the arch's.
pub const HARBOR_OBJECT_SENSOR: &str = "trigger.sensor";
/// The banner: mesh geometry that must never collide.
pub const HARBOR_OBJECT_BANNER: &str = "banner.non_colliding";
/// The water patch, a bounded slab.
pub const HARBOR_OBJECT_WATER: &str = "water.patch";
/// The ground slab, in both sectors so it survives either one unloading.
pub const HARBOR_OBJECT_GROUND: &str = "terrain.ground";
/// A solid object whose mesh reference this fixture deliberately does not
/// resolve, so the "geometry nobody supplied" path is exercised by the same
/// fixture the happy paths are.
pub const HARBOR_OBJECT_ABSENT: &str = "strip.absent_mesh";

/// How many boxes the hangar shell is welded from, and therefore how many
/// triangles its stored mesh holds: three boxes, six quad faces each, two
/// triangles per face.
///
/// The convex hull of the same 24 corners is a closed box with 12 triangles, so
/// this count is what tells a real trimesh from a substituted primitive.
pub const HARBOR_HANGAR_TRIANGLES: usize = 36;

/// The triangle count a convex hull of the hangar shell's corners would carry.
pub const HARBOR_HANGAR_HULL_TRIANGLES: usize = 12;

/// The hangar's own placement: at the origin, so the opening is the same
/// `z ∈ (-1, 1)`, `y ∈ (0, 3)` gap the cuboid arch uses.
pub const HARBOR_HANGAR_POS_M: [f64; 3] = [0.0, 0.0, 0.0];

/// The water slab's half sizes: a patch, not a plane.
pub const HARBOR_WATER_HALF_M: [f64; 3] = [4.0, 0.2, 4.0];
/// The water slab's centre, well clear of the hangar's flight line and inside
/// the `approach` sector's lateral extent.
pub const HARBOR_WATER_POS_M: [f64; 3] = [0.0, 0.2, -20.0];
/// A flight line beside the water patch, in the same `x` sweep, that must
/// never reach it.
pub const HARBOR_WATER_OFF_AXIS_Z_M: f64 = -40.0;

/// The trigger volume's half sizes: long along `x`, as F18-A's is.
pub const HARBOR_SENSOR_HALF_M: [f64; 3] = [4.0, 0.75, 0.75];
/// The trigger volume's centre, inside the `yard` sector.
pub const HARBOR_SENSOR_POS_M: [f64; 3] = [10.0, 1.5, -6.0];

/// The banner's half sizes, as a mesh box.
pub const HARBOR_BANNER_HALF_M: [f64; 3] = [1.5, 1.0, 0.1];
/// The banner's centre, clear of every probe path.
pub const HARBOR_BANNER_POS_M: [f64; 3] = [-15.0, 6.0, -4.0];

/// The ground slab's half sizes and centre, as in the arch world.
pub const HARBOR_GROUND_HALF_M: [f64; 3] = [20.0, 0.5, 15.0];
/// The ground slab's centre: its top face is `y = 0`.
pub const HARBOR_GROUND_POS_M: [f64; 3] = [0.0, -0.5, 0.0];

/// The absent-mesh object's placement, inside the `approach` sector.
pub const HARBOR_ABSENT_POS_M: [f64; 3] = [-20.0, 3.0, 4.0];

// -------------------------------------------------------- twin hangar world ---

/// The twin hangar world's id key.
pub const TWIN_WORLD_KEY: &str = "synthetic.twin_harbor_world";

/// The one sector of the twin hangar world. Every object is in it, so a sector
/// transaction never moves geometry and the twin question is the only thing the
/// fixture is about.
pub const TWIN_SECTOR: &str = "twin";

/// The first shell: the arch as one stored mesh, 36 triangles over three
/// material groups.
pub const TWIN_OBJECT_SHELL_A: &str = "shell.stand_a";
/// The second shell, on the **same** mesh reference as the first.
pub const TWIN_OBJECT_SHELL_B: &str = "shell.stand_b";
/// A presentation-only object naming the same mesh again: role `None`, so it
/// draws and never collides.
pub const TWIN_OBJECT_BANNER: &str = "banner.twin";
/// A trigger volume naming the same mesh again: role `Sensor`, so it reports
/// and never blocks.
pub const TWIN_OBJECT_TRIGGER: &str = "trigger.twin";
/// A solid object naming a **different** mesh, so "one asset per mesh" is not
/// "one asset for the whole world".
pub const TWIN_OBJECT_PANEL: &str = "panel.solo";
/// A cuboid object: it names a mesh the source does not hold, because a cuboid
/// never resolves one. It must therefore add no engine asset at all.
pub const TWIN_OBJECT_GROUND: &str = "terrain.twin";

/// The shell's placement: the origin, so the opening is `z ∈ (-1, 1)`,
/// `y ∈ (0, 3)` exactly as in the harbor world.
pub const TWIN_SHELL_POS_M: [f64; 3] = [0.0, 0.0, 0.0];
/// The second shell's placement: the same arch, a hundred metres downrange, so
/// the two are visibly two objects rather than one drawn twice.
pub const TWIN_SHELL_TWIN_POS_M: [f64; 3] = [100.0, 0.0, 0.0];
/// The banner's placement, clear of both shells.
pub const TWIN_BANNER_POS_M: [f64; 3] = [-15.0, 6.0, -4.0];
/// The trigger volume's placement, clear of both shells.
pub const TWIN_TRIGGER_POS_M: [f64; 3] = [10.0, 1.5, -6.0];
/// The panel's placement: its own mesh, well clear of everything else.
pub const TWIN_PANEL_POS_M: [f64; 3] = [-30.0, 2.0, 0.0];
/// The twin ground slab's half extents.
pub const TWIN_GROUND_HALF_M: [f64; 3] = [80.0, 0.5, 20.0];
/// The twin ground slab's centre, its top face at `y = 0`.
pub const TWIN_GROUND_POS_M: [f64; 3] = [50.0, -0.5, 0.0];

/// The panel's half extents as a stored mesh box: deliberately not the shell's
/// shape, so a test can tell the two assets apart by geometry alone.
pub const TWIN_PANEL_HALF_M: [f64; 3] = [1.0, 2.0, 0.25];

/// How many updates a mesh-derived collider needs before it exists: the
/// `ColliderConstructorHierarchy` is an `Update` system, and the collider is
/// attached to its body by a pass that can only see it on a later frame. The
/// same measurement is pinned by `accept_t333_*`.
pub const MESH_SETTLE_UPDATES: u64 = 4;

// ---------------------------------------------------------------- identity ---

/// The synthetic world's id key.
pub const WORLD_KEY: &str = "synthetic.arch_world";

/// Sector on the `x < -1` side of the arch.
pub const SECTOR_APPROACH: &str = "approach";
/// Sector containing the arch itself, `x ∈ [-1, 1]`.
pub const SECTOR_ARCH: &str = "arch";
/// Sector on the `x > 1` side of the arch.
pub const SECTOR_BEYOND: &str = "beyond";

/// The left arch leg.
pub const OBJECT_LEG_LEFT: &str = "arch.leg_left";
/// The right arch leg — the one the failure-case probe is aimed at.
pub const OBJECT_LEG_RIGHT: &str = "arch.leg_right";
/// The lintel above the opening.
pub const OBJECT_LINTEL: &str = "arch.lintel";
/// The ground slab; it belongs to every sector, so it never streams away
/// while any part of the world is resident.
pub const OBJECT_GROUND: &str = "terrain.ground";
/// The rotated water patch; it belongs to no sector, so it is resident.
pub const OBJECT_WATER: &str = "water.patch";
/// An object with the explicit [`WorldCollisionRole::None`]: presented, and
/// never given a collider.
pub const OBJECT_NON_COLLIDING: &str = "banner.non_colliding";
/// A trigger volume with the explicit [`WorldCollisionRole::Sensor`]: it
/// reports an overlap and never blocks motion.
pub const OBJECT_SENSOR: &str = "trigger.sensor";
/// An object whose collision role the evidence never resolved.
pub const OBJECT_UNEVIDENCED_ROLE: &str = "sign.unevidenced_role";
/// An object with a solid role but an unresolved collision shape.
pub const OBJECT_UNEVIDENCED_SHAPE: &str = "hangar.unevidenced_shape";

// --------------------------------------------------------- stored meshes ---

/// The presentation questions a bare stored mesh leaves open. They travel with
/// each upload untouched; nothing in this fixture settles them.
const MESH_UNKNOWNS: [MeshPresentationUnknown; 2] = [
    MeshPresentationUnknown::FrontFaceWinding,
    MeshPresentationUnknown::UvOrigin,
];

/// A stored mesh under construction: positions and polygon outlines, in stored
/// units and stored order.
#[derive(Debug, Default)]
struct StoredMesh {
    positions: Vec<[f32; 3]>,
    polygons: Vec<RawPolygon>,
}

impl StoredMesh {
    /// Appends one axis-aligned box: its eight corners and its six quad faces,
    /// every face carrying the raw stored `material` index.
    ///
    /// Boxes are **not** welded to each other — each keeps its own eight
    /// corners, exactly as a stored polygon soup looks like — so a derived
    /// triangle mesh has to reconcile the coincidence itself and nothing here
    /// pre-merges geometry the way a collision builder might.
    fn box_at(&mut self, min: [f32; 3], max: [f32; 3], material: u32) -> &mut Self {
        let base = self.positions.len() as u32;
        for corner in [
            [min[0], min[1], min[2]],
            [max[0], min[1], min[2]],
            [max[0], max[1], min[2]],
            [min[0], max[1], min[2]],
            [min[0], min[1], max[2]],
            [max[0], min[1], max[2]],
            [max[0], max[1], max[2]],
            [min[0], max[1], max[2]],
        ] {
            self.positions.push(corner);
        }
        // Six quad faces, in the order the F17-B upload path and the t333
        // fixture both use, so a derived shape here is comparable with theirs.
        for face in [
            [0, 1, 2, 3], // z = min
            [4, 5, 6, 7], // z = max
            [0, 1, 5, 4], // y = min
            [1, 2, 6, 5], // x = max
            [2, 3, 7, 6], // y = max
            [3, 0, 4, 7], // x = min
        ] {
            self.polygons.push(RawPolygon {
                kind: PrimitiveKind::Polygon,
                raw_flags: 0,
                material,
                corners: face
                    .iter()
                    .map(|corner| RawCorner {
                        position: base + corner,
                        normal: None,
                        uv: None,
                        color: None,
                    })
                    .collect(),
            });
        }
        self
    }

    /// The stored mesh.
    fn build(self) -> RawMesh {
        RawMesh {
            positions: self.positions,
            normals: Vec::new(),
            polygons: self.polygons,
        }
    }
}

/// The stored mesh of a single box, in metres, on stored material `0`.
fn box_mesh(half: [f32; 3]) -> RawMesh {
    let mut mesh = StoredMesh::default();
    mesh.box_at([-half[0], -half[1], -half[2]], half, 0);
    mesh.build()
}

/// The stored mesh of the hangar shell: the arch as **one** mesh, and its three
/// boxes on three **different** stored material indices.
///
/// The two legs and the lintel are separate boxes in one polygon soup, so the
/// stored geometry has a rectangular tunnel through it along `x` and a convex
/// hull of the same corners does not. The opening is therefore a property of
/// the mesh, not of a subtraction the collision builder performs — which is the
/// only way "never close a traversable opening through convex-hull
/// simplification" (F18 non-negotiable behavior 1) is a testable claim about
/// this stage.
///
/// The three material indices are deliberate. F17-B groups triangles by their
/// stored raw material index, so a source built from only group `0` would
/// present and collide the left leg alone and silently drop the right leg and
/// the lintel. Every material index being `0` would hide exactly the bug this
/// fixture has to be able to see.
fn hangar_shell_mesh() -> RawMesh {
    let mut mesh = StoredMesh::default();
    // A leg: 1 m thick along `x`, 3 m tall, 1 m deep, its inner face at
    // `z = ∓1` so the opening is 2 m wide. The left leg is material 0.
    mesh.box_at([-0.5, 0.0, -2.0], [0.5, 3.0, -1.0], 0);
    // The right leg is material 1.
    mesh.box_at([-0.5, 0.0, 1.0], [0.5, 3.0, 2.0], 1);
    // The lintel closes the arch above the opening, its underside at `y = 3`.
    mesh.box_at([-0.5, 3.0, -2.0], [0.5, 4.0, 2.0], 2);
    mesh.build()
}

/// The F17-B render mesh of one stored fixture mesh: the split view the world
/// mesh source consumes, exactly as a catalog's `MeshUpload::render` hands over.
fn render_mesh(stored: &RawMesh) -> RenderMesh {
    RenderMesh::build(stored).expect("the fixture's stored mesh has a decodable outline")
}

/// The mesh reference an object record names, from its own key.
#[must_use]
pub fn mesh_reference(key: &str) -> Resolved<ContentId> {
    mesh(key)
}

// --------------------------------------------------------------- provenance ---

fn claim(key: &str) -> ClaimId {
    ClaimId::new(&format!("f18a.{key}")).expect("the fixture claim ids are valid")
}

/// Designed provenance for a fixture claim.
#[must_use]
pub fn fixture_provenance(key: &str) -> Provenance {
    Provenance::designed(claim(key))
}

fn known<T>(value: T, key: &str) -> Resolved<T> {
    Resolved::Known(Known::new(value, fixture_provenance(key)))
}

fn unknown<T>(key: &str, reason: &str) -> Resolved<T> {
    Resolved::unknown(claim(key), reason).expect("the fixture reasons are non-empty")
}

/// The identity transform at `translation`, in meters.
fn translated(translation: [f64; 3]) -> CanonicalTransform {
    CanonicalTransform::try_new(
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        translation,
    )
    .expect("the fixture translations are finite")
}

/// A rotation about `+y` by `angle` radians, then a translation.
fn rotated_y(translation: [f64; 3], angle: f64) -> CanonicalTransform {
    let (sin, cos) = angle.sin_cos();
    CanonicalTransform::try_new(
        [[cos, 0.0, sin], [0.0, 1.0, 0.0], [-sin, 0.0, cos]],
        translation,
    )
    .expect("the fixture rotation is finite")
}

fn mesh(key: &str) -> Resolved<ContentId> {
    known(
        ContentId::from_source(ContentKind::Mesh, &format!("synthetic.{key}"))
            .expect("the fixture mesh ids are valid"),
        &format!("{key}.mesh"),
    )
}

fn sector(key: &str) -> SectorId {
    SectorId::new(key).expect("the fixture sector keys are valid")
}

fn object(key: &str) -> WorldObjectId {
    WorldObjectId::new(key).expect("the fixture object keys are valid")
}

fn cuboid(half: [f64; 3]) -> Resolved<WorldCollisionShape> {
    known(
        WorldCollisionShape::cuboid(half).expect("the fixture half extents are positive"),
        "fixture.cuboid",
    )
}

/// A solid box instance in `sectors`.
fn solid_box(
    key: &str,
    transform: CanonicalTransform,
    half: [f64; 3],
    surface: Resolved<SurfaceRole>,
    sectors: Vec<SectorId>,
) -> WorldObjectInstance {
    WorldObjectInstance::try_new(
        object(key),
        mesh(key),
        transform,
        known(WorldCollisionRole::Solid, "fixture.collision-role.solid"),
        cuboid(half),
        surface,
        sectors,
        fixture_provenance(&format!("{key}.record")),
    )
    .expect("the fixture sector lists contain no duplicates")
}

// ------------------------------------------------------------------ fixture ---

/// Builds the synthetic arch world.
///
/// Nine object instances: three arch parts that define the traversable
/// opening, a ground slab spanning two sectors, a resident water patch with
/// no sector, an explicit non-colliding banner, an explicit sensor volume,
/// and two objects that carry an **explicit unknown** so the unresolved
/// paths are exercised by the same fixture the happy paths are. All three
/// [`WorldCollisionRole`] values therefore occur in one record:
/// `None` (the banner), `Solid` (the arch parts, ground and water) and
/// `Sensor` (the trigger volume).
///
/// # Errors
///
/// Never for this fixture — its ids, transforms and shapes are constants
/// validated on the path here — but kept as `Result` so a later fixture that
/// takes parameters has the same shape as the importer's own constructor.
pub fn arch_world() -> Result<WorldDefinition, WorldError> {
    let approach = sector(SECTOR_APPROACH);
    let arch = sector(SECTOR_ARCH);
    let beyond = sector(SECTOR_BEYOND);

    let sectors = vec![
        Sector::new(
            approach.clone(),
            Aabb::try_new([-40.0, -2.0, -6.0], [-1.0, 10.0, 6.0])
                .expect("the approach bounds are well formed"),
        ),
        Sector::new(
            arch.clone(),
            Aabb::try_new([-1.0, -2.0, -6.0], [1.0, 10.0, 6.0])
                .expect("the arch bounds are well formed"),
        ),
        Sector::new(
            beyond.clone(),
            Aabb::try_new([1.0, -2.0, -6.0], [40.0, 10.0, 6.0])
                .expect("the beyond bounds are well formed"),
        ),
    ];

    let ground_surface = known(SurfaceRole::Ground, "fixture.surface.ground");
    let water_surface = known(SurfaceRole::Water, "fixture.surface.water");
    let no_surface = unknown::<SurfaceRole>(
        "surface.unmeasured",
        "no evidence has classified this instance's gameplay surface",
    );

    let objects = vec![
        solid_box(
            OBJECT_LEG_LEFT,
            translated([0.0, ARCH_LEG_HALF_M[1], -ARCH_LEG_Z_M]),
            ARCH_LEG_HALF_M,
            ground_surface.clone(),
            vec![arch.clone()],
        ),
        solid_box(
            OBJECT_LEG_RIGHT,
            translated([0.0, ARCH_LEG_HALF_M[1], ARCH_LEG_Z_M]),
            ARCH_LEG_HALF_M,
            ground_surface.clone(),
            vec![arch.clone()],
        ),
        solid_box(
            OBJECT_LINTEL,
            translated(ARCH_LINTEL_POS_M),
            ARCH_LINTEL_HALF_M,
            ground_surface.clone(),
            vec![arch],
        ),
        solid_box(
            OBJECT_GROUND,
            translated(GROUND_POS_M),
            GROUND_HALF_M,
            ground_surface.clone(),
            vec![approach.clone(), beyond.clone()],
        ),
        solid_box(
            OBJECT_WATER,
            rotated_y(WATER_POS_M, WATER_ROTATION_RAD),
            WATER_HALF_M,
            water_surface,
            vec![],
        ),
        // The explicit `None` role: presented, and never given a collider.
        // (`solid_box` would be wrong here: it hardcodes `Solid`.)
        WorldObjectInstance::try_new(
            object(OBJECT_NON_COLLIDING),
            mesh(OBJECT_NON_COLLIDING),
            translated(NON_COLLIDING_POS_M),
            known(WorldCollisionRole::None, "fixture.collision-role.none"),
            cuboid(NON_COLLIDING_HALF_M),
            ground_surface.clone(),
            vec![approach.clone()],
            fixture_provenance("banner.non_colliding.record"),
        )
        .expect("the fixture sector lists contain no duplicates"),
        // The explicit `Sensor` role: reports an overlap, never blocks.
        WorldObjectInstance::try_new(
            object(OBJECT_SENSOR),
            mesh(OBJECT_SENSOR),
            translated(SENSOR_POS_M),
            known(WorldCollisionRole::Sensor, "fixture.collision-role.sensor"),
            cuboid(SENSOR_HALF_M),
            ground_surface.clone(),
            vec![beyond.clone()],
            fixture_provenance("trigger.sensor.record"),
        )
        .expect("the fixture sector lists contain no duplicates"),
        // The collision role itself is unevidenced: the record carries the
        // unknown, and the spawn reports it instead of choosing a side.
        WorldObjectInstance::try_new(
            object(OBJECT_UNEVIDENCED_ROLE),
            mesh(OBJECT_UNEVIDENCED_ROLE),
            translated([-10.0, 2.0, 4.0]),
            unknown::<WorldCollisionRole>(
                "collision-role.unmeasured",
                "no evidence has classified whether this instance blocks motion",
            ),
            cuboid([0.5, 0.5, 0.1]),
            no_surface,
            vec![approach],
            fixture_provenance("sign.unevidenced_role.record"),
        )
        .expect("the fixture sector lists contain no duplicates"),
        // A solid role whose *shape* is unevidenced: the spawn must report
        // the missing geometry rather than substitute a box for it.
        WorldObjectInstance::try_new(
            object(OBJECT_UNEVIDENCED_SHAPE),
            mesh(OBJECT_UNEVIDENCED_SHAPE),
            translated([20.0, 3.0, -5.0]),
            known(WorldCollisionRole::Solid, "fixture.collision-role.solid"),
            unknown::<WorldCollisionShape>(
                "collision-shape.unmeasured",
                "no collider geometry has been measured for this instance",
            ),
            ground_surface.clone(),
            vec![beyond],
            fixture_provenance("hangar.unevidenced_shape.record"),
        )
        .expect("the fixture sector lists contain no duplicates"),
    ];

    let boundary = known(
        WorldBoundary::try_new(Some(-50.0), Some(500.0), None)
            .expect("the fixture boundary is well formed"),
        "fixture.boundary",
    );

    WorldDefinition::try_new(
        WorldId::from_key(WORLD_KEY).expect("the fixture world key is valid"),
        Origin::SyntheticFixture,
        boundary,
        sectors,
        objects,
        fixture_provenance("arch_world.record"),
    )
}

/// Builds the harbor world: the F18-B fixture for the import path and for
/// mesh-derived static collision.
///
/// Six object instances, all `Origin::SyntheticFixture`:
///
/// | object | role | shape | surface | sectors |
/// | --- | --- | --- | --- | --- |
/// | `objective.hangar` | `Solid` | `FromMesh` (36 triangles) | `Ground` | `yard` |
/// | `trigger.sensor` | `Sensor` | `FromMesh` | `Ground` | `yard` |
/// | `banner.non_colliding` | `None` | `FromMesh` | `Ground` | `approach` |
/// | `water.patch` | `Solid` | `FromMesh` | `Water` | *resident* |
/// | `terrain.ground` | `Solid` | `Cuboid` | `Ground` | `approach`, `yard` |
/// | `strip.absent_mesh` | `Solid` | `FromMesh` | `Ground` | `approach` |
///
/// The hangar's opening is in its *mesh*, so a collision built from anything
/// other than that mesh closes it. The water patch is a bounded slab, so a
/// collision plane invented over low flight would show. The ground belongs to
/// both sectors, so it must survive either one unloading. The last object names
/// a mesh this fixture does not resolve, so "geometry nobody supplied" is
/// exercised by the same fixture as the happy paths.
///
/// # Errors
///
/// Never for this fixture — its ids, transforms and shapes are constants
/// validated on the path here — but kept as `Result` like
/// [`arch_world`], so both fixtures have the same shape as the importer's own
/// constructor.
pub fn harbor_world() -> Result<WorldDefinition, WorldError> {
    let approach = sector(HARBOR_SECTOR_APPROACH);
    let yard = sector(HARBOR_SECTOR_YARD);

    let sectors = vec![
        Sector::new(
            approach.clone(),
            Aabb::try_new([-40.0, -2.0, -46.0], [-1.0, 12.0, 30.0])
                .expect("the approach bounds are well formed"),
        ),
        Sector::new(
            yard.clone(),
            Aabb::try_new([-1.0, -2.0, -30.0], [14.0, 12.0, 30.0])
                .expect("the yard bounds are well formed"),
        ),
    ];

    let ground_surface = known(SurfaceRole::Ground, "harbor.surface.ground");
    let water_surface = known(SurfaceRole::Water, "harbor.surface.water");
    let solid = known(WorldCollisionRole::Solid, "harbor.collision-role.solid");
    let sensor = known(WorldCollisionRole::Sensor, "harbor.collision-role.sensor");
    let none_role = known(WorldCollisionRole::None, "harbor.collision-role.none");
    let from_mesh = known(
        WorldCollisionShape::FromMesh,
        "harbor.collision-shape.from-mesh",
    );

    let objects = vec![
        // The damaged objective: the arch, as one mesh.
        WorldObjectInstance::try_new(
            object(HARBOR_OBJECT_HANGAR),
            mesh(HARBOR_OBJECT_HANGAR),
            translated(HARBOR_HANGAR_POS_M),
            solid.clone(),
            from_mesh.clone(),
            ground_surface.clone(),
            vec![yard.clone()],
            fixture_provenance("harbor.hangar.record"),
        )
        .expect("the fixture sector lists contain no duplicates"),
        // A trigger volume whose collision is derived from a mesh as well, so
        // the sensor role is exercised on the mesh path.
        WorldObjectInstance::try_new(
            object(HARBOR_OBJECT_SENSOR),
            mesh(HARBOR_OBJECT_SENSOR),
            translated(HARBOR_SENSOR_POS_M),
            sensor,
            from_mesh.clone(),
            ground_surface.clone(),
            vec![yard.clone()],
            fixture_provenance("harbor.sensor.record"),
        )
        .expect("the fixture sector lists contain no duplicates"),
        // Mesh geometry that must never collide.
        WorldObjectInstance::try_new(
            object(HARBOR_OBJECT_BANNER),
            mesh(HARBOR_OBJECT_BANNER),
            translated(HARBOR_BANNER_POS_M),
            none_role,
            from_mesh.clone(),
            ground_surface.clone(),
            vec![approach.clone()],
            fixture_provenance("harbor.banner.record"),
        )
        .expect("the fixture sector lists contain no duplicates"),
        // Water: a bounded patch with its own surface rule, and no sector, so
        // it is resident whatever the streaming policy does.
        WorldObjectInstance::try_new(
            object(HARBOR_OBJECT_WATER),
            mesh(HARBOR_OBJECT_WATER),
            translated(HARBOR_WATER_POS_M),
            solid.clone(),
            from_mesh.clone(),
            water_surface,
            vec![],
            fixture_provenance("harbor.water.record"),
        )
        .expect("the fixture sector lists contain no duplicates"),
        // The ground slab belongs to both sectors, so it stays while either is
        // loaded: a cuboid, so both collision paths are in one record.
        WorldObjectInstance::try_new(
            object(HARBOR_OBJECT_GROUND),
            mesh(HARBOR_OBJECT_GROUND),
            translated(HARBOR_GROUND_POS_M),
            solid,
            cuboid(HARBOR_GROUND_HALF_M),
            ground_surface.clone(),
            vec![approach.clone(), yard],
            fixture_provenance("harbor.ground.record"),
        )
        .expect("the fixture sector lists contain no duplicates"),
        // A solid object whose mesh this source does not hold: the load must
        // report it, not invent geometry for it.
        WorldObjectInstance::try_new(
            object(HARBOR_OBJECT_ABSENT),
            mesh(HARBOR_OBJECT_ABSENT),
            translated(HARBOR_ABSENT_POS_M),
            known(WorldCollisionRole::Solid, "harbor.collision-role.solid"),
            from_mesh,
            ground_surface,
            vec![approach],
            fixture_provenance("harbor.absent_mesh.record"),
        )
        .expect("the fixture sector lists contain no duplicates"),
    ];

    let boundary = known(
        WorldBoundary::try_new(Some(-50.0), Some(500.0), None)
            .expect("the fixture boundary is well formed"),
        "harbor.boundary",
    );

    WorldDefinition::try_new(
        WorldId::from_key(HARBOR_WORLD_KEY).expect("the harbor world key is valid"),
        Origin::SyntheticFixture,
        boundary,
        sectors,
        objects,
        fixture_provenance("harbor_world.record"),
    )
}

/// The mesh source the harbor world is loaded with: each stored mesh the
/// fixture authored, keyed by the reference its record names, built through the
/// production [`WorldMeshes::insert_render_mesh`] path — which uploads **every**
/// material group of the mesh, not just one.
///
/// [`HARBOR_OBJECT_ABSENT`] is deliberately **not** in it, so the report has one
/// real gap to name. Every other object is served by exactly one entry, which
/// is what makes "one asset, one set of triangles" checkable. The hangar shell
/// carries three material groups, so this is also where "every group reaches
/// the collider" is exercised.
#[must_use]
pub fn harbor_meshes() -> WorldMeshes {
    let mut meshes = WorldMeshes::new();
    for (key, stored) in [
        (HARBOR_OBJECT_HANGAR, hangar_shell_mesh()),
        (
            HARBOR_OBJECT_SENSOR,
            box_mesh(half_f32(HARBOR_SENSOR_HALF_M)),
        ),
        (
            HARBOR_OBJECT_BANNER,
            box_mesh(half_f32(HARBOR_BANNER_HALF_M)),
        ),
        (HARBOR_OBJECT_WATER, box_mesh(half_f32(HARBOR_WATER_HALF_M))),
    ] {
        let reference = mesh(key)
            .known()
            .expect("the fixture mesh references are known");
        meshes
            .insert_render_mesh(reference, &render_mesh(&stored), &MESH_UNKNOWNS)
            .expect("the fixture's stored meshes upload through every material group");
    }
    meshes
}

/// Builds the twin hangar world: two records naming **one** stored mesh, plus
/// the other three roles that mesh path has to cover.
///
/// | object | role | shape | mesh |
/// | --- | --- | --- | --- |
/// | `shell.stand_a` | `Solid` | `FromMesh` | the arch shell |
/// | `shell.stand_b` | `Solid` | `FromMesh` | **the same** shell |
/// | `banner.twin` | `None` | `FromMesh` | **the same** shell |
/// | `trigger.twin` | `Sensor` | `FromMesh` | **the same** shell |
/// | `panel.solo` | `Solid` | `FromMesh` | a panel of its own |
/// | `terrain.twin` | `Solid` | `Cuboid` | *named but unresolved* |
///
/// Four records name one stored mesh on purpose: they are the four consumers the
/// spawn path has for a mesh record — two solids, a presentation-only object and
/// a trigger volume — so a shared-asset claim that only held for the solid path
/// would be caught here. The fifth names a **different** mesh, because "one
/// asset per mesh" is a claim about how the source is keyed and would also be
/// satisfied by one asset for the whole world; the sixth is a cuboid, whose mesh
/// reference this source does not resolve, so the number of engine assets must
/// not move when it spawns.
///
/// The two shells are a hundred metres apart, so a reader can see they are two
/// objects and not one object drawn twice.
///
/// # Errors
///
/// Never for this fixture, like [`harbor_world`]: its ids, transforms and shapes
/// are constants validated on the path here.
pub fn twin_harbor_world() -> Result<WorldDefinition, WorldError> {
    let yard = sector(TWIN_SECTOR);

    let sectors = vec![Sector::new(
        yard.clone(),
        Aabb::try_new([-40.0, -2.0, -30.0], [140.0, 12.0, 30.0])
            .expect("the twin world's bounds are well formed"),
    )];

    let ground_surface = known(SurfaceRole::Ground, "twin.surface.ground");
    let solid = known(WorldCollisionRole::Solid, "twin.collision-role.solid");
    let sensor = known(WorldCollisionRole::Sensor, "twin.collision-role.sensor");
    let none_role = known(WorldCollisionRole::None, "twin.collision-role.none");
    let from_mesh = known(
        WorldCollisionShape::FromMesh,
        "twin.collision-shape.from-mesh",
    );
    // One stored mesh, named by four records. The reference is *this* fixture's
    // own helper, so the records and the mesh source below agree by
    // construction rather than by two literals that happen to match.
    let shell = mesh(TWIN_OBJECT_SHELL_A);

    let mesh_record = |key: &str, mesh_ref: Resolved<ContentId>, pos: [f64; 3]| {
        WorldObjectInstance::try_new(
            object(key),
            mesh_ref,
            translated(pos),
            solid.clone(),
            from_mesh.clone(),
            ground_surface.clone(),
            vec![yard.clone()],
            fixture_provenance(&format!("twin.{key}.record")),
        )
        .expect("the fixture sector lists contain no duplicates")
    };

    let objects = vec![
        mesh_record(TWIN_OBJECT_SHELL_A, shell.clone(), TWIN_SHELL_POS_M),
        mesh_record(TWIN_OBJECT_SHELL_B, shell.clone(), TWIN_SHELL_TWIN_POS_M),
        mesh_record(TWIN_OBJECT_PANEL, mesh(TWIN_OBJECT_PANEL), TWIN_PANEL_POS_M),
        // The presentation-only record, on the same mesh: role `None`, so it must
        // present the shared asset and never get a collider.
        WorldObjectInstance::try_new(
            object(TWIN_OBJECT_BANNER),
            shell.clone(),
            translated(TWIN_BANNER_POS_M),
            none_role,
            from_mesh.clone(),
            ground_surface.clone(),
            vec![yard.clone()],
            fixture_provenance("twin.banner.record"),
        )
        .expect("the fixture sector lists contain no duplicates"),
        // The trigger volume, on the same mesh: the body-less layout, which has
        // to reach the same asset as the solid path.
        WorldObjectInstance::try_new(
            object(TWIN_OBJECT_TRIGGER),
            shell,
            translated(TWIN_TRIGGER_POS_M),
            sensor,
            from_mesh,
            ground_surface.clone(),
            vec![yard.clone()],
            fixture_provenance("twin.trigger.record"),
        )
        .expect("the fixture sector lists contain no duplicates"),
        // A cuboid, whose named mesh this source does not hold: its mesh
        // reference stays unresolved on purpose, so "a record that names a mesh
        // without needing one adds no engine asset" is exercised here too.
        WorldObjectInstance::try_new(
            object(TWIN_OBJECT_GROUND),
            mesh(TWIN_OBJECT_GROUND),
            translated(TWIN_GROUND_POS_M),
            solid,
            cuboid(TWIN_GROUND_HALF_M),
            ground_surface,
            vec![yard],
            fixture_provenance("twin.ground.record"),
        )
        .expect("the fixture sector lists contain no duplicates"),
    ];

    let boundary = known(
        WorldBoundary::try_new(Some(-50.0), Some(500.0), None)
            .expect("the twin world boundary is well formed"),
        "twin_harbor_world.boundary",
    );

    WorldDefinition::try_new(
        WorldId::from_key(TWIN_WORLD_KEY).expect("the twin world key is valid"),
        Origin::SyntheticFixture,
        boundary,
        sectors,
        objects,
        fixture_provenance("twin_harbor_world.record"),
    )
}

/// The mesh source the twin hangar world is loaded with: the arch shell under the
/// reference **four** of its records name, and the panel under its own.
///
/// The shell is the same stored mesh [`harbor_meshes`] builds for the hangar —
/// three boxes on three material indices, 36 triangles — so a triangle-count
/// assertion on the shared asset means the *merged* mesh is what is shared, not
/// one material group of it.
#[must_use]
pub fn twin_harbor_meshes() -> WorldMeshes {
    let mut meshes = WorldMeshes::new();
    for (key, stored) in [
        (TWIN_OBJECT_SHELL_A, hangar_shell_mesh()),
        (TWIN_OBJECT_PANEL, box_mesh(half_f32(TWIN_PANEL_HALF_M))),
    ] {
        let reference = mesh(key)
            .known()
            .expect("the fixture mesh references are known");
        meshes
            .insert_render_mesh(reference, &render_mesh(&stored), &MESH_UNKNOWNS)
            .expect("the fixture's stored meshes upload through every material group");
    }
    meshes
}

/// One f64 half-extent triple as the stored mesh's f32 unit triple.
fn half_f32(half: [f64; 3]) -> [f32; 3] {
    [half[0] as f32, half[1] as f32, half[2] as f32]
}

/// One mission's load record for `definition`.
///
/// `variant` is `None` for "the evidence never named a variant", which becomes
/// an explicit unknown rather than a default. `population` and `damaged` are the
/// authored choices the load states for itself (F18 non-negotiable behavior 5).
///
/// # Errors
///
/// [`WorldError`] from the record's own validation, or
/// [`WorldError::DamagedObjectNotActivated`] when `damaged` names an object
/// `population` does not activate.
pub fn world_instance(
    definition: &WorldDefinition,
    variant: Option<&str>,
    population: &[&str],
    damaged: &[&str],
) -> Result<WorldInstance, WorldError> {
    let variant = match variant {
        Some(key) => known(
            WorldId::from_key(key).expect("the fixture variant key is valid"),
            "fixture.variant",
        ),
        None => unknown::<WorldId>(
            "variant.unmeasured",
            "no evidence has named a variant for this load",
        ),
    };
    WorldInstance::try_new(
        definition.id().clone(),
        variant,
        WorldPopulation::Only(object_set(population)),
        object_set(damaged),
        fixture_provenance("load.record"),
    )
}

// ------------------------------------------------------ depot (F18-C) ---

/// The depot world's id key.
pub const DEPOT_WORLD_KEY: &str = "synthetic.depot_world";

/// The approach to the depot, west of the hangar face.
pub const DEPOT_SECTOR_APPROACH: &str = "approach";
/// The hangar face itself: the shell and the door panel.
pub const DEPOT_SECTOR_YARD: &str = "yard";
/// A sector far east of everything else, holding nothing gameplay needs: the
/// one a visibility pass is allowed to stream away.
pub const DEPOT_SECTOR_ANNEX: &str = "annex";

/// The hangar shell: the same stored arch the harbor world uses, so the door is
/// a hole in *this* world too and not only in a drawing.
pub const DEPOT_OBJECT_HANGAR: &str = "depot.hangar";
/// The door panel that shuts the shell's opening.
pub const DEPOT_OBJECT_DOOR: &str = "depot.door";
/// The trigger volume in front of the panel: the overlay's own trigger.
pub const DEPOT_OBJECT_TRIGGER: &str = "trigger.depot";
/// The depot's ground slab, in the approach and the yard.
pub const DEPOT_OBJECT_GROUND: &str = "terrain.depot";
/// A crate in the annex, high above the depot's flight line.
pub const DEPOT_OBJECT_CRATE: &str = "depot.crate";

/// Half extents of the door panel, which is *exactly* the shell's opening: 1 m
/// thick along the flight axis, 3 m tall and 2 m wide. A panel with these half
/// extents fills the tunnel, so the passage is shut until the panel moves.
pub const DEPOT_DOOR_HALF_M: [f64; 3] = [0.5, 1.5, 1.0];
/// The panel's centre, which is the opening's own centre.
pub const DEPOT_DOOR_POS_M: [f64; 3] = [0.0, 1.5, 0.0];
/// The displacement the door overlay applies: far enough in `+z` to clear the
/// opening's width, so the tunnel the shell describes is flyable afterwards.
pub const DEPOT_DOOR_OPEN_OFFSET_M: [f64; 3] = [0.0, 0.0, 2.0];
/// Half extents of the trigger volume: as wide and as tall as the opening, so a
/// body on the tunnel's centreline is inside it, and 1 m thick along the flight
/// axis so the crossing is unambiguous.
pub const DEPOT_TRIGGER_HALF_M: [f64; 3] = [0.5, 1.5, 1.5];
/// The trigger's centre, four metres in front of the panel's face.
pub const DEPOT_TRIGGER_POS_M: [f64; 3] = [-4.0, 1.5, 0.0];
/// The depot ground slab's half sizes and centre, as the harbor's.
pub const DEPOT_GROUND_HALF_M: [f64; 3] = [20.0, 0.5, 15.0];
/// The depot ground slab's centre: its top face is `y = 0`.
pub const DEPOT_GROUND_POS_M: [f64; 3] = [0.0, -0.5, 0.0];
/// The annex crate's half sizes, and the height that keeps it clear of the
/// depot's flight line — a streaming test that also hit the crate would not be
/// about streaming.
pub const DEPOT_CRATE_HALF_M: [f64; 3] = [1.0, 1.0, 1.0];
/// The crate's centre, high above the ground in the annex sector.
pub const DEPOT_CRATE_POS_M: [f64; 3] = [30.0, 6.0, 0.0];

/// Builds the depot world: the F18-C fixture for mission overlays and for
/// visibility-driven streaming.
///
/// | object | role | shape | surface | sectors |
/// | --- | --- | --- | --- | --- |
/// | `depot.hangar` | `Solid` | `FromMesh` (36 triangles) | `Ground` | `yard` |
/// | `depot.door` | `Solid` | `Cuboid` | `Ground` | `yard` |
/// | `trigger.depot` | `Sensor` | `Cuboid` | `Ground` | `approach` |
/// | `terrain.depot` | `Solid` | `Cuboid` | `Ground` | `approach`, `yard` |
/// | `depot.crate` | `Solid` | `Cuboid` | `Ground` | `annex` |
///
/// The shell's opening is shut by a panel that *is* the opening's own box, so
/// "the door opened" is a claim about geometry: with the panel in place the
/// tunnel is not flyable, and after the overlay's displacement it is. The
/// trigger is a sensor volume in front of the panel, which is the shape an
/// overlay's trigger must have ([`MissionOverlay`] refuses any other role). The
/// annex is a third sector with nothing gameplay needs in it, so a visibility
/// pass has both a sector it may stream away and one it may not.
///
/// The door is a **cuboid** on purpose: a cuboid object is a presentation
/// entity and a collider entity, so an overlay that moved the drawn geometry
/// and left the collision behind is visible here rather than being impossible
/// by construction the way a one-entity mesh object would make it.
///
/// # Errors
///
/// Never for this fixture, like [`arch_world`] and [`harbor_world`]: its ids,
/// transforms and shapes are constants validated on the path here.
pub fn depot_world() -> Result<WorldDefinition, WorldError> {
    let approach = sector(DEPOT_SECTOR_APPROACH);
    let yard = sector(DEPOT_SECTOR_YARD);
    let annex = sector(DEPOT_SECTOR_ANNEX);

    let sectors = vec![
        Sector::new(
            approach.clone(),
            Aabb::try_new([-40.0, -2.0, -6.0], [-1.0, 12.0, 6.0])
                .expect("the depot approach bounds are well formed"),
        ),
        Sector::new(
            yard.clone(),
            Aabb::try_new([-1.0, -2.0, -6.0], [14.0, 12.0, 6.0])
                .expect("the depot yard bounds are well formed"),
        ),
        Sector::new(
            annex.clone(),
            Aabb::try_new([20.0, -2.0, -6.0], [40.0, 12.0, 6.0])
                .expect("the depot annex bounds are well formed"),
        ),
    ];

    let ground_surface = known(SurfaceRole::Ground, "depot.surface.ground");
    let solid = known(WorldCollisionRole::Solid, "depot.collision-role.solid");
    let sensor = known(WorldCollisionRole::Sensor, "depot.collision-role.sensor");
    let from_mesh = known(
        WorldCollisionShape::FromMesh,
        "depot.collision-shape.from-mesh",
    );

    let objects = vec![
        // The shell, as one stored mesh with a tunnel through it.
        WorldObjectInstance::try_new(
            object(DEPOT_OBJECT_HANGAR),
            mesh(DEPOT_OBJECT_HANGAR),
            translated([0.0, 0.0, 0.0]),
            solid.clone(),
            from_mesh,
            ground_surface.clone(),
            vec![yard.clone()],
            fixture_provenance("depot.hangar.record"),
        )
        .expect("the fixture sector lists contain no duplicates"),
        // The panel that shuts that tunnel, a cuboid so its visual and its
        // collider are two entities.
        solid_box(
            DEPOT_OBJECT_DOOR,
            translated(DEPOT_DOOR_POS_M),
            DEPOT_DOOR_HALF_M,
            ground_surface.clone(),
            vec![yard.clone()],
        ),
        // The overlay's trigger: a sensor volume on the panel's own flight line.
        WorldObjectInstance::try_new(
            object(DEPOT_OBJECT_TRIGGER),
            mesh(DEPOT_OBJECT_TRIGGER),
            translated(DEPOT_TRIGGER_POS_M),
            sensor,
            cuboid(DEPOT_TRIGGER_HALF_M),
            ground_surface.clone(),
            vec![approach.clone()],
            fixture_provenance("depot.trigger.record"),
        )
        .expect("the fixture sector lists contain no duplicates"),
        // The ground spans the approach and the yard, so it survives either one
        // streaming away — the F18-B membership rule, in a third world.
        solid_box(
            DEPOT_OBJECT_GROUND,
            translated(DEPOT_GROUND_POS_M),
            DEPOT_GROUND_HALF_M,
            ground_surface.clone(),
            vec![approach.clone(), yard],
        ),
        // The annex crate, in a sector no load record requires.
        solid_box(
            DEPOT_OBJECT_CRATE,
            translated(DEPOT_CRATE_POS_M),
            DEPOT_CRATE_HALF_M,
            ground_surface,
            vec![annex],
        ),
    ];

    let boundary = known(
        WorldBoundary::try_new(Some(-50.0), Some(500.0), None)
            .expect("the depot boundary is well formed"),
        "depot.boundary",
    );

    WorldDefinition::try_new(
        WorldId::from_key(DEPOT_WORLD_KEY).expect("the depot world key is valid"),
        Origin::SyntheticFixture,
        boundary,
        sectors,
        objects,
        fixture_provenance("depot_world.record"),
    )
}

/// The mesh source the depot world is loaded with: the hangar shell's upload.
///
/// The panel, the trigger, the ground and the crate are cuboids and need no
/// upload. That is deliberate: the panel is the object whose render and
/// collision halves are separate entities, and it is a cuboid, so "both
/// consumers updated" cannot be satisfied here by a one-entity object moving by
/// construction.
#[must_use]
pub fn depot_meshes() -> WorldMeshes {
    let mut meshes = WorldMeshes::new();
    let reference = mesh(DEPOT_OBJECT_HANGAR)
        .known()
        .expect("the fixture mesh references are known");
    let stored = hangar_shell_mesh();
    meshes
        .insert_render_mesh(reference, &render_mesh(&stored), &MESH_UNKNOWNS)
        .expect("the fixture's stored meshes upload through every material group");
    meshes
}

/// The depot's door overlay: crossing [`DEPOT_OBJECT_TRIGGER`] displaces
/// [`DEPOT_OBJECT_DOOR`] by [`DEPOT_DOOR_OPEN_OFFSET_M`].
///
/// A *designed* record, not an imported one: which volumes the 2000 PC original
/// used as triggers, and what opening a door does to its geometry, is
/// unmeasured. What is measured here is what this one does.
#[must_use]
pub fn door_overlay() -> MissionOverlay {
    MissionOverlay::try_new(
        object(DEPOT_OBJECT_TRIGGER),
        OverlayEffect::Displace {
            target: object(DEPOT_OBJECT_DOOR),
            offset_m: DEPOT_DOOR_OPEN_OFFSET_M,
        },
        fixture_provenance("depot.door_overlay"),
    )
    .expect("the depot door overlay's offset is finite")
}

/// A mission load record for the depot world.
///
/// `open_door` is what makes the same authored world two different missions:
/// with the overlay the tunnel becomes flyable, without it the panel *is* the
/// opening and nothing gets through (F18 non-negotiable behavior 5). `required`
/// is the set of objects gameplay cannot lose to streaming (F18
/// non-negotiable behavior 3), which the visibility policy holds in place.
///
/// # Errors
///
/// [`WorldError`] from the load record's own validation, including the overlay
/// refusals [`WorldInstance::validate_against`] makes for a trigger that is not
/// a sensor or an object the population never activates.
pub fn depot_mission(
    definition: &WorldDefinition,
    open_door: bool,
    required: &[&str],
) -> Result<WorldInstance, WorldError> {
    let overlays = if open_door {
        vec![door_overlay()]
    } else {
        Vec::new()
    };
    world_instance(
        definition,
        Some("synthetic.depot_world.mission_01"),
        &depot_population(),
        &[],
    )?
    .with_mission_layer(overlays, object_set(required))
}

/// Every object the depot world declares, in definition order: the population a
/// depot mission load record activates.
#[must_use]
pub fn depot_population() -> Vec<&'static str> {
    vec![
        DEPOT_OBJECT_HANGAR,
        DEPOT_OBJECT_DOOR,
        DEPOT_OBJECT_TRIGGER,
        DEPOT_OBJECT_GROUND,
        DEPOT_OBJECT_CRATE,
    ]
}

// ------------------------------------------------------------------- probe ---

/// Why a swept probe could not be spawned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeError {
    /// A named field was NaN or infinite.
    NonFinite {
        /// The offending field, as the caller spelled it.
        field: &'static str,
    },
    /// The mass was not strictly positive.
    NonPositiveMass,
    /// A box half extent was not strictly positive.
    NonPositiveHalfExtent {
        /// The axis whose half extent was zero or negative.
        axis: &'static str,
    },
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::NonPositiveMass => write!(f, "mass_kg must be greater than zero"),
            Self::NonPositiveHalfExtent { axis } => {
                write!(f, "half_extents_m[{axis}] must be greater than zero")
            }
        }
    }
}

impl std::error::Error for ProbeError {}

/// The body the acceptance tests fly through (or into) the arch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProbeSpec {
    /// Initial world position, in meters.
    pub position_m: [f64; 3],
    /// Initial world velocity, in m/s.
    pub velocity_m_s: [f64; 3],
    /// Half extents of the probe's box, in meters.
    pub half_extents_m: [f64; 3],
    /// Total mass, in kilograms.
    pub mass_kg: f64,
}

impl ProbeSpec {
    /// Validates the probe before any entity exists.
    ///
    /// # Errors
    ///
    /// [`ProbeError::NonFinite`], [`ProbeError::NonPositiveMass`] or
    /// [`ProbeError::NonPositiveHalfExtent`], naming the field.
    pub fn validate(&self) -> Result<(), ProbeError> {
        for (value, field) in self
            .position_m
            .iter()
            .chain(self.velocity_m_s.iter())
            .chain([self.mass_kg].iter())
            .copied()
            .zip([
                "position_m[0]",
                "position_m[1]",
                "position_m[2]",
                "velocity_m_s[0]",
                "velocity_m_s[1]",
                "velocity_m_s[2]",
                "mass_kg",
            ])
        {
            if !value.is_finite() {
                return Err(ProbeError::NonFinite { field });
            }
        }
        if self.mass_kg <= 0.0 {
            return Err(ProbeError::NonPositiveMass);
        }
        for (axis, extent) in ["x", "y", "z"].into_iter().zip(self.half_extents_m) {
            if !extent.is_finite() {
                return Err(ProbeError::NonFinite {
                    field: "half_extents_m",
                });
            }
            if extent <= 0.0 {
                return Err(ProbeError::NonPositiveHalfExtent { axis });
            }
        }
        Ok(())
    }
}

/// Spawns a dynamic, swept-CCD body on the aircraft collision layer.
///
/// The probe opts in to [`SweptCcd`] because the acceptance scenario *is* a
/// high-speed sweep, and it sets [`SpeculativeMargin::ZERO`] so that sweep is
/// what the test measures. Measured on the pinned pair (`avian 0.7.0`,
/// `bevy 0.19.1`, `SubstepCount(1)`): with Avian's *default* speculative
/// margin the probe stops at the leg even with `SweptCcd` removed, so a test
/// over the defaults cannot tell a swept sweep from speculative collision;
/// with the margin zeroed, removing `SweptCcd` makes the same probe tunnel
/// from `x = -1.83` straight past the wall with an empty contact log, and
/// restoring it clamps the probe to `x = -0.75` — exactly the wall's near
/// face — on the crossing tick. The fixture therefore isolates the swept
/// path rather than relying on whichever default the pinned version ships.
///
/// Whether retail aircraft bodies run continuous detection at all is
/// unmeasured and is not claimed here. Gravity, the fixed rate and the
/// one-tick integration are the ones [`crate::physics`] already owns, so the
/// probe travels at a constant velocity until something stops it.
///
/// # Errors
///
/// [`ProbeError`] when the spec is invalid; nothing is spawned then.
pub fn spawn_swept_probe(app: &mut App, spec: &ProbeSpec) -> Result<Entity, ProbeError> {
    spawn_probe(app, spec, true)
}

/// Spawns the same body **without** [`SweptCcd`]: it is detected only where
/// a discrete sample actually overlaps another collider.
///
/// This is the body the sensor role is measured with, because of a measured
/// property of the pinned engine: Avian's swept CCD has no sensor filter —
/// `solve_swept_ccd` stops a body at the first time of impact against *any*
/// collider its swept path reaches, [`Sensor`] included — so a **swept**
/// body crossing a trigger volume is held at that volume's near face for the
/// crossing frame (measured here: 2.416 m of a 400 m/s probe's travel, which
/// is exactly the distance from its previous sample to the sensor face).
/// Sensors have no contact response of their own, so a body that is not
/// swept proves the role's own claim — *reports an overlap and never blocks
/// motion* — undisturbed. The swept/sensor interaction is recorded in
/// `docs/findings/2026-09-30-f18-a-world-instances-sectors-and-collision-roles.md`
/// and is a limitation for F18-B/C trigger volumes, not a claim about the
/// original.
///
/// [`SpeculativeMargin::ZERO`] is kept on this body too, so the *discrete
/// overlap* is what is measured, never a predicted contact.
///
/// # Errors
///
/// [`ProbeError`] when the spec is invalid; nothing is spawned then.
pub fn spawn_discrete_probe(app: &mut App, spec: &ProbeSpec) -> Result<Entity, ProbeError> {
    spawn_probe(app, spec, false)
}

fn spawn_probe(app: &mut App, spec: &ProbeSpec, swept: bool) -> Result<Entity, ProbeError> {
    spec.validate()?;
    let position = Vec3::new(
        spec.position_m[0] as f32,
        spec.position_m[1] as f32,
        spec.position_m[2] as f32,
    );
    let velocity = Vec3::new(
        spec.velocity_m_s[0] as f32,
        spec.velocity_m_s[1] as f32,
        spec.velocity_m_s[2] as f32,
    );
    let mut entity = app.world_mut().spawn((
        RigidBody::Dynamic,
        Collider::cuboid(
            (spec.half_extents_m[0] * 2.0) as f32,
            (spec.half_extents_m[1] * 2.0) as f32,
            (spec.half_extents_m[2] * 2.0) as f32,
        ),
        Mass(spec.mass_kg as f32),
        Transform::from_translation(position),
        Position(position),
        Rotation::default(),
        LinearVelocity(velocity),
        AngularVelocity(Vec3::ZERO),
        SpeculativeMargin::ZERO,
        CollisionEventsEnabled,
        avian_layers(CollisionLayers::from(CollisionLayer::Aircraft)),
    ));
    if swept {
        entity.insert(SweptCcd::default());
    }
    Ok(entity.id())
}

/// The layer set a spawned probe carries; exported so a test can assert the
/// probe and the world were built from the same designed matrix.
#[must_use]
pub fn probe_layers() -> AvianCollisionLayers {
    avian_layers(CollisionLayers::from(CollisionLayer::Aircraft))
}

/// The layer set every static world collider carries.
#[must_use]
pub fn static_world_layers() -> AvianCollisionLayers {
    avian_layers(CollisionLayers::from(CollisionLayer::StaticWorld))
}

/// The set of objects in a load record's population, for tests and callers
/// that want an explicit subset.
///
/// A tiny helper so a caller does not have to reach into
/// [`cs_content::world::WorldPopulation`] with its own iterator plumbing.
#[must_use]
pub fn object_set(keys: &[&str]) -> BTreeSet<WorldObjectId> {
    keys.iter().map(|key| object(key)).collect()
}

// ---------------------------------------------------------------- harness ---

/// Builds the headless Bevy world every world fixture runs on: the real pinned
/// plugin group through [`crate::asset_stack::headless_app`], the real F23-A
/// fixed-rate adapter, gravity zero, and a manually driven clock seeded so the
/// first counted update produces the full manual delta and exactly one fixed
/// step (see `docs/findings/2026-09-23-t334-first-frame-fixed-step.md`).
///
/// This is the one place that composition is written down, so a mesh-derived
/// collider — which needs the asset stack Avian's `collider-from-mesh` systems
/// read — cannot be built on a world that lacks it while the cuboid path works
/// fine.
pub fn world_app() -> App {
    let frame = Duration::from_secs_f64(1.0 / crate::physics::BASELINE_FIXED_HZ as f64);

    let mut app = crate::asset_stack::headless_app();
    app.add_plugins((
        // The world contact log and the mission-overlay pass belong to the
        // composition that runs a world: a load happens after `App::finish`,
        // where `add_plugins` would panic, so they are installed here or not at
        // all.
        super::contacts::WorldPlugin,
        super::overlays::WorldOverlayPlugin,
        // The resting rule (#428). The world composition does not install
        // `PhysicsBodiesPlugin` — it needs the world contact log and the
        // overlay pass, not the spawn preflight — so the resting rule is added
        // here explicitly. A body that has struck world geometry comes to rest
        // against it, which is what makes "the door opened" and "the body on it
        // moved" two separately recorded facts rather than one ambiguous
        // observation.
        crate::physics::RestingBodiesPlugin,
        crate::physics::PhysicsAdapterPlugin::new(crate::physics::BASELINE_FIXED_HZ),
    ));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(frame));
    app.insert_resource(SubstepCount(1));
    app.insert_resource(Gravity::ZERO);

    let startup = app.world().resource::<Time<Real>>().startup();
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .update_with_instant(startup);
    app.finish();
    app.cleanup();
    app
}

/// Why a [`WorldFixture`] could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldFixtureError {
    /// The definition could not be spawned into the Bevy/Avian world.
    Spawn(super::spawn::WorldSpawnError),
    /// The swept probe's spec was invalid; nothing was spawned.
    Probe(ProbeError),
}

impl std::fmt::Display for WorldFixtureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(err) => write!(f, "could not spawn the world: {err}"),
            Self::Probe(err) => write!(f, "could not spawn the probe: {err}"),
        }
    }
}

impl std::error::Error for WorldFixtureError {}

impl From<super::spawn::WorldSpawnError> for WorldFixtureError {
    fn from(err: super::spawn::WorldSpawnError) -> Self {
        Self::Spawn(err)
    }
}

impl From<ProbeError> for WorldFixtureError {
    fn from(err: ProbeError) -> Self {
        Self::Probe(err)
    }
}

/// Builds a [`WorldFixture`].
pub struct WorldFixtureBuilder {
    definition: WorldDefinition,
    meshes: WorldMeshes,
    probe: Option<ProbeSpec>,
    discrete_probe: bool,
}

impl WorldFixtureBuilder {
    /// Starts building a fixture for `definition`.
    #[must_use]
    pub fn new(definition: WorldDefinition) -> Self {
        Self {
            definition,
            meshes: WorldMeshes::new(),
            probe: None,
            discrete_probe: false,
        }
    }

    /// The geometry the definition's mesh references resolve to.
    ///
    /// A world built without one can still collide through every object whose
    /// record carries a `Cuboid`; a `FromMesh` object is then reported as
    /// [`SkipReason::MeshUnavailable`](super::spawn::SkipReason::MeshUnavailable)
    /// rather than given invented geometry.
    #[must_use]
    pub fn meshes(mut self, meshes: WorldMeshes) -> Self {
        self.meshes = meshes;
        self
    }

    /// Also spawn a swept probe. Without one, the fixture is a static world.
    #[must_use]
    pub const fn probe(mut self, spec: ProbeSpec) -> Self {
        self.probe = Some(spec);
        self.discrete_probe = false;
        self
    }

    /// Also spawn a probe with **no** continuous detection
    /// ([`spawn_discrete_probe`]): detection by discrete overlap only. This
    /// is the body a sensor volume must be measured with.
    #[must_use]
    pub const fn probe_discrete(mut self, spec: ProbeSpec) -> Self {
        self.probe = Some(spec);
        self.discrete_probe = true;
        self
    }

    /// Builds the fixture: the real pinned Bevy/Avian plugin group, the real
    /// F23-A fixed-rate adapter, gravity zero, and the definition spawned
    /// through [`super::spawn_world`].
    ///
    /// # Errors
    ///
    /// [`WorldFixtureError`] when the definition cannot be spawned or the
    /// probe's spec is invalid.
    pub fn build(self) -> Result<WorldFixture, WorldFixtureError> {
        let mut app = world_app();

        let spawned = super::spawn_world(&mut app, &self.definition, &self.meshes)?;
        let probe = match self.probe {
            Some(spec) => Some(if self.discrete_probe {
                spawn_discrete_probe(&mut app, &spec)?
            } else {
                spawn_swept_probe(&mut app, &spec)?
            }),
            None => None,
        };

        Ok(WorldFixture {
            app,
            definition: self.definition,
            spawned,
            probe,
            ticks: 0,
        })
    }
}

impl Default for WorldFixtureBuilder {
    /// A fixture for the synthetic arch world, with no probe.
    fn default() -> Self {
        Self::new(arch_world().expect("the synthetic arch world is well formed"))
    }
}

/// A headless world driven by the real fixed-rate physics adapter.
pub struct WorldFixture {
    app: App,
    definition: WorldDefinition,
    spawned: SpawnedWorld,
    probe: Option<Entity>,
    ticks: u64,
}

impl WorldFixture {
    /// Starts building a fixture for `definition`.
    #[must_use]
    pub fn builder(definition: WorldDefinition) -> WorldFixtureBuilder {
        WorldFixtureBuilder::new(definition)
    }

    /// A fixture for the synthetic arch world, with no probe.
    #[must_use]
    pub fn arch() -> WorldFixture {
        Self::builder(arch_world().expect("the synthetic arch world is well formed"))
            .build()
            .expect("the synthetic arch world spawns")
    }

    /// The definition that was spawned.
    #[must_use]
    pub fn definition(&self) -> &WorldDefinition {
        &self.definition
    }

    /// What [`super::spawn_world`] produced.
    #[must_use]
    pub fn spawned(&self) -> &SpawnedWorld {
        &self.spawned
    }

    /// The probe entity, when one was requested.
    #[must_use]
    pub fn probe(&self) -> Option<Entity> {
        self.probe
    }

    /// Read-only access to the Bevy world, for diagnostics and probes.
    #[must_use]
    pub fn world(&self) -> &bevy::prelude::World {
        self.app.world()
    }

    /// Mutable access to the Bevy world, for a load, an unload or a
    /// [`WorldResidency`](super::residency::WorldResidency) read.
    ///
    /// [`spawn_world`](super::spawn::spawn_world) and
    /// [`load_world`](super::residency::load_world) take an `&mut App`, so
    /// changing a loaded world means handing the whole app over; this is the
    /// borrow that makes that possible from a test.
    #[allow(clippy::mut_from_ref)]
    pub fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }

    /// Spawns a swept probe *after* the world was built.
    ///
    /// A mesh-derived collider does not exist until Avian's hierarchy
    /// constructor has run ([`MESH_SETTLE_UPDATES`] updates), so a probe
    /// spawned at build time would already be through the geometry before the
    /// collision it is supposed to meet exists.
    ///
    /// # Errors
    ///
    /// [`ProbeError`] when the spec is invalid; nothing is spawned then.
    pub fn spawn_swept_probe(&mut self, spec: ProbeSpec) -> Result<Entity, ProbeError> {
        spawn_swept_probe(&mut self.app, &spec)
    }

    /// Spawns a probe with no continuous detection, after the world was built:
    /// detection by discrete overlap only.
    ///
    /// # Errors
    ///
    /// [`ProbeError`] when the spec is invalid; nothing is spawned then.
    pub fn spawn_discrete_probe(&mut self, spec: ProbeSpec) -> Result<Entity, ProbeError> {
        spawn_discrete_probe(&mut self.app, &spec)
    }

    /// The probe's current position in meters, or `None` without a probe.
    #[must_use]
    pub fn probe_position(&self) -> Option<Vec3> {
        let probe = self.probe?;
        self.app.world().get::<Position>(probe).map(|p| p.0)
    }

    /// The probe's current linear velocity in m/s, or `None` without a probe.
    #[must_use]
    pub fn probe_velocity(&self) -> Option<Vec3> {
        let probe = self.probe?;
        self.app.world().get::<LinearVelocity>(probe).map(|v| v.0)
    }

    /// Every contact recorded so far.
    #[must_use]
    pub fn contacts(&self) -> &[super::contacts::WorldContact] {
        self.app
            .world()
            .resource::<super::contacts::WorldContacts>()
            .contacts()
    }

    /// The number of fixed ticks stepped so far.
    #[must_use]
    pub const fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Advances the world by exactly `ticks` fixed steps.
    pub fn step(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.app.update();
            self.ticks += 1;
        }
    }
}
