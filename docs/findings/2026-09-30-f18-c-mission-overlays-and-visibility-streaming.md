# F18-C: mission overlays and safe visibility/streaming

Date: 2026-09-30. Task: F18-C "Add mission overlays and safe visibility/streaming"
(`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`, section
`### F18-C`), acceptance scenario **AC03**. Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary build/test only
— no `CS_GAME_DIR` read, no evidence report required, nothing
`verified_original`.

## Files and the one observable failure (the slice plan)

* `crates/cs_content/src/world.rs` (edited): `MissionOverlay` and `OverlayEffect`
  (the mission-local change a load carries), `WorldInstance::with_mission_layer`
  and its accessors, and five new `WorldError` refusals.
* `crates/cs_app/src/world/overlays.rs` (new): the producer, the hand-off, the
  consumer, the trace, and `displace_object` / `reapply_object`.
* `crates/cs_app/src/world/visibility.rs` (new): the streaming policy — the
  decision (`holds`, `holds_sector`, `retained_sectors`) and the pass
  (`update_visibility`).
* `crates/cs_app/src/world/residency.rs` (edited): the load record gained the
  overlays, the applied set and the required set, and `load_sector` re-applies
  the load's overlays to freshly spawned objects.
* `crates/cs_app/src/world/fixture.rs` (edited): the **depot world** — the same
  stored arch, with a door panel that fills its opening, a sensor volume in
  front of it, and a third sector that holds nothing gameplay needs — plus
  `depot_meshes`, `door_overlay`, `depot_mission`, `depot_population`.
* `crates/cs_app/src/world/mod.rs` (module declarations and re-exports only).
* `crates/cs_app/tests/world/{overlays,visibility}.rs` (new) and
  `crates/cs_app/tests/world/main.rs` (module declarations and docs only): the
  `accept_f18_c_*` acceptance tests (seventeen as implemented, eighteen after
  review — see the review section at the end).
* This file.

**One observable failure:** a body reaches the trigger volume, the door opens,
and **only the drawn half moves** — the panel is visibly gone and the tunnel is
still sealed. That is
`overlays::accept_f18_c_opening_an_authored_door_moves_its_render_and_its_collision_once`,
and it is the failure the stage exists to make impossible: F18 non-negotiable
behavior 1 says the visual and collision geometry share provenance, which is
necessary but **not sufficient**, because a derived offset applied to one
consumer and not the other satisfies the letter of it and leaves a solid door
nobody can fly through.

## What this stage owns

### The record (`cs_content::world`)

`MissionOverlay` is a **trigger object** and an `OverlayEffect`. The trigger is
*not* a new kind of object: it is an ordinary instance of the same
`WorldDefinition` whose collision role is `Sensor`. That is deliberate — the
spec's own non-negotiable behavior 3 requires gameplay state to survive
streaming for ordinary world objects, and an overlay whose trigger lived outside
that machinery would be exactly the unstreamable exception it forbids.

`OverlayEffect::Displace { target, offset_m }` is the **designed vocabulary**: the
smallest one the spec's own acceptance scenario needs. "Once" is a property of
the *load*, not of the record or the frame.

`WorldInstance::with_mission_layer(overlays, required)` is **additive**: F18-A's
and F18-B's fixtures declare neither and keep validating, which
`accept_f18_c_a_load_that_declares_no_mission_layer_is_still_a_valid_load` pins
against a regression that would otherwise be silent.

### The runtime (`cs_app::world::overlays`)

Four pieces, and the separation is the point:

| end | what it is |
| --- | --- |
| producer | `queue_overlay_triggers`: Avian's `CollisionStart` stream, in `FixedPostUpdate` after the step |
| hand-off | `OverlayTriggerRequests`: the trigger objects those contacts named, deduplicated and in stable order |
| consumer | `apply_overlay_requests` (exclusive) → `apply_overlay` |
| trace | `WorldOverlayLog`: `Applied` or `Refused { reason }` |

The hand-off resource exists because the producer runs in a parallel schedule and
cannot write a world record, while the consumer needs `&mut World` to move
entities. It is the shape F11-C's `AirframeSceneRequest` already uses, it is
inspectable from a test without stepping the schedule, and a producer that fires
for a world with no load cannot panic.

A second producer — `request_overlay` — exists for a mission's own branch (a
script deciding the door opens without anything flying into the volume) and goes
through the *same* hand-off, so there is one place that applies an effect and one
trace that says what it did.

### The policy (`cs_app::world::visibility`)

