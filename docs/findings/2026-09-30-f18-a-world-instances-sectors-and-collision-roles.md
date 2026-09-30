# F18-A: world instances, sectors and collision roles

Date: 2026-09-30. Task: F18-A "Define world instances, sectors and collision roles"
(`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`, section
`### F18-A`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Capabilities
used: ordinary build/test only — no `CS_GAME_DIR` read, no evidence report
required.

## Files and the one observable failure (the slice plan)

* `crates/cs_content/src/world.rs` (new): the Bevy-free typed contract —
  `WorldId` (wraps `ContentKind::World`, the namespace `IDENTITY-CONTENT`
  already reserves for "a world group or one variant of a world"), the
  subordinate `SectorId`/`WorldObjectId` keys, `Aabb`, `Sector`,
  `SurfaceRole`, `WorldCollisionRole`, `WorldCollisionShape`,
  `WorldBoundary`, `WorldObjectInstance`, the validated `WorldDefinition`,
  and the per-load `WorldInstance` + `WorldPopulation` records.
* `crates/cs_app/src/world/spawn.rs` (new): `canonical_matrix`,
  `instance_transform`, `avian_layers`, and `spawn_world` — the one
  conversion from a `WorldDefinition` into Bevy/Avian entities.
* `crates/cs_app/src/world/contacts.rs` (new): `WorldObjectBinding`,
  `WorldVisual`, `WorldColliderInstance`, `WorldContacts` +
  `record_world_contacts`, and the `WorldPlugin` that installs them.
* `crates/cs_app/src/world/fixture.rs` (new): the synthetic arch
  `arch_world()`, `spawn_swept_probe`, and the headless `WorldFixture`
  harness.
* `crates/cs_app/src/world/mod.rs` (new): module docs and re-exports.
* `crates/cs_app/src/lib.rs`, `crates/cs_content/src/lib.rs` (wiring only):
  module declarations and docs.
* `crates/cs_app/tests/world/{main,common,sweep,records}.rs` (new): the nine
  `accept_f18_a_*` acceptance tests.
* This file.

**One observable failure:** a body flies at the arch leg and the contact log
stays empty, because nothing the record declared was spawned as collision —
or because a collider was built at a pose the record never had. The stage's
minimum scenario is exactly that: `accept_f18_a_a_probe_aimed_at_an_arch_leg_is_stopped_by_it`
fails with an empty contact log when colliders are not spawned, and
`accept_f18_a_swept_body_flies_through_the_narrow_arch_at_high_speed` fails
with a contact named when a collider is put where the record draws the hole.

## What the stage defines

`cs_content::world` carries no Bevy or Avian dependency (contract rule:
`cs_content` must never depend on them). Three roles sit on one object record
and stay separate:

| field | answers | type |
| --- | --- | --- |
| `mesh` | what it draws | `Resolved<ContentId>` |
| `transform` | where it is | `CanonicalTransform` (the same record the visual and the collider convert from) |
| `collision` | does it block, report, or neither | `Resolved<WorldCollisionRole>`: `None` \| `Solid` \| `Sensor` |
| `shape` | what geometry bounds it | `Resolved<WorldCollisionShape>`: `Cuboid` \| `FromMesh` |
| `surface` | which gameplay surface rule a contact follows | `Resolved<SurfaceRole>`: `Ground` \| `Water` |

Sector membership is stored **on the object**, not on the sector: an object
may belong to several sectors, and an object that names none is *resident*
(it never streams away) rather than an authoring error. `WorldDefinition::try_new`
refuses duplicate ids and dangling sector references once, at construction.

`WorldInstance` is the load record of F18 non-negotiable behavior 5: one
mission's variant, object population and initial damage, with
`validate_against` refusing an id the definition does not declare and refusing
a check against a different definition. Nothing about a load lives outside
that struct, so "leftovers from the last run" has nowhere to live.

`WorldBoundary` carries floor/ceiling/lateral limits that are all optional;
`WorldBoundary::default()` declares **no rule at all**, because behavior 4
forbids an arbitrary invisible wall in fidelity mode.

## Measured behavior on the pinned pair

`bevy 0.19.1` / `avian3d 0.7.0`, `SubstepCount(1)`, 120 Hz fixed rate, gravity
zero. Probe: 0.5 m box, 400 m/s = 3.33 m per tick, start `x = -28.5`,
flight height `y = 1.5`, arch legs 1 m thick along `x`.

* **The opening is not closed.** The probe through `z = 0` travels
  `start + speed * ticks * dt` to within 0.01 m, keeps its velocity, and the
  contact log is empty.
* **The leg is not open.** The same probe at `z = 1.5` is clamped to
  `x = -0.7497` — the wall's near face is `-0.5` and the probe's half extent
  is `0.25`, i.e. it stops *exactly* at the surface — on the crossing tick,
  a `CollisionStart` naming `arch.leg_right` is logged, and it never gets past
  `x = 1.0`.
* **Discrete sampling really does straddle the wall.** Tick positions are
  `x = -1.8333` then `x = +1.5000`: consecutive samples sit on either side of
  the 1 m leg with a gap larger than probe-plus-wall, so a non-swept test
  would step over it. The test asserts this precondition itself.
* **`SweptCcd` alone is what clamps it, once speculative collision is off.**
  With Avian's *default* `SpeculativeMargin`, removing `SweptCcd` from the
  probe changes nothing: the identical trace is produced and the leg is still
  hit, so a test over the defaults cannot tell a swept sweep from speculative
  collision. With `SpeculativeMargin::ZERO` on the probe (the fixture's
  setting), removing `SweptCcd` makes the probe tunnel from `x = -1.8333`
  straight to `x = +1.5000` and beyond with an empty contact log, and
  restoring it clamps to `-0.7497`. The fixture therefore isolates the swept
  path instead of depending on whichever default the pinned version ships.
* **The broad-phase bound is inflated by a contact margin of 0.005 m per
  side.** Measured on the water patch: `ColliderAabb` reports
  `(-5.469102, -0.055, -21.469102)..(5.469102, 0.155, -10.530898)` where the
  authored box gives `(-5.464102, -0.05, -21.464102)..(5.464102, 0.15, -10.535898)`.
  The centre matches exactly, the size does not — so the acceptance tests
  compare box **size** against `Collider::shape_scaled()` (the geometry the
  narrow phase uses) and only the **centre** against `ColliderAabb`.
* **Rotation is honoured.** The water patch authored at 4×0.1×4 m rotated 30°
  about `+y` lands with a broad-phase half-width of 5.469 m versus the
  reference 5.464 m, and the collider's own box is still 4×0.1×4 m.

## Test sensitivity (mutation matrix)

Every mutation below was applied, the selector run, and the source restored.
All nine tests pass unmutated.

| mutation | tests that failed |
| --- | --- |
| remove `SweptCcd` from the probe | `..._a_probe_aimed_at_an_arch_leg_is_stopped_by_it` |
| never spawn a collider (`if true` in place of `role.creates_collider()`) | `..._unresolved_instances...`, `..._visual_and_collision_instances_agree...`, `..._water_is_a_bounded_patch...`, `..._a_probe_aimed...` |
| offset every collider by `+0.6 m` in `x` | `..._visual_and_collision_instances_agree...`, `..._water_is_a_bounded_patch...` |
| `WorldContacts::record` never appends | `..._a_probe_aimed_at_an_arch_leg_is_stopped_by_it` |
| default an unknown collision role to `Solid` | `..._unresolved_instances_are_reported_instead_of_guessed` |

The two record-level tests
(`..._world_definition_refuses_duplicate_ids_and_dangling_sector_refs`,
`..._each_world_instance_states_its_variant_population_and_damage`,
`..._sector_membership_and_residency_are_explicit_records`,
`..._unresolved_surface_roles_are_listed_not_defaulted`) call
`cs_content::world` directly and fail on their own refusals.

## Designed vocabulary, not original data

Every id, key grammar, sector, role name, role value, boundary field,
`WorldInstance` field, the seven fixture objects and the arch's dimensions are
**newly authored engine contract** or **synthetic fixture content**
(`Origin::SyntheticFixture`). The following are **unknown** and are not
guessed here:

* **Whether the original stores world geometry per sector at all**, and how a
  sector or an object instance is identified in the source. The spec requires
  sector/visibility metadata; its storage is unmeasured. Resolved by the
  import/audit stages **F18-B** and **F18-D** (research lead already recorded
  in `docs/research/FINDINGS.md`: "Complete CS GameZ variants, material flags
  and collision roles").
* **Which gameplay surface classes the original distinguishes** (is water a
  material flag, a volume, or something else), and what rule each carries.
  Declared here as the two roles the spec names explicitly; measured by
  **F18-D**.
* **Which authored geometry is solid, sensor or non-colliding** in the
  original. `WorldCollisionRole` is a designed three-value vocabulary; per
  object assignment arrives with evidence in **F18-B/D**.
* **The original's floor, ceiling and world-boundary rules.** The typed home
  exists (`WorldBoundary`) and currently documents "no rule" rather than a
  wall; **F18-B/D** reads the real values into it.
* **Whether retail aircraft bodies run continuous detection.** The fixture
  proves the *engine's* swept path works, not that the original needed it
  (**F23-D**, **F18-D**).
* **`ContentKind` has no `sector` or `world_object` namespace.** Adding one
  means editing `crates/cs_types/src/content.rs`, which is outside this task's
  owner paths, so sector and object ids are validated subordinate keys *inside*
  one world rather than catalog `ContentId`s. If a later stage needs them as
  catalog elements (one row per sector, per the `IDENTITY-CONTENT` catalog
  collections), that is a `cs_types` change and must be scheduled explicitly.

## Known limitations that gate later stages (not silently dropped)

* `WorldCollisionShape::FromMesh` is **declared and never spawned** here.
  `spawn_world` reports such an instance as
  `SkipReason::MeshColliderDeferred`, never as collided. Building mesh-derived
  static collision is **F18-B**, which the owner ruling of 2026-09-28 also
  gates on Avian collider-from-mesh over a real asset stack (#333).
* An authored matrix that cannot be decomposed into translation, rotation and
  scale (a shear, or a mirror no quaternion holds) produces **no collider and
  no visual**: `spawn_world` stops with
  `WorldSpawnError::UnrepresentableTransform` rather than approximating a
  pose. Mesh colliders, which can follow the render path's full affine, are
  **F18-B**'s answer to this for real geometry.
* `WorldContacts` records only pairs with **exactly one** world collider: an
  actor touching world geometry. Two static world objects resting against each
  other is authoring, not gameplay, and is deliberately not logged.
* Streaming/unload-reload (**AC02**) and mission overlays (**AC03**) are not
  implemented; `Sector` carries an extent and a stable id but no visibility
  metadata, because the original's is unmeasured. **F18-B** (load and reload)
  and **F18-C** (overlays, streaming) own those.
* No retail world group was visited: **AC04** needs `gpu` + `retail`
  (**F18-D**). Nothing in this stage is `verified_original`.

## Evidence

Ordinary build/test only; no `CS_GAME_DIR` read and no evidence report is
required for this stage. Commands run locally:

```sh
cargo fmt --all -- --check                                        # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # exit 0
cargo test --workspace --locked                                   # exit 0
cargo test --workspace --locked -- accept_f18_a_ --include-ignored
#   9 tests run, 9 passed (crates/cs_app/tests/world)
```

The nine acceptance tests are listed with their failure sensitivity in the
mutation matrix above.

## Sources

No external sources were consulted for this stage. The record shapes follow
`docs/contracts/IDENTITY-CONTENT.md` and the F11 precedent
(`crates/cs_content/src/scene.rs`, `docs/findings/2026-09-29-f11-a-node-hierarchy-bindings.md`);
the schedule and layer vocabulary follow
`docs/findings/2026-09-29-f23-a-avian-schedule-adapter-and-collision-layers.md`.
Avian/Bevy API facts above were read from the pinned sources in the local
cargo registry and then *measured* by running the fixture.
