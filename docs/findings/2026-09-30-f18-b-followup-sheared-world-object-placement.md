# F18-A follow-up (#421): how a world object with an unrepresentable authored matrix is placed

Date: 2026-09-30. Task: #421 "Decide how a world object with an unrepresentable
authored matrix is placed" (key `F18-B-followup-sheared-mesh-placement`), filed
by F18-B (#86) out of F18-A's review. Spec:
`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`
(non-negotiable behavior 1, acceptance test AC01). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary build/test
only — no `CS_GAME_DIR` read, no evidence report required, nothing
`verified_original`. Test prefix: `accept_f18_b_shear_`.

## The question, and the answer

`spawn_world` refused a **whole world** when any instance's authored matrix had
no translation/rotation/scale decomposition (F18-A). This task decides whether a
sheared world object is refused, placed asymmetrically, or placed exactly. The
answer is the third: **it is placed exactly, in both halves.**

| half | what carries the authored affine |
| --- | --- |
| presentation | the node's `GlobalTransform`, the whole authored matrix, decomposed by nothing, and **no `Transform` on the node** |
| collision | the collider's **shape**: the authored collision geometry's own points pushed through the authored linear map, with the pose reduced to the authored translation and an identity rotation/scale |

Nothing is approximated and no affine is refused for being sheared. Both halves
read the same authored matrix through the same conversion
(`cs_app::world::canonical_matrix`), so F18 non-negotiable behavior 1 — *visual
and collision meshes share provenance and coordinate conversion* — stays
structural. The refusal that is left is narrower and named: an authored
component that does not survive the runtime's f32
(`AffinePlacementError::NotRepresentable`), a linear map with determinant zero
(`CollapsesSpace` — the object has no volume, so no collider of it exists), and
collision geometry the physics library cannot turn into a solid
(`UnbuildableCollision`). It still happens **before the first entity is
spawned**, so a refused definition leaves the app exactly as it was.

### The two alternatives, and why not

* **Collision follows the affine, presentation is refused** (asymmetric). This
  is invisible collision: a wall a body stops against and the player never sees.
  Rejected on fidelity grounds alone; no measurement was needed to reject it.
* **Both must be representable** (the F18-A contract, unchanged). The premise
  this task was filed on was that the *presented* half also needs the
  decomposition, so the bake would be half a feature. **That premise is wrong,
  and measured**: a `GlobalTransform` is an arbitrary affine and Bevy does not
  touch it unless the same entity has a `Transform`. The reason the refusal
  looked necessary was that `spawn_world` put a `Transform` on every visual
  entity.

### What it does to F18 non-negotiable behavior 1

The task asked what happens if the answer is "bake the matrix into the mesh for
the collider". With this design the collision mesh is **not a second asset**:

* the **same stored vertices** are used on both sides — there is no second file,
  no second upload, no second fingerprint and no second conversion function;
* the **same authored linear map** is applied to them, only in a different
  numeric domain (the render affine versus the collision shape);
* the derived parry shape is a *runtime collider object*, not content. Its
  representation differs from the presented mesh in ways that are already
  measured as exact, not simplifying: a box becomes the convex polyhedron whose
  eight points are the box's eight corners under the map (a parallelepiped is
  convex, so the hull of those eight points **is** the box), and a triangle mesh
  keeps every stored triangle.

So the "different verified simplification" allowance of behavior 1 is not even
used: the simplification is **none**, and the test asserts the triangle count and
the eight corners rather than a count that was allowed to shrink.

The one place a second asset *would* be needed is the mesh path, and it is
therefore the one combination this stage refuses:
`AffinePlacementError::ShearedMeshUndecided` (see "What is still unknown").

## Files and the one observable failure (the slice plan)

* `crates/cs_app/src/world/affine.rs` (new): `AffinePlacement`
  (`Trs`/`Sheared`), `AffinePlacementError`, `AffinePlacement::of` (the one
  classification), `AffinePlacement::presentation`, `::collider_pose`,
  `::bake`, the free functions `shear_residual` and `bake_shape`, and the
  private `parallelepiped` that builds the eight mapped corners and their twelve
  triangles.
* `crates/cs_app/src/world/spawn.rs` (edited): `instance_transform` →
  `instance_placement`, which resolves **both** the placement and the collider
  geometry before anything is spawned; `PlannedCollider`; `WorldSpawnError::
  UnrepresentableTransform` → `UnplaceableAffine { object, source }`; the visual
  entity carries `GlobalTransform` only; the collider entity is built from
  `Collider::from(shape)` and the placement's pose.