A radius policy, because that is the smallest honest answer: a sector's own
`Aabb` is the extent the record already declares, and "is the focus within
`radius` of that extent" needs no invented per-sector visibility metadata.

Two rules make it **safe**:

1. **A gameplay-required object is never streamed away.** The load declares it
   and every sector holding one is held whatever the focus does — in both
   directions: a pass that finds such a sector missing loads it again, because
   an absent required object is neither simulated nor summarized in the world.
   That is non-negotiable behavior 3's *still simulated* half.
2. **Everything that does stream away is summarized, not forgotten.** Its objects
   keep their condition and the overlays the load already applied in the
   residency record, and the sector load re-applies both to the fresh entities.
   That is the *correctly summarized* half — and it is what
   `visibility::accept_f18_c_a_streamed_out_object_returns_with_the_state_its_load_holds`
   travels: a door that opened, streamed out and comes back **open**.

The pass **loads before unloads**, so a failure can only leave the world larger,
never missing geometry it was holding. It is also convergent: a second pass over
the same focus reports nothing moved but still reports the sector it is holding.

## Measured behavior on the pinned pair

`bevy 0.19.1` / `avian3d 0.7.0`, `SubstepCount(1)`, 120 Hz fixed rate, gravity
zero. The door is a **cuboid**, so its drawn geometry and its collided geometry
are two entities and a one-sided update is visible.

* **Writing only `Transform` leaves the collider shut for a tick.** Measured on
  the depot panel: after a transform-only write the entity's `Position` still
  read `z = 0`, and only after one further `App::update` did it read `z = 2`.
  Avian's `PhysicsTransformPlugin` runs `transform_to_position` in
  `PhysicsSystems::Prepare`, *before* the step — so the collider is one tick
  behind the draw unless `displace_object` writes Avian's `Position` too. This is
  the measurement the AC03 test is built around: it reads the door **in the
  update the overlay was applied in**, and a mutation that drops the `Position`
  write fails. A test that read the pose after a hundred ticks would not, because
  Avian's sync would have repaired it — and that is worth saying, because it is
  the difference between a test that pins the claim and one that only appears to.
* **Writing `GlobalTransform` as well is dead code.** Bevy's
  `sync_simple_transforms` recomputes it from `Transform` at the start of the
  next step; measured, the value was correct after one `App::update` with no
  write of ours. It was removed rather than tested for. A mutation deleting it
  leaves the suite green, which is the right outcome for a write with no reader
  in between.
