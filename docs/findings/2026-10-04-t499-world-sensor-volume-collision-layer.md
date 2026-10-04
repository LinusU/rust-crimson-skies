# T499: a world sensor volume declares the collision layer the swept preflight classifies it by

Date: 2026-10-04. Task: #499 "Give a world-authored sensor volume a collision
layer the swept preflight can classify" (`F18-sensor-volume-collision-layer`).
Changed: `crates/cs_app/src/world/spawn.rs` (F18's world spawn) and the
measurement composition in `crates/cs_app/src/world/fixture.rs`. The
classification code is F23-C's `crates/cs_app/src/physics/preflight.rs`, which
this task does **not** change — see "Why `classify_hit` is untouched".
Contracts: `docs/contracts/FLIGHT-PHYSICS.md` ("Collision and ballistic tests").
Source task: **#415**, which found the gap and filed it as its second composition
gap (`docs/findings/2026-10-02-t415-spawn-tick-trigger-crossing.md`, "Two
composition gaps this task found and did not fix"). Sibling: **#498**, which
built the ordinary-flight half of the same crossing rule
(`docs/findings/2026-10-03-t498-swept-trigger-crossing.md`).

Capabilities used: ordinary build/test only. No `CS_GAME_DIR` read, no evidence
report required. This stage can award at most **checked**.

## The defect, measured before anything was changed

`classify_hit` in the preflight classifies a cast hit from exactly two
components on the hit entity: its `BodyLayer` and its `Sensor` marker. It
returns `None` when the layer is missing, and the two callers disagree about
what that means:

* the **solid** cast's predicate is
  `classify_hit(...).is_none_or(|kind| kind == ContactKind::SolidContact)` — an
  unclassifiable hit is **solid**;
* the **sensor** cast's predicate is
  `classify_hit(...).is_some_and(|kind| kind == ContactKind::SensorOverlap)` — an
  unclassifiable hit is **not** a crossing, so nothing is recorded.

`spawn_cuboid_collider`, `spawn_mesh_trigger_volume` and `spawn_mesh_body` all
inserted `WorldColliderInstance`, `CollisionLayers` and (for a trigger) the
`Sensor` marker, and **no `BodyLayer` at all**. A world trigger volume was
therefore the worst of both: it stopped the spawn it should have flown through,
and left no record of the crossing.

Measured through the production spawn (`spawn_world` on `arch_world`), the
production `spawn_body` for the projectile, and the production preflight, on the
pinned pair (`bevy 0.19.1` / `avian3d 0.7.0`, `SubstepCount(1)`, 120 Hz fixed,
gravity zero). Geometry: the arch world's `trigger.sensor` cuboid at
`[10, 1.5, -4]` with half extents `[4, 0.75, 0.75]`, so `z ∈ [-4.75, -3.25]`;
the flight is along `+z` through the volume's centre, i.e. along its **thin**
axis, so a body can cross the whole volume inside one tick. A 10 cm swept
`Projectile` spawns 0.4 ticks of travel short of the near face (F23-B's
first-tick hole), `SubstepCount(1)` as the world composition installs it:

| fired | travel/tick | `hit` | `clamped` | `stopped` | `vz` after the spawn tick | `passed` | crossings delivered |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 60 m/s | 0.50 m | `Some(volume)` | **true**, at 0.200 m | **true** | 60 → **0** | `None` | **0** |
| 600 m/s | 5.00 m | `Some(volume)` | **true**, at 2.000 m | **true** | 600 → **0** | `None` | **0** |

The body did not merely lose a tick: it **never crossed the volume at all**. It
ended the spawn tick at `z = -4.799` — the volume's near face at `z = -4.750`
plus the preflight's `SPAWN_CONTACT_OVERLAP_M` of 1 mm — and stayed there for
ticks 2 and 3, while its free-flight position at tick 3 would have been
`z = -3.500`: the projectile is spawned at `z = -5.000`
(`-4.750 - 0.4 × 0.500 - 0.050`, `arch_projectile`'s own arithmetic) and 60 m/s
at 120 Hz is 0.5 m of travel a tick, so `-5.000 + 3 × 0.500`. F23-C's
`accept_f23_c_preflight_never_stops_on_a_sensor` was therefore violated for the
world's own volumes, and #415's spawn-tick crossing record was empty for exactly
the "mission-overlay triggers F18-C binds" that its affected content names. (That
free position is 0.250 m *inside* the volume. It is arithmetic from the fixture's
own constants rather than a second measurement, and the measured pose above is
what pins the spawn at `-5.000`: `-5.000 + 0.200 + 0.001 = -4.799`.)

**One further observation, which matters for reading the contact numbers.** In
both cells the world's own contact log *did* name the volume, once, with the
projectile. That is not evidence the crossing was reported: the clamp put the
body on the volume's surface, and *that* is what the discrete narrow phase saw.
It is the artefact #401 and #498 both had to state — "the clamp only looked like
a report because it forced the body onto the surface" — reproduced here on the
spawn tick. **One contact report named the volume, and the body never entered
it.**

The solid arm in the same world, for contrast: a `Projectile` fired at
`arch.leg_right` from the same 0.4-tick offset clamped at 0.200 m and stopped,
ending at `x = -0.549` against the leg's near face at `x = -0.500`. World
geometry already stopped spawns — but **by absence**, because an unclassifiable
hit is solid, not because the matrix said so.

## The decision

> **A world collider declares the collision layer it was already given: the one
> layer in its engine membership, `CollisionLayer::StaticWorld`.** Every spawn
> bundle inserts it, whatever the record's role. The role stays a separate fact
> on the separate component that already carries it — the Avian `Sensor` marker —
> which is exactly what `classify_contact` reads for the shape class.

One answer per collider, on purpose. The world spawn has always put exactly this
layer in the membership mask (`static_world_membership()`), so making it the
declared layer too means a consumer never has to ask which of two layer
components is the real one, and there is no code path in which the declared
layer and the membership could disagree. `static_world_layer()` is now the one
place the value is written, and `static_world_membership()` derives from it.

**The interaction with `WorldCollisionRole::Sensor`, stated rather than
duplicated.** The content record's role is the authority on *what a volume does* —
it reports an overlap and never blocks — and it stays the only such authority.
It reaches the engine as the `Sensor` marker (F18-A, task #401) and it reaches
the report as `WorldColliderInstance::role()` (which #498's crossing pass and
`overlays::queue_overlay_triggers` both read). The new `BodyLayer` carries no
claim about sensor-ness at all: a `Solid` world object and a `Sensor` world
volume carry the **same** layer, and the pair resolves differently only because
`classify_contact` sees a sensor on one side of it. That is the existing rule in
`cs_sim::collision` — *"a sensor on either side makes an interacting pair a
`SensorOverlap`"* — applied rather than restated. Had the declared layer been
`CollisionLayer::Trigger` for a trigger volume, the role would have been
declared twice, in two vocabularies, and #415's stated worry would apply in a
new place: the F18-C path and the spawn-tick path inventing two ways to name one
volume.

`accept_t499_a_world_collider_declares_the_layer_the_sweep_classifies_it_by`
holds the correspondence: the declared layer's bit **is** the membership bit, and
the classification is computed from the two components `classify_hit` reads, for
all three spawn layouts (hand-built cuboid sensor, hand-built cuboid solid,
mesh-derived sensor, mesh-derived solid) and for the inert camera layer.

## The repaired side, measured the same way

Same geometry, same projectile, same composition; only the three `BodyLayer`
inserts added:

| fired | travel/tick | structural cell | `clamped` | `vz` after 3 ticks | `passed` | `passed_distance_m` | crossings |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 60 m/s | 0.50 m | ends **inside** the volume | false | 60.000 | `Some(volume)` | 0.200 m | 1, tick 1, `Entry` |
| 192 m/s | 1.60 m | one whole tick **inside** (centre `z = -3.840`, 0.59 m short of the far face) | false | 192.000 | `Some(volume)` | 0.640 m | 1, tick 1, `Entry` |
| 600 m/s | 5.00 m | **passes through entirely** | false | 600.000 | `Some(volume)` | 2.000 m | 1, tick 1, `Entry` |

The solid arm is unchanged to the last digit: `clamped: true`, `stopped: true`,
distance 0.200 m, ending at `x = -0.549` — now by the matrix's
`SolidContact` rather than by absence.

Two readings worth keeping:

* **The crossing distance is the spawn-hole offset, at every speed.**
  `0.200 / 0.500 = 0.640 / 1.600 = 2.000 / 5.000 = 0.4` — the exact ratio of
  the distance the body was spawned short of the volume, which is the geometric
  invariant and not a speed-dependent number. #415 pinned the same invariant for
  the session path with its own 2 cm volume; here it holds for the world's own
  1.5 m one, so the two producers agree on the reference the distance is
  measured from (the spawn pose).
* **The pass-through cell is the only trace, and that is the point.** Over all
  three ticks at 600 m/s the engine's discrete phase reports **nothing** — no
  sample of the body lands inside a 1.5 m volume it crosses in one tick — while
  the preflight's swept cast recorded the crossing at 2.000 m and the consumer
  delivered it once. Before this task that cell had no record anywhere: not a
  crossing, not an overlap, and a body stopped at the checkpoint instead. In the
  dwell cell the discrete stream does report the overlap, on **tick 2** — one
  tick later than the swept record's tick 1, the same ordering #415 pinned.

### Once-per-pair is the consumer's, and it is measured here too

Three ticks of flight re-read the same preflight record three times: the
consumer delivered **1** crossing and counted **2** refusals
(`TriggerCrossings::duplicates`). That is #415's ledger doing its work on a world
volume — a trigger that fired once per tick would be a mission objective that
fires at the frame rate.

The fixture also carries **both** producers, because `world_app()` installs
`WorldSweptCrossingPlugin` and the measurement adds the preflight on top of it.
So the single delivery is measured with #498's ordinary-flight pass running
against the same pair. It contributes no second entry, and **why is per cell, not
one rule** (measured from `SweptBodyTracks`' own counters after each tick):

| cell | first sight | later ticks | `entries` |
| --- | --- | --- | --- |
| 60 m/s, ends inside | registered on tick 1 with **0 casts**, `inside` = the volume | 2 casts, 2 touches, the body never leaves | **0** |
| 192 m/s, one tick inside | registered on tick 1 with **0 casts**, `inside` = the volume | 1 cast, 1 touch, and the exit half on tick 2 records nothing by rule | **0** |
| 600 m/s, passes through | registered on tick 1 with **0 casts**, `inside` = **empty** — the body is already 1.45 m *past* the far face | 2 casts, **0 touches**: its segments no longer reach the volume | **0** |

So in the first two cells the first-sight branch registers a body that
materialized *inside* a volume and casts nothing — an exit or a dwell, not an
entry, which is #498's stated rule — while in the pass-through cell it registers
a body already *past* the volume, and the segments it casts on ticks 2 and 3
start beyond the volume and therefore touch nothing. Either way the pair crosses
the ledger once, from the preflight alone. That is the "the two must be
consistent rather than each inventing a way to name a volume" requirement
measured rather than asserted.

`exits` reads **2** for the single 192 m/s exit transition, on a tick whose cast
reported 1 touch: the volume is both `inside` at the segment's start and
`touched` along it, and `sweep_volume_crossings`'s `involved` list is chained
from those sets rather than deduplicated, so one transition is walked twice. The
ledger is unaffected — the `(inside, ends inside) = (true, false)` branch is the
exit half and records nothing — but the counter's value is not the number of
transitions its doc comment says. That is #498's pass rather than this task's
repair, so it is filed as **#643** instead of being folded in here.

## Why `classify_hit` is untouched

The preflight is F23-C's path and its `classify_hit` is already the right
function: it reads the declared layer and the sensor marker and asks
`cs_sim::collision::classify_contact`. It had nothing to classify *with*. The
alternative repairs were all considered and rejected on measurement or on
ownership:

| alternative | why not |
| --- | --- |
| Change `classify_hit`'s layer-less fallback to consult the `Sensor` query, so a layer-less sensor classifies as an overlap | Fixes this volume and leaves the *solid* world geometry unclassifiable, so a spawn is still clamped by a collision the matrix never resolved. Measured: with no `BodyLayer` anywhere in the world, the leg clamps "by absence" and a consumer cannot tell a classified stop from an unclassified one. It also records a layer nobody declared. |
| Fall back to a default `CollisionLayer::StaticWorld` for any layer-less hit | Same defect, plus it guesses a layer for every body spawned outside `spawn_body` — the case `contacts::record_contact_reports` already counts as `unclassified` on purpose. |
| A new collision layer, e.g. `WorldTrigger`, in `cs_sim::collision` | `cs_sim` is F23-A's path and this is F18's task; the declared vocabulary already has `CollisionLayer::Trigger` for "sensor volumes that report overlap", and this task is not evidence that the six declared layers are the wrong six. A seventh layer would also have to be given partners, a `requires_continuous_detection` answer and a row in `designed_collides_with`, all of which are designed content belonging to F23-A. |
| Declare `CollisionLayer::Trigger` on a trigger volume's collider | Classifies correctly, but it moves the membership bit off `StaticWorld` — and #498's `sweep_volume_crossings` queries the world with `SpatialQueryFilter::from_mask(StaticWorld)`, which Avian tests as `mask & collider.memberships != 0` (`avian3d-0.7.0/src/spatial_query/query_filter.rs`, `test`). A trigger volume on bit 4 would be invisible to the ordinary-flight crossing pass that way, trading this hole for the one #498 closed. |
| Move the preflight into the world bootstrap (`PhysicsBodiesPlugin` in `world_app()`) | Not this task's composition to close, and not free: #416 is **blocked** on the same `world_app()` because adopting `DECLARED_SUBSTEP_COUNT` there measurably regresses trimesh collision, and #415 filed the missing preflight against #416 for that reason. See "The composition this does not close". |
| Fall back to `WorldColliderInstance::role()` inside `classify_hit` | Makes F23's preflight depend on F18's Bevy-side marker, i.e. inverts the dependency between two features and gives the trigger rule a second home. Rejected for the same reason as the second layer: two ways to name a volume. |

## The measurement composition, and the composition this does not close

`world_app()` still installs no spawn preflight, and this task does not change
that: #415 filed it against **#416**, which is blocked on an owner ruling, and
deciding whether a mission world runs the preflight at all is that task's
question, not this one's.

So the measurement needed a composition that runs it. `world_app_with_spawn_preflight()`
is that, and it is deliberately **not** the production one: it is
`world_app()` plus `physics::preflight::install` and the production
`SpawnTickTriggerPlugin`, reachable from a fixture through
`WorldFixtureBuilder::spawn_preflight()`. `PhysicsBodiesPlugin` is not used for it
because that plugin adds `RestingBodiesPlugin`, which `world_app()` already
installs and Bevy refuses as a duplicate — so the two systems the measurement
needs are installed on their own, through the same `preflight::install` the
plugin calls. The default builder path is byte-for-byte the old one, so every
existing F18 measurement is unaffected.

**What this leaves open, and to whom.** The end-to-end path from a world-authored
mission trigger to a fired overlay is *still* not closed, one gap narrower:

* the preflight now classifies a world sensor volume, so a spawn-tick crossing of
  one is recorded and delivered — this task;
* the ordinary-flight crossing of one is recorded and delivered, and reaches
  `OverlayTriggerRequests` — **#498**;
* **#416** for running the preflight in the world bootstrap at all. Until it
  does, no world composition can produce the spawn-tick half, and a mission
  world still cannot deliver that crossing without a composition that installs
  `SpawnTickTriggerPlugin` as well.

The two producers now agree on the pair, which is the part this task had to fix
for #498's and #415's rules to be the same rule: one
`TriggerCrossings` ledger, one `(actor, volume)` key, and a `CrossingSource` so a
consumer can tell which producer decided a crossing.

**One paragraph outside this task's owner paths is now stale, and is left for its
owner.** The module docs of `crates/cs_app/src/objectives.rs` (written by **#415**)
say the world-authored volumes are not yet visible to the preflight's cast, and
give the reason as their carrying no `BodyLayer`. That reason is false after
this task — they carry one — while the claim itself stays true for the other
reason above. That file is #415's, not this task's, so the correction is filed as
**#644** rather than made here; nothing in this branch depends on it.

## Mutation / removal checks

Each mutation was applied to `crates/cs_app/src/world/spawn.rs`, run, and
reverted. "Failing" counts the five `accept_t499_*` tests.

| Mutation | Failing | What it shows |
| --- | --- | --- |
| all three `BodyLayer(static_world_layer())` inserts removed | **4 of 5** | the layer is the whole repair; the survivor is named below |
| the two **sensor-capable** bundles' inserts removed (`spawn_cuboid_collider`, which carries either role, and `spawn_mesh_trigger_volume`); only the mesh solid keeps its declared layer | **4 of 5** | the same four arms, so the pass-through and clamp assertions are not carried by the solid path's coverage |
| only `spawn_mesh_trigger_volume`'s insert removed | **1 of 5** | which test carries the *import* path, stated plainly below |
| (nothing — the shipped tree) | 0 of 5 | — |

The surviving test under the first two mutations is
`accept_t499_solid_world_geometry_still_clamps_a_spawn`, and its survival is the
finding rather than a gap in it: **solid world geometry clamped before this task
and clamps now**, the difference being that it clamps by
`ContactKind::SolidContact` from the matrix instead of by the
`is_none_or(…)` fallback on a missing layer. That test is the regression arm for
the repair, so "it passed before" is what a regression arm is supposed to say;
the four arms that fail are the ones the classification is for.

**Which bundle carries which arm, measured rather than assumed.** The three
behavioural arms (no clamp/stop/delay, the crossing reaching the consumer, the
distance ratio) are measured on the arch world, whose volume is a **hand-built
cuboid**, so they exercise `spawn_cuboid_collider` alone. That is also why
removing the mesh trigger volume's insert alone fails only the first row of the
table: `accept_t499_a_world_collider_declares_the_layer_the_sweep_classifies_it_by`
is the only test that looks at a mesh-derived collider, and it holds the import
path's declared layer directly — it reads the `BodyLayer` off the settled mesh
entity, asserts it is the entity's own membership bit, and computes the
classification `classify_hit` would reach from the two components it reads.
Sweeping a projectile through a **mesh-derived** volume under the spawn preflight
is therefore measured for nothing in this tree, and is not claimed. The
classification is a function of exactly the two components `classify_hit` reads,
so a layout that declares the same layer and carries the same `Sensor` marker
reaches the same classification — but "a swept **spawn preflight** through a
mesh-derived volume behaves as it does through the cuboid one" is a measurement
nobody has made yet. It is a narrow claim: the ordinary swept-CCD path through a
mesh trigger volume *is* measured, by
`accept_f18_b_a_mesh_trigger_volume_reports_a_swept_body_where_a_sample_lands`
(#401), and #401's own deep-inside gap applies to that discrete stream rather than
to a swept cast. Which retail volumes are mesh-derived and how thick they are is
#427; the missing measurement belongs to whichever task closes that.

## The CI run, and one numeric bound worth naming

CI on the rebased tip was **red in `cargo test`, and not on a test**: the link step
of one doctest binary died with `ld terminated with signal 7 [Bus error]`
(`crates/cs_app/src/livery.rs` line 49, F09-C's path, untouched here). Every
ordinary test target in that run is green, **including all five `accept_t499_*`
tests on x86_64**, and `fmt` and `clippy -D warnings` are green in the same run.
Filed as task #637 rather than worked around, because the only path that could
change it is `.github/workflows/ci.yml` and because a green run must stay green
for the right reason.

One bound in the first commit was tightened for a reason that turned out not to be
the cause, and is worth recording because the measurement behind it is real and
reusable: the solid-leg assertion bounded the solver's residual with an absolute
`1e-3 m/s` while the residual measures **-0.000826 m/s** in this geometry and
**-0.0019 m/s** in task #415's — 83% of the bound, and 1.9× it. An assertion that
sits inside the spread of a value it does not control is a coin flip between
architectures, so it is now a thousandth of the fired speed, which still excludes
"not stopped" (60 m/s) by a factor of a thousand. The same reasoning moved the
no-delay tolerances from absolute to speed-relative.

## Designed values, not original data

Every number above is a measurement of **this** project on the pinned pair, with
synthetic masses, extents, positions and speeds. Specifically:

* the declared layer, the choice to leave it at the membership's own value, and
  the split between "the layer" and "the role" are **designed project rules**,
  made so that the existing `cs_sim::collision` matrix decides a world pair
  exactly as it decides a session-spawned one;
* whether the original game declared a layer for its world trigger volumes,
  whether a checkpoint could stop a body spawned inside one tick of it, and
  whether the original reported a crossing at all are **unknown**;
* the arch world's `trigger.sensor` cuboid, the 10 cm projectile, the 0.4-tick
  spawn offset and the three speeds are the F18-A fixture and F23-D's declared
  probe geometry, reused so this stage measures the same crossing the neighbouring
  stages measured.

No original-data, visual, audible or ordinary-play claim.

## Known limitations that gate later stages

Affected content: every trigger/objective volume a swept body can enter on its
spawn tick — projectile spawn-inside-a-trigger, AI spawn inside a mission
volume, and the mission-overlay triggers F18-C binds.

1. **The world bootstrap still runs no preflight.** #416, blocked; this task
   measured what happens *once* it does, and changed no production composition.
2. **The layer is `StaticWorld`, and `CollisionLayer::Trigger` still names the
   layer that reports overlaps.** One declared layer per collider was chosen over
   the semantically prettier split, and the cost is named: a query that filters
   by membership finds a trigger volume under `StaticWorld`. #498's crossing
   pass relies on exactly that, so the two agree — but a future F39-B content
   binding that asks "which layers does this load's trigger set cover" gets
   `StaticWorld`, and that answer is right for this tree and is not a claim about
   the original's own layers.
3. **A spawn tick crosses one volume.** The preflight's sensor cast returns the
   *first* sensor it meets; a body whose spawn tick crosses two world volumes
   records the first. Measured, not assumed — the record is a single
   `Option<Entity>`. #415 already recorded this; it is unchanged here and is an
   F23-C change, not a consumer one.
4. **Only the entry is delivered.** The exit half is #498's stateful rule at the
   ordinary-flight entry point and is unchanged: a consumer must not treat
   `TriggerCrossings` as an "inside" test.
5. **Mesh-derived volumes go quiet for a body that lands deep inside them** with
   no triangle within `max_contact_distance` (#401, measured). The preflight's
   swept cast does not have that gap — it asks the volume's own geometry — so the
   spawn-tick record is *more* capable than the discrete stream here. Which
   retail volumes are mesh-derived and how thick they are is #427.

## Sources

- Task #499; the finding that filed the gap and the code it names:
  `docs/findings/2026-10-02-t415-spawn-tick-trigger-crossing.md`, section "Two
  composition gaps this task found and did not fix"; #415's consumer,
  `crates/cs_app/src/objectives.rs` `deliver_spawn_tick_crossings`.
- `crates/cs_app/src/physics/preflight.rs` — `classify_hit`, the two casts'
  predicates, `SpawnPreflightEvent::passed`; and F23-C's criterion
  `accept_f23_c_preflight_never_stops_on_a_sensor`.
- `docs/findings/2026-10-03-t498-swept-trigger-crossing.md` and
  `crates/cs_app/src/world/crossings.rs` — the ordinary-flight producer, the
  `SpatialQueryFilter::from_mask(StaticWorld)` its world query depends on, and
  the "the clamp only looked like a report" boundary quoted above.
- `docs/findings/2026-09-30-f23-b-body-creation-forces-sweeps-and-transitions.md`
  (limitation 1, the first-tick hole this geometry reuses) and
  `docs/findings/2026-10-02-t401-trigger-volume-and-swept-ccd.md` (the mesh
  volume's deep-inside gap).
- `cs_sim::collision` — `classify_contact`, `designed_collides_with`,
  `CollisionLayer::StaticWorld` and `CollisionLayer::Trigger` labels.
- Pinned source in the local crate cache: `avian3d-0.7.0`
  (`src/spatial_query/query_filter.rs`, `test`: a collider is a query candidate
  when `mask & collider.memberships != 0`), which is what makes the
  `CollisionLayer::Trigger` alternative measurable rather than merely arguable.