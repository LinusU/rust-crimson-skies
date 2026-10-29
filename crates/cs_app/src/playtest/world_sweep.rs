//! The ground-blow world segment sweep (`0x4c8ec0`) over the playtest's own
//! colliders (task #2214).
//!
//! The original's ground-blow probe `0x48bf60` casts one segment from the
//! vehicle's position along the negated third row of its matrix (`-V[+0x198]`,
//! the body's nose in this port's `-Z` convention) for `groundblow_elev` = 400
//! metres, and reads the nearest contact of the one-segment world query
//! `0x4c8ec0` (`docs/findings/2026-10-29-groundblow-world-segment-sweep.md`,
//! sections 0.4, 1–5 and 8). This module is the port's query for that answer,
//! over the colliders the playtest already spawns plus the live aircraft, and
//! the fill that hands it to `OriginalInput::segment_probe`.
//!
//! # What the record keeps
//!
//! The five things the decode says the record's reader depends on, and how
//! this module produces each:
//!
//! 1. **The segment** is [`ground_blow_segment`]: from the body's position
//!    along `-V[+0x198]`, `groundblow_elev_m` long. The port's nose
//!    (`-Z` body) *is* the negated third row, so the axis is
//!    [`nose_direction`].
//! 2. **The population** is the colliders the port spawns for original
//!    content — [`PlaytestSea`] water tiles and [`WorldSceneryCollider`] land,
//!    mountains and cliffs (#1814) — plus every body on the `Aircraft` layer
//!    but the prober. The original tests every node in its world database; the
//!    port has no node database, so it tests exactly the colliders it spawned
//!    for that database's measured content, and nothing else. Mission ground
//!    targets and turrets are **not** tested: they are not world-database nodes
//!    in the port, and the owner's scope for this stage names the static
//!    scenery and the live aircraft.
//! 3. **The normal is unnormalised**: the hit face's own edge cross product
//!    (`0x55d6d2`..`0x55d73a`), whose magnitude is the face's parallelogram
//!    area — for a triangle `|N| = 2 * area`, for a box face
//!    `|N| = width * height`, for a stored quad its parallelogram
//!    (`2026-10-29-groundblow-normal-scale.md`). parry's normalised hit
//!    normal is never stored. The face itself is the **stored polygon** the
//!    hit triangle stands in for — the collider's [`ColliderFaces`] side
//!    table maps each solver triangle back to its polygon record — or the
//!    collider triangle itself where no stored record exists.
//! 4. **The distance is the chord** `|point - position|` (`0x5388d0`,
//!    `0x48c131`), never the along-ray time of impact. The two agree only
//!    because the probe axis is a unit body axis; the record carries the chord
//!    the original computes.
//! 5. **The class-type filter** is the sweep's dispatch on `node+0x34`
//!    (`0x4c9ac8`, table `0x4c9de0`): types `1, 5, 6, 7, 8, 9, 10` are
//!    intersected, and `2, 3, 4` and anything outside `1..=10` are not.
//!    #2213 measured that **every** M01 collider — all 2393 mesh-bearing
//!    scenery nodes, the sea's 347 water tiles, the aircraft nodes — is class
//!    **5** (`object3d`), so the filter removes nothing from this port's
//!    population; it is applied per target anyway, from
//!    [`OBJECT3D_CLASS_TYPE`], so a future spawn that carries a refused class
//!    has one place to say so.
//!
//! The owner is resolved to [`SegmentProbeOwner`] the way the player's skip
//! gate distinguishes the three cases (§5 of the sweep finding): scenery and
//! the sea are [`SegmentProbeOwner::World`]; an aircraft body is
//! [`SegmentProbeOwner::Vehicle`]. [`SegmentProbeOwner::UnresolvedVehicleNode`]
//! is the original's "the node carries the vehicle bit but the registry
//! `0x71dab8` does not resolve it" case; the port has no node→vehicle registry
//! and knows which entity owns the geometry it hit, so it never needs that
//! walk and never produces that variant. `on_path` is the vehicle's `+0xcc`
//! byte, which the record loader sets when the record's taxi-path field
//! resolves to a `*_aipath` node with at least two points (`0x47c3a9` —
//! `0x47c568`). The port carries that flag on
//! [`crate::mission_actors::ScriptedAircraftActor::on_path`] (#2215): the
//! spawn resolves the record's field-40 name through the compose's
//! [`crate::mission_session::MissionTaxiPaths`] table,
//! so an aircraft whose record names a path reports `on_path: true` here —
//! and M01's five measured records name none, so they state `false`, the
//! loader's own answer for them, not a claim that no retail aircraft taxis.
//!
//! # What this query does **not** reproduce, and why
//!
//! * **An aircraft is a box, not its mesh.** The port's player aircraft body
//!   carries the designed 12 x 2 x 10 m box collider (an AI aircraft the
//!   designed 12 x 4 x 12 m one); the original reaches the
//!   aircraft's own mesh-bearing nodes instead. A hit on an aircraft therefore
//!   follows the engine's *measured* box-face rule (`0x55d270`: the face's
//!   outward normal scaled by the face's area) rather than the aircraft mesh's
//!   triangles, because the port has no aircraft mesh collider to intersect.
//! * **The layer filter `0x56c430(node+0x30)` is not applied.** It is runtime
//!   state (`[0x62cb50]`, the list at `0xa070a8`) whose value at ground-blow
//!   time is unmeasured (§6 of the class-type finding); the port has no
//!   equivalent list, so it cannot apply it without guessing.
//! * **The 32-contact abort (`0x4c9a9d`) is not reproduced.** The original
//!   aborts the whole query — and reports no contact — when a 33rd record would
//!   be appended; this sweep keeps the nearest contact instead. The abort
//!   counts the original's appended *face records* across the whole node walk;
//!   a collider without stored faces still contributes per triangle here, so
//!   the port's count is not the original's and the cap is left out rather
//!   than approximated (#2217 records the polygon-side part of the gap).