* `crates/cs_app/src/world/mod.rs`, `crates/cs_app/src/world/fixture.rs`
  (wiring and one `world_mut` accessor only), `crates/cs_app/src/lib.rs`
  untouched.
* `crates/cs_app/tests/world/shear.rs` (new): the nine
  `accept_f18_b_shear_*` acceptance tests.
* `crates/cs_app/tests/world/spawn.rs` (edited): the F18-A refusal test now
  pins the **narrowed** refusal — see "What changed in an existing test" below.
* `crates/cs_app/tests/world/main.rs` (wiring: one `mod shear;`).
* This file.

**One observable failure:** the sheared panel is placed at a pose that is not its
own — the drawn object and the collider have drifted apart, or the collider is
an unsheared box at the panel's centre, so a body that should hit the panel's
leaning face flies through it. That is
`accept_f18_b_shear_the_collision_carries_the_authored_linear_map_in_its_shape`
(it fails when the bake is skipped or applied with the wrong map) and
`..._the_baked_solid_blocks_inside_the_shear_and_outside_it` (it fails when the
collision is the unsheared box).

## Measured behavior on the pinned pair

`bevy 0.19.1` / `avian3d 0.7.0` / `parry3d 0.27.0`, `SubstepCount(1)`, 120 Hz
fixed rate, gravity zero. Fixture panel: half extents 0.5 × 1.5 × 0.5 m, authored
linear map `[[1, 0.5, 0], [0, 1, 0], [0, 0, 1]]` (a shear of 0.5 per meter of
`+y`), centred at `(0, 1.5, 0)`. All of it is synthetic
(`Origin::SyntheticFixture`).

* **The decomposition F18-A used still decides what F18-A placed.** Measured
  with `Mat4::to_scale_rotation_translation`: identity, pure scale and a
  **mirror** all round-trip exactly (`scale = (-1, 1, 1)` for the mirror, so a
  mirror survives as a negative scale in the pose); the shear does not, with a
  maximum component deviation of `2.5e-1`. The `Trs` test is byte-for-byte the
  one F18-A used, so nothing F18-A placed changed.
* **A singular map is what F18-A was really refusing.** Two determinant-zero
  shapes were measured: a flat axis and a zero scale axis. Both fail the
  round-trip with `maxdiff = 0` **and a NaN rotation** — the decomposition
  produces a NaN quaternion, which never compares equal. F18-A therefore
  refused them, but could only say "unrepresentable". They are now refused as
  `CollapsesSpace`, which is what they are: no volume, no collider.
* **The presentation keeps the whole affine, and a `Transform` would destroy
  it.** Two nodes holding the same authored `GlobalTransform` (a shear plus a
  translation), one with a `Transform` and one without, stepped through five
  `App::update`s: the node with **only** `GlobalTransform` still carries the
  matrix bit-identically, including `y_axis = (0.5, 1, 0)`; the node with a
  `Transform` comes back with `matrix3 = identity` and the translation
  untouched — the shear silently replaced by nothing. This is why the visual
  entity no longer has a `Transform`, and why `accept_f18_b_shear_the_presentation_carries_the_whole_authored_affine`
  fails under that mutation.
* **Avian's physics pose cannot hold a shear.** Read in the pinned source,
  `avian3d-0.7.0/src/physics_transform/mod.rs::transform_to_position` (the
  system behind `PhysicsTransformSystems::TransformToPosition`) queries
  `&GlobalTransform` and writes `Position`/`Rotation` from
  `global_transform.compute_transform()` — a translation/rotation/scale
  decomposition. `transform_to_collider_scale` does the same for the collider's
  scale. So a shear in a collider's *pose* is dropped before the broad phase
  sees it, which is why the linear map goes into the shape.
* **The bake is exact, and exactness is not a tolerance.** The eight mapped
  corners come back bit-identical to `authored matrix × local corner`, with six
  faces of four vertices each. Measured over scales `1e-4` to `1e6` and for
  boxes flat to `1e-3` on each axis in turn, `ConvexPolyhedron::from_convex_mesh`
  returns exactly the eight input points every time.