* **A swept body *does* reach a trigger volume closely enough to fire the
  overlay.** This is what task #401 asked this stage to verify, and it holds: the
  production body configuration (F23's `SweptCcd` + `SpeculativeMargin::ZERO`)
  crossing the depot's 1 m trigger volume produces a `CollisionStart`, the
  producer queues it and the consumer applies the door overlay.
* **…and it pays for the sensor face.** F18-A measured Avian's swept CCD holding
  a body at a sensor volume's near face with no `Sensor` filter in the sweep
  (`avian3d-0.7.0/src/dynamics/ccd/mod.rs`, `solve_swept_ccd`). Measured here at
  400 m/s (3.33 m of travel per tick) against a 1 m volume: the body covers
  **0.58 m** on the crossing tick instead of 3.33 m, clamped exactly at the
  volume's near face minus its own half extent, and the **next** tick is free
  travel again. So the volume is not a wall, but a fast aircraft loses travel on
  every trigger it crosses. F18-A measured the same rule on an 8 m volume (2.416
  m): the loss is the distance from the previous sample to the face. The loss
  scales with the volume, so this stage's trigger is thin by design; a retail
  trigger's thickness and a mission's trigger placement are unmeasured and are
  filed as task **#427**. **No fidelity claim is made here that a swept body
  passes a sensor volume untouched**, and task #401 remains the resolving task
  for the interaction itself.
* **The panel is 4 m ahead of the volume, which is 1.2 ticks at 400 m/s.** A body
  that fast therefore still meets the *closed* panel on the tick after the
  volume, and is stopped by it before the effect lands. That is a property of
  where the fixture put the volume, not of the overlay, and it is why the AC03
  traversal assertion uses a speed whose sampling cannot outrun the distance
  between a trigger and the geometry it opens. A real mission would place its
  triggers further from the geometry they open, or the door would open a tick
  late for a fast aircraft — recorded, not designed around, and filed as part of
  task **#427**.
* **A body resting against a collider is not released when the collider moves.**
  Measured by despawning and by displacing the panel under a resting body: the
  body's creep is **identical** in all three cases (move, despawn, untouched), so
  what keeps it there is not the panel. It is the contact itself: a body that
  arrives with a normal velocity has that velocity killed by the contact, and
  with no gravity and no drag it then drifts at the residual rate forever. This
  is ordinary contact behaviour, not an overlay bug, and it is recorded because
  "the door opened and the body inside it is still stuck" would otherwise look
  like this stage's fault. The contact/restitution rule that settles it is filed
  as task **#428**.

## Test sensitivity (mutation matrix)

Seventeen mutations were applied, `cargo test -p cs_app --test world -- accept_f18_c_`
was run, and the source was restored each time. **Fifteen of the seventeen are
caught.** The two that survive are recorded below rather than papered over.

| mutation | tests that failed |
| --- | --- |
| `displace_object` never writes Avian's `Position` (M1) | `overlays::..._moves_its_render_and_its_collision_once`, `overlays::..._a_missions_own_request...`, `visibility::..._a_streamed_out_object_returns...` |
| `displace_object` never writes `Transform` (M5) | `overlays::..._moves_its_render_and_its_collision_once` |
| the "already applied" check is skipped (M3) | `overlays::..._moves_its_render_and_its_collision_once`, `overlays::..._a_missions_own_request...` |
| the consumer never drains the hand-off (M6) | 3: the door test, the swept test, the mission-request test |
| the load never picks up its declared overlays (M15) | 7, every runtime test |
| a reloaded sector does not re-apply the load's overlays (M13) | `visibility::..._a_streamed_out_object_returns...` |
| the required set is never recorded (M16) | `visibility::..._holds_a_required_objects_sector...`, `visibility::..._a_second_pass...` |
| the policy holds every sector whatever the focus (M7) | all 6 visibility tests |
| nothing is retained for a required object (M8) | `visibility::..._holds_a_required_objects_sector...`, `visibility::..._a_second_pass...` |
| no sector is ever unloaded (M9) | 4 visibility tests |
| the radius is ignored; only containment holds (M17) | `visibility::..._holds_by_point_to_box_distance...`, `visibility::..._a_streamed_out_object_returns...` |
| a negative radius is accepted (M18) | `visibility::..._every_visibility_refusal...` |
| an unload failure is swallowed (M19) | `visibility::..._a_pass_that_cannot_move_a_sector...` |
| any resolved role is accepted as a trigger (M11) | `overlays::..._every_overlay_record_refusal...` |
| two overlays may share a trigger (M12) | `overlays::..._every_overlay_record_refusal...` |
| a non-finite offset is accepted (M14) | `overlays::..._every_overlay_record_refusal...` |
| the required-set activation check is skipped (M10) | `overlays::..._every_overlay_record_refusal...` |
| **the producer's `role != Sensor` filter is deleted (M4)** | **none — see below** |
| **a missing target falls back to any present object (M21)** | **none — see below** |

### The two that survive, and why

**M4 — the producer's role filter is masked by the consumer's.** The producer
forwards any world object it is touched by; the consumer independently requires
the load to *declare* an overlay for whatever it is handed, and `load_world`
refuses at the door a load whose trigger is not a sensor
(`WorldError::OverlayTriggerNotASensor`). So no reachable load can give the
filter anything to be wrong about, and a mission that declares a sensor-backed
overlay over a solid object is refused before it reaches the runtime. The filter
is kept because it is the check the record's own contract states and because the
consumer's filter is a *different* question ("does the load declare this?" rather
than "can a body enter this?") — but a reviewer should read it as documented, not
as covered. `queue_overlay_triggers`' doc comment says exactly this.

**M21 — the target lookup has no negative case.** `apply_overlay` refuses
`TargetNotPresent` when the target is not present, and
`..._every_overlay_refusal_names_what_it_refused...` reaches that by unloading
the sector. Replacing the lookup with a fallback that picked *any* present object
passes, because in that scenario the fallback would then be asked for a
*different* object, which the test does not inspect. The refusal is real and
tested; the specific way the object is chosen when it is missing is not. The
lookup is a `BTreeMap` keyed by the authored id, so there is nothing to choose
between.

### A branch that is handled and stated rather than tested