use std::collections::HashSet;

use avian3d::parry::query::{Ray, RayCast};
use avian3d::prelude::{Collider, Position, Rotation};
use bevy::ecs::entity::Entity;
use bevy::ecs::system::{Query, ResMut, SystemParam};
use bevy::prelude::{Has, Quat, Resource, Vec3, World};
use cs_content::mesh::CollisionFace;
use cs_sim::collision::CollisionLayer;
use cs_sim::flight::original::{OriginalInput, SegmentProbeHit, SegmentProbeOwner, nose_direction};
use cs_types::space::Quaternion;

use super::scene::{PlaytestOriginalFlight, PlaytestSea};
use crate::mission_session::{WorldSceneryCollider, WorldSweepNode};
use crate::physics::BodyLayer;
use crate::render::bevy_mesh::ColliderFaces;

/// The class type every M01 collider stores, and the type the sweep's dispatch
/// handles with the node's own mesh triangles plus a recursion into its
/// children (`0x4c9adf`, `object3d`): #2213 measured all 2393 mesh-bearing
/// scenery nodes, all 347 water tiles and the aircraft nodes as class 5.
pub const OBJECT3D_CLASS_TYPE: u32 = 5;

/// The node class types the sweep dispatches to a handler that can intersect
/// geometry (`0x4c9ac8`: `lea ecx,[eax-1]; cmp ecx,9; ja` with the ten-entry
/// table at `0x4c9de0`). Types `2`, `3` and `4` log
/// `Unrecognized node class type` and test nothing; `0` and `11` and above are
/// refused by the same guard.
pub const INTERSECTED_CLASS_TYPES: [u32; 7] = [1, 5, 6, 7, 8, 9, 10];

/// The class types the sweep's dispatch **refuses** by name (`2`, `3`, `4`).
pub const REFUSED_CLASS_TYPES: [u32; 3] = [2, 3, 4];

/// Whether the sweep's dispatch intersects a node of `class_type`
/// (`0x4c9ac8` and the table at `0x4c9de0`).
#[must_use]
pub const fn class_type_intersects(class_type: u32) -> bool {
    matches!(class_type, 1 | 5 | 6 | 7 | 8 | 9 | 10)
}