* **A hull is *not* exact here, so the hull constructor is not used.**
  `ConvexPolyhedron::from_convex_hull` **merges** points of a thin solid:
  measured, a 1 × 1 × 0.001 box comes back with fewer than eight distinct
  corners. A mesh-derived collider must not be hulled — that is exactly the
  "never close a traversable opening through convex-hull simplification" half of
  F18 behavior 1 — and for a box it would silently thin the wall.
* **The built solid's face normals are the authored image's, and the input
  winding does not decide them.** `from_convex_mesh` merges each coplanar quad
  and orients the merged face from the polygon's own geometry. Measured: with
  every triangle of the input reversed the six normals are bit-identical, and a
  linear map that *mirrors* (`det < 0`) produces the same six normals over its
  own mirrored corner set. The test therefore asserts the six normals against
  `±(g₂ × g₃)`, `±(g₃ × g₁)`, `±(g₁ × g₂)` computed from the record's own map,
  and a mirrored shear is **placed**, not refused.
* **A baked box is still a support map, so swept CCD still holds a body.**
  Measured: a `SweptCcd` probe with `SpeculativeMargin::ZERO` at 400 m/s into
  the sheared panel's slanted `+x` face is clamped at `x = -0.617`, which is
  exactly the face's position (its near face is at `x = -0.625` on that line,
  computed from the record's own map) minus the probe's half extent. This is the
  opposite of a `TrimeshFromMesh` collider, which parry cannot shape-cast
  against at all (F18-B's engine limitation, task #420) — so a sheared **box**
  does not inherit that gap.
* **A mirror *alone* still goes down the pose.** `AffinePlacement::of` returns
  `Trs` for it with `scale.x < 0`, so the negative scale is in the entity's
  transform, exactly as F18-A placed it.

## Test sensitivity (mutation matrix)

Every mutation below was applied, `cargo test -p cs_app --test world --
accept_f18_b_shear_` was run, and the source was restored. All nine tests pass
unmutated.

| mutation | tests that failed |
| --- | --- |
| put a `Transform` back on the presentation entity | `..._the_presentation_carries_the_whole_authored_affine` |
| bake with `Mat3::IDENTITY` (the shear never reaches the geometry) | 4: `..._the_collision_carries_...`, `..._blocks_inside_the_shear...`, `..._a_mirrored_shear_is_placed...`, `..._the_authored_box_is_the_shape_the_bake_starts_from` |
| refuse every non-TRS affine again (the F18-A contract) | all 9 |
| accept a determinant-zero map (place a zero-volume object) | `..._only_a_map_with_no_exact_placement_is_still_refused` |
| place a sheared mesh object too (drop the second-asset refusal) | `..._a_sheared_mesh_object_is_refused...` |
| drop the authored translation from the collider pose | 2: `..._the_collision_carries_...`, `..._blocks_inside_the_shear...` |
| never bake (`Sheared` returns the authored primitive untouched) | the same 4 as the identity bake |
| the mesh bake drops one triangle | `..._the_same_bake_keeps_every_stored_mesh_triangle` |
| reverse every triangle of the baked box | none — a **semantic no-op**, recorded as such |
| build the plan inside the spawn loop instead of before it | none — a **semantic no-op**, recorded as such |

Two rows deserve a word. *Reversing the winding* is a no-op because the physics
library derives each merged face's orientation from the corner set, not from the
input winding (measured above); the orientation claim is pinned by the
face-normal assertion instead. *Building the plan inside the loop instead of
before it* was also a no-op, because the loop still completes before any
`world.spawn`; the atomicity property is pinned by
`accept_f18_a_spawn_refuses_a_matrix_no_runtime_transform_can_hold_before_spawning_anything`,
which F18-A's own matrix already showed to be sensitive to it.

## What changed in an existing test

`accept_f18_a_spawn_refuses_a_matrix_no_runtime_transform_can_hold_before_spawning_anything`
kept its name, its `accept_f18_a_` prefix and its observable-failure rationale,
but its fixture changed from *sheared* to *determinant-zero*. The contract it
pins is unchanged and still discriminating — a refusal happens before the first
entity exists, it names the offending object, and nothing is left behind — but
"a matrix the runtime cannot hold" is no longer the trigger, because a shear is
now held exactly. It now also asserts the *reason*
(`AffinePlacementError::CollapsesSpace`), which the old test could not: F18-A's
`UnrepresentableTransform` carried no reason, and a determinant-zero matrix only
reached it by accident, through the NaN rotation that makes the round-trip
comparison fail.

No other existing test changed, and all fourteen `accept_f18_a_*` tests pass
unmodified.

## What is still unknown, and what it gates

* **Whether any retail world object has a sheared authored matrix is not
  known, and could not be measured here.** F18-B records that how the original
  stores world geometry per sector, how an object instance is identified, and
  what per-object collision role it stores are all unmeasured; no world importer
  exists, so there is nothing to census. This stage therefore decides on
  *engine-capability* grounds, which are measured, and gates the *occurrence*
  question on **F18-D** (`### F18-D: Audit all original world variants and
  stunt-critical openings`, `gpu` + `retail`): its world-variant audit is where
  the count of sheared authored matrices belongs, with a source span and an
  installation hash. Until then the engine is ready for a sheared object and no
  fidelity claim is made that one exists.
* **A sheared *mesh* object is refused, and that is a decision this stage does
  not own.** F18-B landed while this task was in flight, and its mesh path
  derives the collider from the presented `Mesh3d` handle
  (`ColliderConstructor::TrimeshFromMesh`), so a sheared mesh object can only be
  placed by baking the authored linear map into a **second, derived** upload.
  That upload's fingerprint would no longer be the authored one, which is exactly
  the one-asset provenance claim `spawn::MeshReference` carries and F18-B's
  import test asserts. So `AffinePlacementError::ShearedMeshUndecided` refuses
  the combination **by name** instead of placing it from a derived asset the
  report would misattribute. Resolving task: **F18-B**'s mesh policy together
  with F18-D's census; affected content: any retail world object whose collision
  is mesh-derived and whose authored matrix is sheared. Note that the refusal is
  a whole-build refusal, exactly as F18-A's was, so it is visible and never a
  half-placed world — but it is a **narrower** one than F18-A's, because a
  sheared cuboid object is now placed.
* **A non-finite runtime affine is refused, but the record cannot produce one
  through `CanonicalTransform::try_new`**, which validates finiteness in f64. The
  reachable half of `NotRepresentable` is an f64 value too large for f32; it is
  named rather than tested with a synthetic record, because building one would
  mean writing a `CanonicalTransform` through a path its constructor refuses.
* **The mesh bake's winding after a mirror** is measured to be orientation-free
  for the convex-hull path (`from_convex_mesh`), but a `TriMesh` bake under a
  mirroring map is *not* covered: `TriMesh::new` keeps the caller's triangle
  winding, and a mirror reverses it. `bake_shape` therefore places a mirrored
  **box** exactly, and a mirrored **mesh** would arrive with inward-facing
  triangles — a fact F18-B's mesh path must decide before it adopts the call for
  a mirroring object. Affected content: a retail world object whose collision is
  mesh-derived and whose authored matrix both mirrors and shears. Resolving
  task: **F18-B** (#86) together with the F18-D census; recorded here rather
  than guessed.

## Designed vocabulary, not original data

Designed here: `AffinePlacement` and its two variants, the three
`AffinePlacementError` reasons and their labels, `shear_residual`'s
normalisation, the split of the authored affine between the presentation
affine and the collision shape, and the decision to perform **no** simplification
in the bake. None of it is claimed to be the original's vocabulary or behaviour,
and the fixture panel is synthetic content.

## Evidence

Ordinary build/test only; no `CS_GAME_DIR` read and no evidence report is
required for this task. Commands run locally:

```sh
cargo fmt --all -- --check                                        # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # exit 0
cargo test --workspace --locked                                   # exit 0
cargo test --workspace --locked -- accept_f18_b_shear_ --include-ignored
#   9 tests run, 9 passed (crates/cs_app/tests/world)
cargo test --workspace --locked -- accept_f18_a_ --include-ignored
#   14 tests run, 14 passed (crates/cs_app/tests/world), plus F18-B's own 17
```

## Sources

No external sources were consulted. The record shapes follow
`docs/contracts/IDENTITY-CONTENT.md`; the placement contract this task narrows
is F18-A's (`crates/cs_app/src/world/spawn.rs`,
`docs/findings/2026-09-30-f18-a-world-instances-sectors-and-collision-roles.md`)
and the presentation half is the same decision F11 already made for scene nodes
(`crates/cs_app/src/scene.rs::NodeVisualTransform`, whose documentation states
that the canonical matrix is an arbitrary affine and therefore maps to a
`GlobalTransform`, never to a `Transform`). Every Bevy, Avian and parry statement
above was read from the pinned sources in the local cargo registry and then
*measured* by running the fixture.
