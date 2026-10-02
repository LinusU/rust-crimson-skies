# T415: the spawn-tick trigger crossing is delivered as a gameplay crossing

Date: 2026-10-02. Task: #415 "Decide and implement consumption of the
spawn-tick trigger crossing record" (follow-up filed by the F23-D review, task
#92). Producer: `crates/cs_app/src/physics/preflight.rs` (F23-C's swept spawn
response, F23-D's second non-blocking cast). Consumer:
`crates/cs_app/src/objectives.rs`, this task. Contracts:
`docs/contracts/FLIGHT-PHYSICS.md` ("Collision and ballistic tests"),
`docs/contracts/SCRIPT-MISSION.md` ("Objective event ordering"). Sibling task
for the same rule at the other entry point: **#498** (`F18-trigger-swept-
crossing`).

Capabilities used: ordinary build/test only. No `CS_GAME_DIR` read, no evidence
report required. This stage can award at most **checked**.

## The decision

> **A trigger a body crosses inside its spawn tick is delivered as a gameplay
> crossing, not kept as a diagnostic.** It enters the authoritative event
> stream on the tick it happened, exactly once per `(actor, volume)` pair, and
> the delivery changes nothing about the body: no stop, no delay, no impulse.

F23-D closed the record half of a measured engine hole and wrote the rule half
down as undecided, in these words: *"Whether gameplay consumes that field as a
trigger crossing is a rule F23-C's criterion does not decide, so this stage does
not invent one."* This is that decision.

The alternative the acceptance criteria offer — record it and declare it
diagnostic-only — was rejected on the affected content, not on taste. The
record's readers are mission trigger volumes: a mission volume a body can cross
and never report is a mission that cannot be completed, and a field only a log
reads is a defect report, not a fix. Neither is a fidelity claim: nothing here
asserts what the original game did (see "Designed values, not original data").

## The crossing rule, decided once (for #498 to reuse)

The decision is not "the preflight said so". It is the rule both entry points
must satisfy, stated so the ordinary-flight path cannot drift into a different
one:

1. **Decide from the body's own motion over the tick.** A segment swept
   against the volume, or the engine's own time of impact along that segment —
   never a sampled overlap, which is exactly what a body that outruns the
   volume's thickness cannot produce. This is the contract's own rule ("For
   interaction triggers use a swept center/shape appropriate to the original
   rule") and F39-A's (`docs/findings/2026-10-01-f39-a-objective-trigger-spawn-semantics.md`:
   "Triggers sweep the real per-tick movement segment").
2. **The decision is rate-independent.** A body that covers the volume in one
   tick at 60 Hz covers it in one tick at 120 Hz, and the crossing is reported
   in both. Measured below: all 12 rate/speed cells deliver exactly one
   crossing.
3. **Exactly once per `(actor, volume)` pair.** A body that stays inside a
   volume, a record a multi-tick frame re-reads, and a second body crossing the
   same volume are three different cases with three different answers (nothing,
   a counted duplicate, a second entry).
4. **Delivery is a read.** A trigger is not an obstacle (F23-C's criterion is
   not weakened here, and it is measured below).
5. **The exit half is the other path's business.** A spawn-tick record is an
   entry by construction — a body that appears inside a volume was not in it
   before it existed. Whether a body that later leaves emits an exit is
   #498's stateful question, and it needs the same one-bit "inside" per pair
   that `cs_sim::objectives::SweptTrigger` keeps. This task delivers the entry;
   it does not invent a half-kept state machine for a path that does not exist
   yet.

## What was measured, on the pinned pair

`bevy 0.19.1` / `avian3d 0.7.0`, `SubstepCount(2)` (the product default F23-D
froze), `Gravity::ZERO`, a 2 cm cuboid trigger volume and a 10 cm projectile,
the spawn 0.4 ticks of travel short of the volume — F23-B's first-tick hole,
where the body is not yet in the broad phase. Every row is one
`accept_t415_a_every_probed_rate_and_speed_delivers_exactly_one_crossing`
world; "engine reports" counts the classified contact reports naming that
volume over the four ticks, and the test prints the tick each of them landed on
so the table's one exception can be checked rather than believed.

| rate | speed | travel/tick | crossing distance | engine reports | crossing tick |
| --- | --- | --- | --- | --- | --- |
| 60 Hz | 60 m/s | 1.00 m | 0.390 m | 0 | 1 |
| 60 Hz | 120 m/s | 2.00 m | 0.790 m | 0 | 1 |
| 60 Hz | 300 m/s | 5.00 m | 1.990 m | 0 | 1 |
| 60 Hz | 600 m/s | 10.00 m | 3.990 m | 0 | 1 |
| 120 Hz | 60 m/s | 0.50 m | 0.190 m | 0 | 1 |
| 120 Hz | 120 m/s | 1.00 m | 0.390 m | 0 | 1 |
| 120 Hz | 300 m/s | 2.50 m | 0.990 m | 0 | 1 |
| 120 Hz | 600 m/s | 5.00 m | 1.990 m | 0 | 1 |
| 240 Hz | 60 m/s | 0.25 m | 0.090 m | **1** (tick 2) | 1 |
| 240 Hz | 120 m/s | 0.50 m | 0.190 m | 0 | 1 |
| 240 Hz | 300 m/s | 1.25 m | 0.490 m | 0 | 1 |
| 240 Hz | 600 m/s | 2.50 m | 0.990 m | 0 | 1 |

Reading the table:

* **The hole is the pass-through, not the overlap.** Eleven of twelve cells
  report nothing at all: no sample of the body ever lands inside a 2 cm volume
  it crosses in one tick, so the discrete narrow phase has nothing to report.
  This reproduces F23-D's table (limitation 1) on the current tree, including
  its one exception: at 240 Hz and 60 m/s a tick is 0.25 m, the body comes to
  rest *inside* the volume, and the engine's own stream reports the overlap on
  **tick 2** — one tick late. The crossing this task delivers is stamped tick 1
  in all twelve cells, so the swept decision is never later than the discrete
  one, and it exists where the discrete one does not. That ordering is pinned
  as an assertion rather than a table: wherever the engine's stream does report
  the pair, the test requires its tick to be **strictly greater** than the
  crossing's tick. The exact count is deliberately not asserted — a future
  Avian that closed the hole would add reports, and the invariant worth
  keeping is that this project's crossing is not one of them.
* **The decision is rate-independent, as the rule requires.** The same body
  covering the same volume is one crossing at every rate, and the recorded
  distance is always strictly inside that tick's travel (0.39 m of 1.00 m at
  60 Hz/60 m/s, 0.19 m of 0.50 m at 120 Hz/60 m/s, 0.09 m of 0.25 m at 240 Hz /
  60 m/s — each exactly `0.4 × travel − 0.01 m`, so the offset from the spawn
  distance is the same **0.01 m** in every cell, which is the volume's half
  thickness). The body's own 0.05 m half extent does not appear in that
  constant because the probe's spawn position already discounts it:
  `x = −(0.4 × travel + 0.05)` puts the body's *front face* exactly 0.4 ticks
  of travel behind the volume's centre, and the sweep stops when that face
  reaches the volume's near face, 0.01 m in front of the centre.
* **A body that spawns already inside a volume is a different case and is
  measured separately** (60 Hz, 1 m/s, a 2 cm volume the body dwells in for
  several ticks): the preflight's cast reports the volume at distance **0.0**,
  the crossing is delivered once at the spawn position, and the engine's own
  stream *also* reports an overlap on tick 1 — the body is inside the volume,
  so there is a sample to find. Measured across 60 Hz/60 m/s, 60 Hz/1 m/s,
  120 Hz/0.5 m/s and 240 Hz/60 m/s: the record exists in every case, at
  distance 0.0, and the body is never clamped. So the record is not only a
  pass-through detector, and the engine is not blind to *every* spawn-tick
  trigger case — it is blind to the ones a body flies through. Unlike the
  pass-through matrix, this part is **pinned**: all four combinations are run
  by `accept_t415_a_the_spawn_inside_case_is_where_the_engine_is_not_blind`,
  which requires the record at distance 0.0, a crossing delivered once on the
  spawn tick, no clamp, *and* a classified `SensorOverlap` from the engine's own
  stream on that same tick. Without that last assertion the pass-through /
  overlap distinction — the one that keeps the record from being misread as a
  general spawn-tick overlap detector — would be prose only.
* **The delivery does not move or delay the body.**
  `accept_t415_a_the_delivery_does_not_move_or_delay_the_body` runs F23-D's
  geometry twice — a 2 cm trigger at `x = -0.10`, a 2 cm wall at `x = +0.10`,
  a 10 cm projectile fired at 60 m/s from `x = -0.40` at 120 Hz — once with
  `SpawnTickTriggerPlugin` installed and once without, and compares the two
  tick-by-tick traces value for value: every classified contact (pair, kind,
  tick), every preflight record (clamp, stop, obstacle role, distance, crossed
  volume role, distance), and every end-of-tick position and velocity. They are
  **equal**, over all six ticks, including the first tick's end-of-tick pose
  `x = 0.0406681` and the residual `v = -0.0018856069` the solver leaves on the
  wall's face (the solver keeps easing it in over the following ticks, so those
  are the tick-1 values, not a fixed point). The crossing was measured at 0.24 m
  into the tick and the wall at 0.44 m, the wall still reported exactly one
  `SolidContact`, and the body still rests on the wall's near face rather than
  rebounding — a delivery that stopped or delayed the body could not produce
  two identical trajectories.
* **"Exactly once" is a property of the consumer, not of the frame length.** A
  three-tick render frame reads the same preflight record on all three ticks
  (the session drains the log at the *end* of the frame), so a consumer that
  re-read blindly would deliver the same crossing three times. Measured:
  `delivered = 1`, `duplicates = 2` out of a three-tick frame; a later frame
  adds nothing; a second body crossing the same volume is a second pair and
  enters once (`delivered = 2`); taking the batch empties the stream but keeps
  the pair ledger, so a taken crossing does not come back.
* **Nothing is invented for a body that crossed no volume.** A projectile with
  a wall 510 m away and no volume produces `delivered = 0`, and the preflight
  record it does produce names nothing (`passed == None`, `clamped == false`).

The system itself is a second, structural measurement: it takes
`Res<SpawnPreflightLog>`, `Option<Res<PhysicsTickLedger>>` and
`ResMut<TriggerCrossings>` and nothing else — no `Query`, no `Commands`, no
mutable access to a pose or a velocity. A delivery that cannot name a body's
components cannot change one. The ordering is load-bearing in the same way and
is measured by a mutation below: dropping `.after(PhysicsSystems::StepSimulation)`
lets the consumer run before the preflight has written the record, and six of
the eight acceptance tests fail on the tick stamps.

## Mutation / removal checks

Each mutation was applied to `crates/cs_app/src/objectives.rs`, run, and
reverted on this branch. "Failing" counts `accept_t415_a_*` tests. The review
re-applied every row independently and re-measured them against the eighth
test (`accept_t415_a_the_spawn_inside_case_is_where_the_engine_is_not_blind`,
added by the review); the counts below are the review's, and they differ from
the implementer's submission in one row.

| Mutation | Failing | What it shows |
| --- | --- | --- |
| the once-per-pair ledger deleted from `TriggerCrossings::record` (every read delivered) | 2 of 8 | the dedup is load-bearing for a multi-tick frame, not decoration |
| the delivery's schedule slot dropped (unordered with the preflight) | 6 of 8 | the crossing is stamped with the tick it happened on only because the consumer is ordered after the producer |
| the delivery given a `Query<&mut LinearVelocity>` that zeroes the actor's velocity | 3 of 8 | the trajectory-identity test fails when a delivery touches a body — the non-blocking claim is measured, not asserted |
| the delivered `kind` changed from `Entry` to `Exit` | 5 of 8 | a crossing is an event, not a level, and a consumer can read which without matching on fields |
| the delivery system removed (the plugin installs only the resource) | 7 of 8 | the crossing leaves no trace anywhere without it — the state F23-D's review called out. The one survivor is `accept_t415_a_body_that_crosses_no_trigger_delivers_nothing`, which asserts that *nothing* is delivered and is therefore satisfied vacuously; that is what makes it the negative case and not a seventh positive assertion. (The implementer's submission recorded this row as "7 of 7"; the count was wrong, and the error was the count only, not the claim.) |

The two assertions the review added are load-bearing rather than decorative:
the `Entry → Exit` row went from 4 failures to 5 because the new spawn-inside
test reads the kind, and the schedule-slot row went from 5 to 6 for the same
reason.

## Two composition gaps this task found and did not fix

Both are outside its owner paths (F18's world spawn and F23-C's preflight), and
both are filed as separate tasks rather than fixed here. They are named because
they bound the claim: **the end-to-end path from a world-authored mission
trigger to a fired overlay is still not closed**, and this task's crossing
stream is its first working half, not its last.

1. **A world-authored sensor volume carries no `BodyLayer`, so the preflight
   cannot classify it.** `classify_hit` in `preflight.rs` returns `None` for a
   hit entity without a `BodyLayer`, and the sensor cast's predicate accepts
   only `Some(ContactKind::SensorOverlap)`. `crates/cs_app/src/world/spawn.rs`
   inserts `WorldColliderInstance` and `Sensor` on a world object's collider but
   no `BodyLayer`, so a mission trigger volume can never appear in
   `SpawnPreflightEvent::passed`. Two consequences, both read off the code and
   both needing measurement before they are fixed: the record is missing for
   the volumes the task's affected content names ("the mission-overlay triggers
   F18-C binds"), and the *solid* cast treats a layer-less hit as solid
   (`is_none_or(|kind| kind == ContactKind::SolidContact)`), so a spawn whose
   tick passes a world sensor volume may be clamped against it — which would
   violate F23-C's criterion for exactly the volumes a mission cares about.
   Filed: "Give a world-authored sensor volume a collision layer" (F18/F23-C
   owner paths).
2. **The mission world composition does not run the preflight at all.**
   `world_app()` (`crates/cs_app/src/world/fixture.rs`) installs
   `PhysicsAdapterPlugin`, `WorldPlugin` and `WorldOverlayPlugin`, but not
   `PhysicsBodiesPlugin` — so `resolve_spawn_preflights` is not in that
   schedule. The same composition also overwrites `SubstepCount(1)`, which is
   already task **#416**; the missing preflight belongs with it, and a note was
   added there rather than a duplicate task.

The consumer deliberately does **not** feed F18-C's
`OverlayTriggerRequests` from this crossing stream. With both gaps open that
branch could not be reached by any world, and an unreachable branch in a
hand-off is worse than a missing one: it would read as "the crossing opens the
door" while no load can produce it. Wiring the hand-off is F39-B's composition
work, after the two gaps above.

## Designed values, not original data

Every number above is a measurement of **this** project on the pinned pair, with
synthetic masses, extents, positions and speeds. Specifically:

* the crossing decision, its once-per-pair ledger, the schedule slot and the
  plugin are **designed project rules**, chosen so the contract's swept-trigger
  requirement is met and so #498 can adopt the same rule at the other entry
  point;
* whether the original game reported a trigger crossing for a body that
  spawned inside or swept through a volume, whether a trigger fired on entry
  only or on both entry and exit, and with what delay, are **unknown**. F23-D
  left the delay to **F26** and this task leaves it there: the measurements
  above say that *this* implementation delivers the crossing on the tick it
  happened, one tick before the engine's own stream in the one cell where the
  engine reports it at all. They say nothing about the original;
* the 2 cm volume, the 10 cm projectile, the 0.4-tick spawn distance and the
  rate/speed matrix are the declared probe geometry of F23-D's contact sweep,
  reused so the two stages measure the same crossing.

No original-data, visual, audible or ordinary-play claim.

## Known limitations that gate later stages

Affected content: every trigger/objective volume a swept body can enter on its
spawn tick — projectile spawn-inside-a-trigger, AI spawn inside a mission
volume, and the mission-overlay triggers F18-C binds.

Resolving tasks: the two composition gaps filed above (a world sensor volume's
collision layer, and the preflight in the world bootstrap with **#416**);
**#498** for the exit half and the ordinary-flight path; **F39-B** for the
content binding (which `SymbolId` a volume is, and the `SweptTrigger` ledger
that pairs it with an `ActorId` and a `Volume`); **F26** for any calibrated
delay; **#427** for retail trigger thickness and mission trigger placement,
which is what decides whether the pass-through case is rare or routine in the
original.

1. **Only the entry is delivered.** A body that crosses a spawn-tick volume is
   one entry; the exit, and a body that crosses and returns, need the stateful
   per-pair rule that #498 is to define. A consumer of this stream must not
   treat `TriggerCrossings` as an "inside" test — it is an event stream.
2. **The stream names entities, not content.** A crossing names the volume's
   `Entity`. The `cs_script::ir::SymbolId` a mission program would use is
   F39-B's binding; guessing one would put a fabricated id into a mission's
   trigger table.
3. **One volume per spawn tick.** The preflight's sensor cast returns the
   *first* sensor it meets along the sweep, so a spawn tick that crosses two
   volumes records the first only. Measured, not assumed: the record is a single
   `Option<Entity>`. A body that can cross two volumes in one tick needs a
   producer that reports both — an F23-C change, not a consumer one.

## Sources

- Task #415; the F23-D finding that left the rule open
  (`docs/findings/2026-09-30-f23-d-stability-high-speed-contact-and-convergence-evidence.md`,
  limitation 1) and its acceptance test
  `accept_f23_d_a_spawn_records_a_sensor_it_crossed_before_a_solid_stops_it`,
  whose geometry this task reuses unchanged.
- `docs/findings/2026-09-30-f23-c-authoritative-events-render-interpolation-and-session.md`
  (the preflight's clamp-only response and the criterion that a sensor never
  stops a spawn), `docs/findings/2026-09-30-f23-b-body-creation-forces-sweeps-and-transitions.md`
  (limitation 1: the first-tick hole), and
  `docs/findings/2026-10-01-f39-a-objective-trigger-spawn-semantics.md` (the
  swept-segment crossing rule and the deferred content binding).
- `docs/findings/2026-10-02-t401-trigger-volume-and-swept-ccd.md` and task
  **#498** — the same report boundary at the ordinary-flight entry point, and
  the measurement that a discrete overlap reports nothing for a body that
  outruns the volume.
- `docs/contracts/FLIGHT-PHYSICS.md` ("Collision and ballistic tests"),
  `docs/contracts/SCRIPT-MISSION.md` ("Objective event ordering"),
  `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`
  (owner paths and non-negotiable behavior 1).
- Pinned sources in the local crate cache: `avian3d-0.7.0`
  (`dynamics/ccd/mod.rs`, `collider_tree/update.rs` — the swept AABB is written
  after the step, which is the hole the preflight closes).