/// The segment the ground-blow probe `0x48bf60` casts: from the body's
/// position along `-V[+0x198]`, `groundblow_elev_m` long.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundBlowSegment {
    /// The body's position, metres: the original's `p0`.
    pub from_m: [f64; 3],
    /// The segment's far end, metres: `p0 + axis * length`.
    pub to_m: [f64; 3],
    /// The unit direction the probe runs along: the body's nose, `-V[+0x198]`.
    pub axis: [f64; 3],
    /// The probe's length, metres (`groundblow_elev`, retail 400).
    pub length_m: f64,
}

impl GroundBlowSegment {
    /// Whether the segment has a length the query can test.
    #[must_use]
    pub fn is_degenerate(&self) -> bool {
        !self.length_m.is_finite() || self.length_m <= 0.0
    }
}

/// The probe's segment for a body at `position_m` with attitude `orientation`:
/// along [`nose_direction`] (the port's nose is `-Z` body, the original's
/// negated third matrix row) for `elev_m` metres.
#[must_use]
pub fn ground_blow_segment(
    position_m: [f64; 3],
    orientation: Quaternion,
    elev_m: f64,
) -> GroundBlowSegment {
    let axis = nose_direction(orientation);
    GroundBlowSegment {
        from_m: position_m,
        to_m: [
            position_m[0] + axis[0] * elev_m,
            position_m[1] + axis[1] * elev_m,
            position_m[2] + axis[2] * elev_m,
        ],
        axis,
        length_m: elev_m,
    }
}

/// One contact of the sweep: the facts the original's record carries that its
/// readers use, plus the collider that answered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldSweepContact {
    /// The collider entity the segment met.
    pub entity: Entity,
    /// The contact point in world space, metres (record `+0x0c`).
    pub point_m: [f64; 3],
    /// The hit face's **unnormalised** normal (record `+0x00`): its magnitude
    /// is the face's parallelogram area.
    pub normal: [f64; 3],
    /// The chord `|point - from|`, metres (record `+0x1c` is its square; the
    /// ground blow recomputes the chord itself).
    pub distance_m: f64,
    /// What the hit's owner resolved to.
    pub owner: SegmentProbeOwner,
}

impl WorldSweepContact {
    /// The three facts [`SegmentProbeHit`] carries.
    #[must_use]
    pub fn hit(&self) -> SegmentProbeHit {
        SegmentProbeHit {
            distance_m: self.distance_m,
            normal: self.normal,
            owner: self.owner,
        }
    }
}

/// One collider the sweep tests, at the pose it is tested at, with the owner
/// its hit resolves to and its node class type.
#[derive(Clone, Copy)]
pub struct SweepTarget<'a> {
    /// The collider entity.
    pub entity: Entity,
    /// The collider, read through [`Collider::shape_scaled`] so the shape is the
    /// geometry Avian's solver and broad phase see (its transform scale is
    /// already applied).
    pub collider: &'a Collider,
    /// The body's pose (`Position`).
    pub position: Vec3,
    /// The body's attitude (`Rotation`).
    pub rotation: Quat,
    /// What a hit on this collider resolves to.
    pub owner: SegmentProbeOwner,
    /// The node class type the sweep's dispatch tests (`node+0x34`).
    pub class_type: u32,
    /// The collider's stored-face side table (task #2217): which stored
    /// polygon — corners and flag word — each collision triangle stands in
    /// for. `None` on a collider with no stored polygon record, which keeps
    /// per-triangle behaviour.
    pub faces: Option<&'a ColliderFaces>,
}

/// The owner one collider resolves to for the player's skip gate, or `None`
/// when the sweep does not test the collider at all.
///
/// The five inputs are everything the production query and the read-only
/// [`probe_in_world`] path can both see: the two scenery markers, the body's
/// declaration layer, whether the entity is the prober, and the vehicle's
/// own `+0xcc` on-path flag — `false` for a body that is not a scripted
/// aircraft at all, exactly as the original's registry walk resolves no
/// vehicle there and the gate's `+0xcc == 0` default applies.
#[must_use]
pub fn owner_of(
    is_sea: bool,
    is_scenery: bool,
    is_node: bool,
    layer: CollisionLayer,
    is_prober: bool,
    on_path: bool,
) -> Option<SegmentProbeOwner> {
    if is_prober {
        // `0x4cca30(node, 0)` clears the probing vehicle's own node so the
        // probe cannot hit its own geometry.
        return None;
    }
    if is_sea || is_scenery || is_node {
        return Some(SegmentProbeOwner::World);
    }
    if layer == CollisionLayer::Aircraft {
        // The port resolves the aircraft body itself, so the registry walk of
        // `0x4afee0` has nothing to search; `on_path` is the vehicle's own
        // `+0xcc` byte, imported on `ScriptedAircraftActor` (#2215).
        return Some(SegmentProbeOwner::Vehicle { on_path });
    }
    None
}