`load_sector` can fail a streaming pass with `WorldSpawnError::UnrepresentableTransform`
or `OverlayError::VanishedEntity`, and neither is reachable here for the same
reason F18-B recorded for its own `rollback`: a load refuses a definition with an
unrepresentable matrix **at the door**, so a sector reloaded from the same
definition cannot meet one, and an object the load is spawning does not yet exist
to have lost an entity. The branch is handled rather than assumed, and the test
that documents it is
`visibility::..._a_sector_streamed_back_in_without_geometry_is_a_reported_gap` —
which asserts what *is* reachable (a `FromMesh` object the source does not hold
comes back presented, colliderless, reported `MeshUnavailable`, with nothing
invented in its place) and says plainly in its doc comment that a mutation
swallowing the refusal leaves the suite green.

The *unload* half of the same path **is** reachable and **is** tested: a
presented entity is despawned behind the residency record's back and the pass
refuses, names the object, and leaves the sector loaded.

## Designed vocabulary, not original data

Designed here: `MissionOverlay`, `OverlayEffect::Displace`, the required-object
set, the hand-off resources, the trace records, the radius policy and its
refusals, the depot world and its geometry. None of it is claimed to be the
original's vocabulary or behaviour.

The **required-object declaration** is the one that most needs saying: the
2000 PC original certainly had *some* notion of a mission's important objects,
and this is not a measurement of it. It is the smallest declaration that makes
non-negotiable behavior 3 testable — a set the load states and the policy
honours — and it is what a `cs_content::script` binding would populate once
F06/F07 measure the original's own trigger and objective semantics.

## Known limitations that gate later stages (not silently dropped)

* **A swept body pays travel at every trigger volume** (measured: 0.58 m at
  400 m/s against the depot's 1 m volume; F18-A measured the same rule at
  2.416 m against 8 m). Affected content: every world object with
  `WorldCollisionRole::Sensor` — the depot's `trigger.depot`, any retail trigger
  or objective volume F18-D imports — when the body reaching it carries
  `SweptCcd` (F23's aircraft). Resolving task for the interaction: **#401**.
  Retail volume thickness and mission trigger placement: **#427**. Until both
  are done, no claim is made that a swept body passes a sensor volume untouched.
* **A door opens one tick after the body that triggered it arrives at the
  panel**, if the trigger is closer to the panel than one tick of travel at the
  body's speed. Affected content: every mission overlay a fast aircraft can
  outrun. This is a *design* property of where a mission places its triggers and
  has no engine fix; **#427** carries the measurement, and F18-D's
  original-mission integration is where the real distances get read.
* **A body in contact with world geometry is not released by an overlay that
  moves that geometry away** — measured, and traced to the contact killing the
  body's normal velocity rather than to the move (the creep is identical whether
  the collider moves, is despawned, or is untouched). Affected content: a door
  that opens while something rests against it. Resolving task: **#428**, in the
  same physics path as F23's contact work; not an overlay defect.
