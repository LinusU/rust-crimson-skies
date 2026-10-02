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
* `crates/cs_app/tests/world/{main,common,sweep,records,spawn}.rs` (new): the fourteen
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

`WorldDefinition::record_fingerprint` is the canonical digest of what a
definition *says* — world id, declared boundary, sectors and objects, each
hashed in id order — returned as a `ContentHash` like every other fingerprint
in the workspace, so it can be pinned later without depending on an
implementation-defined hash.

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
* **A swept body is stopped by a sensor volume; a non-swept one is not** —
  *measured 2026-09-30, and resolved by task #401 on 2026-10-02; the numbers and
  the layout below are what was measured then, and are kept because they are the
  reason the layout changed.* Measured on the trigger volume (`trigger.sensor`,
  8 m along the flight axis, x ∈ [6, 14]): with `SweptCcd` on the probe, the
  probe ended at `x = 19.0835` instead of `21.5` — a loss of 2.416 m, which is
  exactly the distance from its previous sample (`x = 4.8333`) to the sensor's
  near face (`x = 5.75` including the probe's half extent). Reading
  `avian3d-0.7.0/src/dynamics/ccd/mod.rs::solve_swept_ccd` explains it: the
  swept query stops a body at the first time of impact against **any**
  collider its path reaches, with no `Sensor` filter. The same probe spawned
  *without* `SweptCcd` (`spawn_discrete_probe`) crosses the volume with
  drift < 0.01 m, unchanged velocity, and a `CollisionStart` naming
  `trigger.sensor`. Sensors have no contact response of their own — it is the
  CCD that holds the body — so the role's "never blocks motion" is measured
  with a non-swept body, and the swept/sensor interaction was recorded as a
  limitation (below).
  **Resolved:** a `WorldCollisionRole::Sensor` object is now spawned on an
  entity with **no rigid body**, which `SweptCcdBodyQuery` cannot resolve, so
  the same swept probe crosses the volume with free travel on all fifteen ticks
  (50.00 m, drift < 0.01 m) **and** the crossing is still reported once. See
  `docs/findings/2026-10-02-t401-trigger-volume-and-swept-ccd.md`.

## Test sensitivity (mutation matrix)

Every mutation below was applied, the selector run, and the source restored.
All fourteen tests pass unmutated.

| mutation | tests that failed |
| --- | --- |
| remove `SweptCcd` from the probe | `..._a_probe_aimed_at_an_arch_leg_is_stopped_by_it` |
| never spawn a collider (`if true` in place of `role.creates_collider()`) | `..._unresolved_instances...`, `..._visual_and_collision_instances_agree...`, `..._water_is_a_bounded_patch...`, `..._a_probe_aimed...` |
| offset every collider by `+0.6 m` in `x` | `..._visual_and_collision_instances_agree...`, `..._water_is_a_bounded_patch...` |
| `WorldContacts::record` never appends | `..._a_probe_aimed_at_an_arch_leg_is_stopped_by_it` |
| default an unknown collision role to `Solid` | `..._unresolved_instances_are_reported_instead_of_guessed` |
| spawn as it walks instead of refusing first (review mutation) | `..._spawn_refuses_a_matrix_no_runtime_transform_can_hold_before_spawning_anything` |
| never insert the Avian `Sensor` marker (review mutation) | `..._every_collision_role_decides_what_is_spawned`, `..._a_sensor_reports_the_probe_and_never_blocks_it` |
| give role `None` a collider anyway (review mutation) | `..._unresolved_instances_are_reported_instead_of_guessed`, `..._every_collision_role_decides_what_is_spawned`, `..._visual_and_collision_instances_agree...` |
| hash the record in supplied order instead of id order (review mutation) | `..._record_fingerprint_is_order_independent_and_tracks_the_record` |
| drop the boundary from the fingerprint (review mutation) | `..._record_fingerprint_is_order_independent_and_tracks_the_record` |
| derive the Avian filter as empty (`\|= 0`) (review mutation) | `..._spawned_bodies_carry_the_designed_collision_layers`, `..._a_probe_aimed_at_an_arch_leg_is_stopped_by_it`, `..._a_sensor_reports_the_probe_and_never_blocks_it` |

The four record-level tests
(`..._world_definition_refuses_duplicate_ids_and_dangling_sector_refs`,
`..._each_world_instance_states_its_variant_population_and_damage`,
`..._sector_membership_and_residency_are_explicit_records`,
`..._unresolved_surface_roles_are_listed_not_defaulted`) call
`cs_content::world` directly and fail on their own refusals.

## Designed vocabulary, not original data

Every id, key grammar, sector, role name, role value, boundary field,
`WorldInstance` field, the nine fixture objects and the arch's dimensions are
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
  scale (a shear, or a mirror no quaternion holds) is refused **before the
  first entity exists**: `spawn_world` returns
  `WorldSpawnError::UnrepresentableTransform` with the app untouched, rather
  than approximating a pose or leaving a half-built world the caller has no
  `SpawnedWorld` to ask about. Affected content: any retail object whose
  authored matrix is sheared or mirrored, discovered by **F18-B** when it
  imports real geometry; `spawn_discrete`/mesh colliders, which can follow the
  render path's full affine, are **F18-B**'s answer to this for real geometry.
* ~~**Swept CCD stops a body at a sensor volume** (measured above). Affected
  content: every world object with `WorldCollisionRole::Sensor` —
  `trigger.sensor` in this fixture, and any retail trigger or objective volume
  **F18-B** imports or **F18-C** binds to mission overlays — when the body
  reaching it carries `SweptCcd` (F23's aircraft probe/airframes). Resolving
  task: **#401** (filed by this review).~~ **Resolved by #401 (2026-10-02):** a
  `Sensor` object is spawned on an entity with no rigid body, which Avian's
  `SweptCcdBodyQuery` cannot resolve, so a swept body crosses a trigger volume
  untouched and the crossing is still reported once. What replaced it as a
  limitation is the *report* boundary, not the hold: a trigger volume's report is
  a discrete overlap, so a body whose tick outruns the volume's thickness is not
  reported of it, and a **mesh-derived** volume also goes quiet when a body lands
  deep inside it. Affected content: a retail trigger volume thinner than one
  tick of travel at the reaching body's speed. Resolving task: a swept crossing
  report (F39's trigger semantics). See
  `docs/findings/2026-10-02-t401-trigger-volume-and-swept-ccd.md`.
* `WorldContacts` records only pairs with **exactly one** world collider: an
  actor touching world geometry. Two static world objects resting against each
  other is authoring, not gameplay, and is deliberately not logged.
* Streaming/unload-reload (**AC02**) and mission overlays (**AC03**) are not
  implemented; `Sector` carries an extent and a stable id but no visibility
  metadata, because the original's is unmeasured. **F18-B** (load and reload)
  and **F18-C** (overlays, streaming) own those.
* No retail world group was visited: **AC04** needs `gpu` + `retail`
  (**F18-D**). Nothing in this stage is `verified_original`.

## What the review changed (2026-09-30, second session)

The reviewer read the whole branch against `specs/F18-...md` and fixed four
problems rather than handing them back:

1. **`spawn_world` built the world as it walked.** An unrepresentable matrix
   returned an error *after* the objects before it had already been spawned,
   so the caller was left with a half-built app and no `SpawnedWorld` to ask
   what it got — contradicting the function's own "the whole build stops"
   contract. It now decomposes every instance first and refuses before the
   plugin or any entity exists. `SkipReason::UnrepresentableTransform`,
   which that error path made unreachable, was removed rather than left
   documenting a skip that never happens.
2. **Two of the three declared collision roles had no test.** The fixture
   carried only `Solid` and explicit unknowns, so "always spawn a collider"
   and "never mark a sensor" both passed the whole suite. The arch world now
   also carries `banner.non_colliding` (role `None`) and `trigger.sensor`
   (role `Sensor`), and `tests/world/spawn.rs` asserts what each role
   produces. Because Avian's swept CCD holds a body at a sensor's face
   (measured above), a `spawn_discrete_probe` (same body, no `SweptCcd`,
   same `SpeculativeMargin::ZERO`) is what proves "reports an overlap and
   never blocks motion".
3. **`record_fingerprint` contradicted its own documentation.** It hashed in
   supplied order while promising insertion-order independence, and used
   `DefaultHasher`, whose output is explicitly unspecified across Rust
   releases — unusable for a fingerprint a later stage would pin (as F17
   pins its golden fingerprints). It now builds a canonical byte encoding
   (version tag, id order, boundary included) and returns `sha256` as a
   `ContentHash`.
4. **The refusal test initially passed even against the broken build**: with
   only one object there was nothing to leave half-spawned. It now puts a
   valid object *before* the sheared one, which is what makes mutation 6
   below fail.
5. **`probe_layers()` and `static_world_layers()` promised a test that did
   not exist.** Both were exported "so a test can assert the probe and the
   world were built from the same designed matrix", but nothing called them,
   so `avian_layers` — the derivation from
   `CollisionLayer::designed_collides_with` to Avian's membership/filter
   masks — had no coverage of its own. It has now:
   `accept_f18_a_spawned_bodies_carry_the_designed_collision_layers` pins the
   spawned components to the designed sets and asserts the two sets interact.

The first mutation matrix (five rows) is unchanged; the six review
mutations were applied, run and reverted the same way.

## Evidence

Ordinary build/test only; no `CS_GAME_DIR` read and no evidence report is
required for this stage. Commands run locally:

```sh
cargo fmt --all -- --check                                        # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # exit 0
cargo test --workspace --locked                                   # exit 0
cargo test --workspace --locked -- accept_f18_a_ --include-ignored
#   14 tests run, 14 passed (crates/cs_app/tests/world)
```

The fourteen acceptance tests are listed with their failure sensitivity in
the mutation matrix above.

## Sources

No external sources were consulted for this stage. The record shapes follow
`docs/contracts/IDENTITY-CONTENT.md` and the F11 precedent
(`crates/cs_content/src/scene.rs`, `docs/findings/2026-09-29-f11-a-node-hierarchy-bindings.md`);
the schedule and layer vocabulary follow
`docs/findings/2026-09-29-f23-a-avian-schedule-adapter-and-collision-layers.md`.
Avian/Bevy API facts above were read from the pinned sources in the local
cargo registry and then *measured* by running the fixture.