/// The declaration one collider entity carries: its body layer and the two
/// scenery markers. Both the production query and [`probe_in_world`] read these
/// and nothing else decides the sweep's population.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColliderKind {
    /// The body's declared collision layer.
    pub layer: CollisionLayer,
    /// Whether the collider carries [`PlaytestSea`].
    pub is_sea: bool,
    /// Whether the collider carries [`WorldSceneryCollider`].
    pub is_scenery: bool,
    /// The class type of the collider's [`WorldSweepNode`], when it carries
    /// one (a world-definition object whose node the original sweeps).
    pub node_class: Option<u32>,
}

/// The [`SweepTarget`] for one collider entity, or `None` when the sweep does
/// not test it.
///
/// Eight explicit parts rather than a builder: every one is something both
/// callers — the system query and [`probe_in_world`] — can see of the entity,
/// stated together so the two paths cannot disagree about what decides the
/// population.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn target_of<'a>(
    entity: Entity,
    collider: &'a Collider,
    position: Vec3,
    rotation: Quat,
    kind: ColliderKind,
    prober: Entity,
    on_path: bool,
    faces: Option<&'a ColliderFaces>,
) -> Option<SweepTarget<'a>> {
    Some(SweepTarget {
        entity,
        collider,
        position,
        rotation,
        owner: owner_of(
            kind.is_sea,
            kind.is_scenery,
            kind.node_class.is_some(),
            kind.layer,
            entity == prober,
            on_path,
        )?,
        class_type: kind.node_class.unwrap_or(OBJECT3D_CLASS_TYPE),
        faces,
    })
}

/// The nearest contact of `segment` over `targets`, or `None` when the segment
/// meets nothing the original would report.
///
/// The sweep collects every contact of its segment and `0x4c8ec0` picks the one
/// with the smallest `|p0 - point|^2` (`0x4c8f0a`, `0x4c8f2d`); the chord is
/// the same order as the distance along a unit axis, and the search here keeps
/// the strictly smaller chord, as the original's minimum does.
pub fn nearest_contact<'a>(
    segment: &GroundBlowSegment,
    targets: impl IntoIterator<Item = SweepTarget<'a>>,
) -> Option<WorldSweepContact> {
    if segment.is_degenerate() {
        return None;
    }
    let from = Vec3::new(
        segment.from_m[0] as f32,
        segment.from_m[1] as f32,
        segment.from_m[2] as f32,
    );
    let to = Vec3::new(
        segment.to_m[0] as f32,
        segment.to_m[1] as f32,
        segment.to_m[2] as f32,
    );
    if !from.is_finite() || !to.is_finite() || to == from {
        return None;
    }
    let mut best: Option<WorldSweepContact> = None;
    for target in targets {
        if !class_type_intersects(target.class_type) {
            continue;
        }
        let Some(contact) = contact_of(&target, from, to) else {
            continue;
        };
        if best.is_none_or(|best| contact.distance_m < best.distance_m) {
            best = Some(contact);
        }
    }
    best
}

/// The contact `target` gives the segment `from..to`, at the record's own
/// conventions.
fn contact_of(target: &SweepTarget<'_>, from: Vec3, to: Vec3) -> Option<WorldSweepContact> {
    let (point, normal) = face_record(target, from, to)?;
    let point_m = [f64::from(point.x), f64::from(point.y), f64::from(point.z)];
    let chord = [
        point_m[0] - f64::from(from.x),
        point_m[1] - f64::from(from.y),
        point_m[2] - f64::from(from.z),
    ];
    Some(WorldSweepContact {
        entity: target.entity,
        point_m,
        normal: [
            f64::from(normal.x),
            f64::from(normal.y),
            f64::from(normal.z),
        ],
        distance_m: chord[0].hypot(chord[1]).hypot(chord[2]),
        owner: target.owner,
    })
}