* **The streaming policy is asked about one focus, so a sector an actor other
  than the focus is inside of may be streamed away** — and the ground in it goes
  with it. The player is not affected when the focus *is* the player (a focus
  inside a box is a containment, so the player's own sector is always held), but
  an AI aircraft (F31), a mission's scripted mover, or the player when the focus
  is a camera that leads it, are in a sector this pass may unload with no
  protection but the load's own `required` declaration. Affected content: every
  non-player mover, in every world — `cs_app::world::update_visibility` takes one
  [`VisibilityRequest`](../../crates/cs_app/src/world/visibility.rs) and holds one
  sector set, and nothing in the record says where the actors are. Resolving task:
  **#429**. This is a limitation of the *policy's inputs*, not of the residency
  transaction, which is correct about what a load means.
* **The visibility policy is a radius, and the original's rule is unmeasured.**
  Whether the 2000 PC original streamed by distance at all, by a mission-authored
  mask, or by neither is unknown, and this stage does not guess. Affected
  content: every retail world group's streaming behaviour. F18-D.
* **The streaming pass is a function, not a scheduled system.** A sector load
  reaches the mesh spawn path, which takes an `&mut App`; a Bevy system cannot
  hold an `App`, so the pass is called by whatever composition owns the camera or
  the player aircraft. Wiring a real mission's camera into it belongs to the
  mission composition, not to this stage.
* **Only `Displace` exists.** Damage, destruction, rotation, scripted sequences
  and anything a mission script does beyond moving a world object are **not**
  here; the damage rules belong to `cs_content::damage` / `cs_sim::damage`
  (F29), and animation to F20.
* **The original's world-vertex unit scale, its sector storage, its per-object
  collision roles, and whether it stores overlays at all** are unmeasured
  (unchanged from F18-A and F18-B). F18-B/D.
* **No retail world group was visited.** AC04 needs `gpu` + `retail` (**F18-D**).
  Nothing in this stage is `verified_original`.

## Evidence

Ordinary build/test only; no `CS_GAME_DIR` read and no evidence report is
required for this stage. Commands run locally:

```sh
cargo fmt --all -- --check                                        # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # exit 0
cargo test --workspace --locked                                   # 153 binaries, 0 failed
cargo test --workspace --locked -- accept_f18_c_ --include-ignored
#   17 tests run, 17 passed (crates/cs_app/tests/world)
```

The seventeen acceptance tests are listed with their failure sensitivity in
the mutation matrix above.

## Review (bunny-2, fresh context, 2026-09-30)

Checked against `### F18-C`, `AGENTS.md` and the mutation matrix. The two
mutations most load-bearing for this stage were re-run independently — dropping
Avian's `Position` write, and dropping the sector load's overlay re-application
— and each failed exactly the tests the matrix names. The two unclaimed cases in
the matrix were confirmed by reading the producer and the consumer rather than
by re-running all nineteen.

Three things were changed on review, all inside the owner paths:

* **`load_sector`'s abort order.** An object joined the rollback list only
  *after* its condition stamp, its record insert and the overlay re-application
  — the three steps that can still fail. A failure at either of the last two
  therefore returned `Err` while the record already claimed the object present
  and its entities were still live, which is the one outcome `residency`'s own
  `rollback` contract exists to prevent. (The pre-existing `stamp` step carried
  the narrower version of the same hole: the entities leaked and the record did
  not mention them, so a retry would have spawned the object a second time.) An
  object now joins the list the moment its entities exist, so every refusal
  takes back exactly what this call spawned. No successful path changes: the list
  is read only by `rollback` and by the returned `SectorLoad::spawned`. The
  branch is still unreachable, as the section above says; the point is that the
  invariant no longer rests on its unreachability.
* **Four untested public accessors removed** —
  `ResidentWorld::{overlays, is_required, required_sectors}` and
  `WorldInstance::is_required`. None had a caller or a test, and
  `required_sectors` was a second, untested spelling of
  `visibility::retained_sectors`, which is the tested path. The record's
  readable surface is now exactly what is used: `overlay_for`, `is_applied`,
  `applied_overlays` and `required_objects`.
* **The occupied-sector limitation is now actually recorded.** The module
  documentation of `visibility` claimed that the hazard of streaming the sector
  a mover occupies "is recorded as a limitation"; nothing recorded it. It is in
  the list above with its affected content and its resolving task (**#429**,
  `F18-E`), and the module documentation now says which actors it can reach —
  the player is not one of them while the focus *is* the player, and claiming
  otherwise would have replaced one inaccuracy with another.
* **Rule 1 held a required sector in one direction only, and said so.** The
  module documentation stated that rule 1 is "a *hold*, not a force-load" and
  that a caller wanting a required sector back "asks for it with
  `load_sector`". The pass is written as *residency matches the held set* and
  the held set contains every required sector, so the next pass **did** load it
  again — the documentation described a weaker policy than the code, and a
  caller reading it would have believed a required sector could sit empty while
  the pass reported a clean no-op. The code is the right way round (a required
  object that is absent is neither simulated nor summarized *in the world*, only
  in the record), so the documentation now states the two-directional hold, and
  `visibility::accept_f18_c_a_required_sector_unloaded_outside_the_policy_comes_back`
  (new, eighteenth `accept_f18_c_` test) pins it: a sector the *policy* never
  empties is restored when something else does, and the annex still goes in the
  same pass. That test is the **only** thing pinning this direction: mutating
  the pass so it loads from the focus's held set alone (still never unloading a
  required sector) leaves the other seventeen green and fails this one.

One test comment was corrected rather than a test changed:
`..._a_missions_own_request...` described the producer's `role != Sensor`
filter as producing a *traced refusal* if it were removed. It does not: the
consumer skips an undeclared trigger silently, which is what M4 says and what
`queue_overlay_triggers` already documents. The comment now states the same
thing as the module documentation instead of a stronger and wrong version.

## Sources

No external sources were consulted. The record shapes follow
`docs/contracts/IDENTITY-CONTENT.md` and F18-A's and F18-B's findings; the
entity layout is F18-B's collider-on-body rule
(`docs/findings/2026-09-30-t424-collider-on-body-invariant.md`); the swept-CCD
interaction is F18-A's, and task #401 carries the resolution. Every Avian, Bevy
and parry statement above was read from the pinned sources in the local cargo
registry and then *measured* by running the fixture.