/// The `(point, unnormalised normal)` a collider's nearest accepted face gives
/// the segment, from the shape the collider actually holds.
fn face_record(target: &SweepTarget<'_>, from: Vec3, to: Vec3) -> Option<(Vec3, Vec3)> {
    let shape = target.collider.shape_scaled();
    if let Some(mesh) = shape.as_trimesh() {
        return trimesh_contact(target, mesh, from, to);
    }
    if let Some(cuboid) = shape.as_cuboid() {
        return cuboid_contact(target, cuboid, from, to);
    }
    // A shape whose face record is not measured is never guessed: the port's
    // original-content colliders are triangle meshes and the aircraft box, and
    // anything else answers nothing rather than an invented normal.
    None
}

/// The measured one-sided face gate of `0x55d6c0`: the segment's start is
/// strictly on the `+N` side of the face's plane and its far end strictly on
/// the `-N` side, so `N` faces the incoming segment.
#[must_use]
pub fn face_accepts(plane_point: Vec3, normal: Vec3, from: Vec3, to: Vec3) -> bool {
    let start = (from - plane_point).dot(normal);
    let end = (to - plane_point).dot(normal);
    start > 0.0 && end < 0.0
}

/// The nearest accepted face of a triangle mesh, in the mesh's own frame, as
/// `(point, unnormalised normal)`.
///
/// Where the collider carries its stored-face side table
/// ([`SweepTarget::faces`], task #2217) a candidate triangle resolves to the
/// **stored polygon** it stands in for: `0x55c9c0` tests the stored face —
/// the whole outline, or a strip step's three corners — so each stored face
/// is tested once, with its own stored corners, its own flag word and the
/// cyclic inside-edge walk of `0x55d6c0`, and the record keeps the stored
/// face's normal and the crossing point. A triangle with no face record keeps
/// the per-triangle test below.
fn trimesh_contact(
    target: &SweepTarget<'_>,
    mesh: &avian3d::parry::shape::TriMesh,
    from: Vec3,
    to: Vec3,
) -> Option<(Vec3, Vec3)> {
    let (local_from, local_to) = local_segment(target, from, to);
    let delta = local_to - local_from;
    let length = delta.length();
    if length <= 0.0 || !length.is_finite() {
        return None;
    }
    let ray = Ray::new(local_from, delta / length);
    let mut best: Option<(f32, Vec3, Vec3)> = None;
    // A stored face is tested once: several collision triangles can name the
    // same record (an outline's ear-clipped halves all stand in for its one
    // stored polygon), and the face test covers the whole polygon whichever
    // triangle the BVH leaf named.
    let mut tested_faces: HashSet<u32> = HashSet::new();
    // The mesh's own BVH prunes to the triangles the segment's box meets; each
    // surviving triangle is then tested exactly, because parry's nearest hit
    // alone would drop a back face the original rejects in favour of a farther
    // front face it accepts.
    for leaf in mesh
        .bvh()
        .leaves(|node| node.aabb().intersects_local_ray(&ray, length))
    {
        let index = leaf as usize;
        if index >= mesh.num_triangles() {
            continue;
        }
        // A triangle the side table maps to a stored face is tested only as
        // that face — the original has no per-triangle answer for it, so a
        // rejected face is rejected (its triangle cannot hit where the
        // polygon did not) and its other ear-clipped halves are skipped.
        if let Some(face_index) = target.faces.and_then(|faces| faces.face_index(index)) {
            if tested_faces.insert(face_index)
                && let Some(faces) = target.faces
                && let Some(face) = faces.face(face_index)
                && let Some((t, point, normal)) =
                    stored_face_contact(face, local_from, delta, local_to)
                && best.is_none_or(|(best_t, _, _)| t < best_t)
            {
                best = Some((t, point, normal));
            }
            continue;
        }
        let triangle = mesh.triangle(leaf);
        let (a, b, c) = (triangle.a, triangle.b, triangle.c);
        let normal = (b - a).cross(c - a);
        if !face_accepts(a, normal, local_from, local_to) {
            continue;
        }
        let Some(t) = segment_triangle(local_from, delta, a, b, c) else {
            continue;
        };
        if best.is_none_or(|(best_t, _, _)| t < best_t) {
            best = Some((t, local_from + delta * t, normal));
        }
    }
    let (_, point, normal) = best?;
    Some((world_point(target, point), target.rotation * normal))
}

/// `0x55d6c0` on one **stored** face: `(t, point, unnormalised normal)` of the
/// segment's accepted crossing, or `None` when the face rejects it.
///
/// The decode, applied to `face`'s own stored corners:
///
/// * the record normal is `(corner1 - corner0) × (corner2 - corner0)` —
///   unnormalised, so a stored quad's magnitude is its parallelogram area,
///   not twice an ear-clipped triangle's;
/// * the plane gate: the segment's far end must sit strictly on the `-N`
///   side **unless the face's stored two-sided bit is set**
///   (`0x55d75b`, flag `= (vertex_info >> 10) & 1` pushed at `0x55cc73`),
///   then the sign-bit test requires the segment to straddle the plane
///   (`0x55d7a5`: `+0.0` counts as the positive side);
/// * the point is the plane crossing `from + (d1 / (d1 - d2)) * delta`;
/// * the inside test walks **every** cyclic edge of the face: the projected
///   half-plane test of `0x55d7ec` on, which in 3D is
///   `dot(edge × (point - corner), N) > -1e-4 * max|N_i|` — the double
///   tolerance `0x609180` scaled to the dominant normal axis the original's
///   six-case projection divides by.
fn stored_face_contact(
    face: &CollisionFace,
    from: Vec3,
    delta: Vec3,
    to: Vec3,
) -> Option<(f32, Vec3, Vec3)> {
    if face.corners.len() < 3 {
        return None;
    }
    let corners: Vec<Vec3> = face
        .corners
        .iter()
        .map(|&corner| Vec3::from(corner))
        .collect();
    let origin = corners[0];
    let normal = (corners[1] - origin).cross(corners[2] - origin);
    let d1 = (from - origin).dot(normal);
    let d2 = (to - origin).dot(normal);
    // The one-sided gate, overridden by the stored two-sided bit.
    if !(d2 < 0.0 || face.is_two_sided()) {
        return None;
    }
    // Opposite sign bits: `+0.0` is on the positive side, `-0.0` is not.
    if d1.is_sign_negative() == d2.is_sign_negative() {
        return None;
    }
    let t = d1 / (d1 - d2);
    let point = from + delta * t;
    // The cyclic inside-edge walk over every stored corner.
    let dominant = normal.x.abs().max(normal.y.abs()).max(normal.z.abs());
    let tolerance = -1e-4 * dominant;
    for index in 0..corners.len() {
        let corner = corners[index];
        let edge = corners[(index + 1) % corners.len()] - corner;
        let inside = edge.cross(point - corner).dot(normal);
        // The original's strict `>` test: anything not strictly inside —
        // outside, on the tolerance, or NaN — rejects the face.
        if inside.partial_cmp(&tolerance) != Some(std::cmp::Ordering::Greater) {
            return None;
        }
    }
    Some((t, point, normal))
}

/// The contact a cuboid gives the segment, as `(point, unnormalised normal)`.
///
/// A box face's record follows `0x55d270`'s measured rule: the face's outward
/// normal scaled by the face's area (`width * height`, `2026-10-29-groundblow-
/// normal-scale.md`), which is the cross product of the face's two edges.
fn cuboid_contact(
    target: &SweepTarget<'_>,
    cuboid: &avian3d::parry::shape::Cuboid,
    from: Vec3,
    to: Vec3,
) -> Option<(Vec3, Vec3)> {
    let (local_from, local_to) = local_segment(target, from, to);
    let delta = local_to - local_from;
    let length = delta.length();
    if length <= 0.0 || !length.is_finite() {
        return None;
    }
    let ray = Ray::new(local_from, delta / length);
    let hit = cuboid.cast_local_ray_and_get_normal(&ray, length, false)?;
    let point = local_from + ray.dir * hit.time_of_impact;
    let dims = cuboid.half_extents.abs() * 2.0;
    let axes = hit.normal.abs();
    // The face's area: the product of the box's two full extents across it.
    let area = if axes.x >= axes.y && axes.x >= axes.z {
        dims.y * dims.z
    } else if axes.y >= axes.z {
        dims.x * dims.z
    } else {
        dims.x * dims.y
    };
    let normal = hit.normal.normalize_or_zero() * area;
    if !face_accepts(point, normal, local_from, local_to) {
        return None;
    }
    Some((world_point(target, point), target.rotation * normal))
}

/// The segment in the collider's own frame.
fn local_segment(target: &SweepTarget<'_>, from: Vec3, to: Vec3) -> (Vec3, Vec3) {
    let inverse = target.rotation.inverse();
    (
        inverse * (from - target.position),
        inverse * (to - target.position),
    )
}

/// A point in the collider's frame, in world space.
fn world_point(target: &SweepTarget<'_>, local: Vec3) -> Vec3 {
    target.position + target.rotation * local
}

/// `t` in `[0, 1]` at which the segment `from + t * delta` crosses the
/// two-sided triangle `a, b, c`, or `None`. The face's winding is not
/// consulted here: the original accepts a face by its plane sides
/// ([`face_accepts`]), not by which side of the triangle the ray met.
fn segment_triangle(from: Vec3, delta: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let e1 = b - a;
    let e2 = c - a;
    let p = delta.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return None;
    }
    let inverse = 1.0 / det;
    let t_vec = from - a;
    let u = t_vec.dot(p) * inverse;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = t_vec.cross(e1);
    let v = delta.dot(q) * inverse;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inverse;
    (0.0..=1.0).contains(&t).then_some(t)
}

/// The colliders the sweep tests, as a system parameter.
///
/// The query reads every collider that carries a body pose; [`target_of`] then
/// keeps the static scenery, the sea and the aircraft and drops everything
/// else. The query's own `&Rotation` is why it is a separate system parameter
/// from the flight driver's `&mut Rotation`: see [`fill_groundblow_probes`].
#[derive(SystemParam)]
#[allow(clippy::type_complexity)]
pub struct SweepColliders<'w, 's> {
    colliders: Query<
        'w,
        's,
        (
            Entity,
            &'static Collider,
            &'static Position,
            &'static Rotation,
            &'static BodyLayer,
            Has<PlaytestSea>,
            Has<WorldSceneryCollider>,
            Option<&'static crate::mission_actors::ScriptedAircraftActor>,
            Option<&'static WorldSweepNode>,
            Option<&'static ColliderFaces>,
        ),
    >,
}

impl SweepColliders<'_, '_> {
    /// Every collider the sweep tests.
    fn targets(&self, prober: Entity) -> impl Iterator<Item = SweepTarget<'_>> {
        self.colliders.iter().filter_map(
            move |(
                entity,
                collider,
                position,
                rotation,
                layer,
                is_sea,
                is_scenery,
                actor,
                node,
                faces,
            )| {
                let kind = ColliderKind {
                    layer: layer.0,
                    is_sea,
                    is_scenery,
                    node_class: node.map(|node| node.class_type),
                };
                target_of(
                    entity,
                    collider,
                    position.0,
                    rotation.0,
                    kind,
                    prober,
                    actor.is_some_and(|actor| actor.on_path),
                    faces,
                )
            },
        )
    }

    /// The nearest contact of `segment` for `prober`.
    #[must_use]
    pub fn nearest(
        &self,
        segment: &GroundBlowSegment,
        prober: Entity,
    ) -> Option<WorldSweepContact> {
        nearest_contact(segment, self.targets(prober))
    }
}

/// The ground-blow probes cast for this tick's flying records (task #2214).
///
/// The fill is a system of its own ([`fill_groundblow_probes`]) because the
/// driver that steps the law holds `&mut Rotation` and the sweep has to read
/// the same pose; one system cannot hold both queries. The driver reads this
/// resource and hands each entry to the law's input.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct GroundBlowProbes {
    /// The hit filled for each record that cast a probe this tick, by entity.
    /// Only records whose sweep found a contact appear: the original's "no
    /// contact" is a zero amount, not a record, and the input carries `None`.
    pub hits: Vec<(Entity, SegmentProbeHit)>,
    /// Probes cast on the tick this resource was last filled.
    pub cast_this_tick: u64,
    /// Probes cast since the session began.
    pub cast_total: u64,
}

impl GroundBlowProbes {
    /// The probe filled for `entity` this tick, if the sweep found a contact.
    #[must_use]
    pub fn hit(&self, entity: Entity) -> Option<SegmentProbeHit> {
        self.hits
            .iter()
            .find(|(filled, _)| *filled == entity)
            .map(|(_, hit)| *hit)
    }
}

/// Casts the ground-blow probe for every flying record, player and AI, and
/// stores it in [`GroundBlowProbes`] (tasks #2214, #2224).
///
/// The segment is cast from the same pre-step pose the driver is about to seed
/// its law from (`Position`/`Rotation`), along the body's nose, for the
/// record's own `groundblow_elev_m`. A crashed record is skipped exactly as the
/// driver skips it: its flight is over.
pub fn fill_groundblow_probes(
    mut probes: ResMut<GroundBlowProbes>,
    colliders: SweepColliders,
    records: Query<(
        Entity,
        &PlaytestOriginalFlight,
        &Position,
        &Rotation,
        Option<&super::fatal_contact::FatalContactRule>,
    )>,
) {
    probes.hits.clear();
    probes.cast_this_tick = 0;
    for (entity, flight, position, rotation, rule) in &records {
        if rule.is_some_and(|rule| rule.crash.is_some()) {
            continue;
        }
        let segment = ground_blow_segment(
            [
                f64::from(position.0.x),
                f64::from(position.0.y),
                f64::from(position.0.z),
            ],
            super::to_quaternion(rotation.0),
            flight.model.globals.groundblow_elev_m,
        );
        if let Some(contact) = colliders.nearest(&segment, entity) {
            probes.hits.push((entity, contact.hit()));
        }
        probes.cast_this_tick += 1;
    }
    probes.cast_total += probes.cast_this_tick;
}

/// The input one flying record's law steps with: `input` with the ground blow's
/// world-sweep answer overlaid, for every seat (tasks #2214, #2224).
///
/// [`fill_groundblow_probes`] casts the probe earlier in the same tick and
/// [`GroundBlowProbes`] carries its answer; this is the one place that hands it
/// to the law, so a caller (the flight driver, or a test using the same pose)
/// cannot disagree about which records get it. The original casts the probe
/// for every vehicle it runs the force block for, so an AI record is overlaid
/// exactly like the player's. A record whose sweep found no contact stays
/// `None`, which is the original's zero amount rather than a record.
#[must_use]
pub fn overlay_groundblow_probe(
    mut input: OriginalInput,
    entity: Entity,
    probes: &GroundBlowProbes,
) -> OriginalInput {
    input.segment_probe = probes.hit(entity);
    input
}

/// The ground-blow probe for a body at `position` with attitude `rotation`,
/// read from a `&World` instead of from inside a system.
///
/// This is the same query [`SweepColliders::nearest`] answers — the same
/// population ([`target_of`]), the same rule, the same colliders — reached
/// through [`World::iter_entities`] so a caller outside a system (an
/// acceptance test, a report) can recompute exactly what the fill stored.
#[must_use]
pub fn probe_in_world(
    world: &World,
    position: Vec3,
    rotation: Quat,
    elev_m: f64,
    prober: Entity,
) -> Option<WorldSweepContact> {
    let segment = ground_blow_segment(
        [
            f64::from(position.x),
            f64::from(position.y),
            f64::from(position.z),
        ],
        super::to_quaternion(rotation),
        elev_m,
    );
    let targets = world.iter_entities().filter_map(|entity| {
        let collider = entity.get::<Collider>()?;
        let body = entity.get::<Position>()?;
        let rotation = entity.get::<Rotation>()?;
        let layer = entity.get::<BodyLayer>()?;
        let kind = ColliderKind {
            layer: layer.0,
            is_sea: entity.contains::<PlaytestSea>(),
            is_scenery: entity.contains::<WorldSceneryCollider>(),
            node_class: entity.get::<WorldSweepNode>().map(|node| node.class_type),
        };
        let on_path = entity
            .get::<crate::mission_actors::ScriptedAircraftActor>()
            .is_some_and(|actor| actor.on_path);
        target_of(
            entity.id(),
            collider,
            body.0,
            rotation.0,
            kind,
            prober,
            on_path,
            entity.get::<ColliderFaces>(),
        )
    });
    nearest_contact(&segment, targets)
}
