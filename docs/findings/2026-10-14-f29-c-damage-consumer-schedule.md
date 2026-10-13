# F29-C: the damage-consumer schedule — the session tick drives every pass

Date: 2026-10-14. Task: #668 `F29-C-SCHEDULE` "Drive the F29 damage and
debris consumer passes from the session schedule"
(`specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
`### F29-C`). Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
Predecessors recorded in
`docs/findings/2026-10-02-f29-c-damage-consumers.md`,
`docs/findings/2026-10-02-f29-damage-zone-collider-call.md`,
`docs/findings/2026-10-05-f29-c1-propulsion-consumer.md`,
`docs/findings/2026-10-05-f29-c-2-debris-spawn-consumer.md` and
`docs/findings/2026-10-02-f27-c-weapon-session-wiring.md` (the sibling
schedule this one mirrors). Capabilities used: ordinary build/test only —
no `CS_GAME_DIR` read, no render, no audio, so no `private/evidence/`
report is produced.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/damage.rs` (extend): `FiringGate` with the
  `FireResolver` implementation, `DamageConsumerRefusal::MissingFlightGate`,
  `PendingDamageEvents`/`DamageTickBatch`, `DamageScheduleMissing`,
  `DroppedDamageBatch`, `ReleasedDebris`, `DamageTeardownRecord`,
  `DamageScheduleReport`, `attribute_events`, `bound_damage_actors`,
  `debris_actors`, `flight_gate_entity`, the three merge helpers,
  `drive_damage_schedule` and `DamageSchedulePlugin`.
- `crates/cs_app/src/weapons.rs` (extend): the `FiringGate` implementation
  for `WeaponSession`, and `drive_weapon_session` now publishes each
  resolved batch into `PendingDamageEvents`.
- `crates/cs_app/src/physics/session.rs` (wiring only):
  `production_builder` composes `DamageSchedulePlugin` beside the animation
  and weapon plugins.
- `crates/cs_app/src/lib.rs` (wiring only): the module docs name the
  schedule and the `FiringGate` seam.
- `crates/cs_app/tests/damage/schedule.rs` (**new**), compiled into the
  existing `accept_f29_c_damage_consumers` target; task test prefix
  `accept_f29_c_schedule_`.
- This file.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data,
no binary file. `DamageResolver` itself is untouched: the schedule is a
consumer of it, not a change to the authority.

**One observable failure, before the change:** F29-C and its subtasks
built every consumer seam — the mount/visual pass, the propulsion gate,
the debris spawner, the collider bridge — and each was exercised by a test
that called it directly, but **no running session ever called them**. A
destruction in a live tick left the victim's guns firing, its engine's
thrust authority live, its colliders in place and its wreckage unspawned,
because nothing connected a resolved batch to the consumers that read it.
The resolver emitted `DamageEvent`s and dropped them: `step_weapon_session`
returned them inside `RoutedRound` and the schedule report recorded the
counts, then the records were discarded with the step.

## What this task wires

`DamageSchedulePlugin` installs one exclusive system,
`drive_damage_schedule`, into `FixedPostUpdate` ordered
`.after(drive_weapon_session)` — the same tick and the same placement rule
F30-C's targeting schedule uses, so the producer and its consumers share a
committed tick rather than straddling one. One run, in order:

1. **Session teardown.** `DamageScheduleState` remembers the generation the
   last run served. When the installed `SessionDamageResolver` names a
   different one — a restart's `insert_resource` swap — every debris
   instance the dead generation's actors still own is released through
   `release_debris`, every `PendingDamageEvents` batch the dead generation
   left is dropped, and `DamageScheduleReport::tore_down` names the
   generation with both counts.
2. **Binding-release teardown.** The state also remembers which actors had
   damage bindings (actor, zone or debris). An actor whose bindings all
   went away between runs — the scene teardown that releases a swapped
   aircraft's records — gets the same `release_debris`, named in
   `released`. `SpawnedDebris` is deliberately *not* a binding: counting it
   would re-bind the actor the release was meant to end.
3. **Pending damage.** `drive_weapon_session` publishes one
   `DamageTickBatch` per routed round that produced events: the
   `HitEvent`s handed to `DamageResolver::resolve` beside the
   `DamageEvent`s it emitted, in resolve order. A batch stamped by another
   session is dropped and named in `dropped` rather than consumed.
4. **Colliders.** `attribute_events` assigns each event to its victim: a
   `HitApplied`/`HitBlocked`/`HitRefused` looks its `HitEventId` up in the
   batch's hits and binds the hit's `target`; a `PartTransition` or
   `SystemDisabled` names only a node, so it belongs to the hit in flight;
   `Lifecycle` and `KillAwarded` carry their actor outright. An event tied
   to no hit is counted `unclaimed` — a malformed publication's own name —
   rather than attributed by guessing. `apply_damage_events` then runs per
   victim, unchanged.
5. **The convergent passes, per actor.** The union of bound actors, the
   tick's victims and actors a spawned instance still names gets the three
   state-driven consumers: `apply_damage_state` (mount gate through the
   `FiringGate` surface plus the `AirframeDamageState` visual record),
   `apply_propulsion_state` when the resolver reports a propulsion carrier
   — refused `MissingFlightGate` when no bound entity carries the
   `FlightAircraft` component — and `apply_debris_state`. Being convergent,
   a quiet tick's report is a no-op, which is how a pass that ran is told
   apart from one that could not (`None`).
6. **Destruction publication, last.** `publish_destroyed_actors` turns the
   batch's `Lifecycle(Destroyed)` records into `DamageActorDestroyed`
   messages after the consumers applied, so a subscriber that reacts sees
   the settled state — the F36-C interaction consumer's own latch reads it
   this way.

Every authority a pass needs is named when absent: `missing` carries
`WeaponGate`, `VisualRecord` and `ForeignWeaponGate { found }`, and a
world without the resolver still runs the two teardown halves before
reporting `session: None`.

## Choices worth stating

- **The batch carries hits *and* events.** The resolver's
  `PartTransition` names only a `DamageNodeKey` — no actor — because the
  authority emits it per node within a resolve call. The victim is the
  hit's `target`, so the transport has to carry the hits beside the
  events; a world that saw only the events could not run a per-victim
  collider pass at all. `DamageTickBatch::session` reads the stamp off the
  records themselves rather than trusting a separate field.
- **`FiringGate` is a trait, not an accessor change.** The mount/visual
  seam was written against `&mut FireResolver`, and `WeaponSession` owns
  its `FireResolver` inside a private `GunCadence`. Exposing
  `resolver_mut` would have let any caller reach past the session's
  closed-aware `state`/`state_mut` accessors — the guard F27-C's review
  added for exactly this reason — so the pass is generic over a
  three-method gate and `WeaponSession`'s implementation answers `None`
  after `close`. `FireResolver` itself keeps its direct shape; the
  consumer tests that drive it by hand are untouched.
- **The report answers for the tick, not the history.** `colliders`,
  `consumers` and `debris` are each pass's merged outcome for the run `at`
  names; `begin_tick` clears them because the passes are convergent and a
  destroyed part stays destroyed — re-reporting it every tick would read
  as re-doing it. What happened is retained in the pass logs (the collider
  and consumer logs are the append-only record) and in the world itself;
  the report says what the *run* did. The schedule tests assert on the
  transition tick for the same reason.
- **`MissingFlightGate` is a refusal, not a skip.** A graph that declares
  a propulsion carrier and a world that binds the actor without a
  `FlightAircraft` is a spawn-path gap, and the schedule's job is to name
  it, not to treat "no flight body" as "no propulsion state". It sits
  beside `UnarmedActor`, which names the same gap for the weapon gate.
- **The flight gate is found by binding, not by assumption.** `flight_gate_entity`
  looks for the `DamageActorBinding` entity that also carries
  `FlightAircraft`, filtered by the live `SceneGenerations::latest` when
  one is installed — the same first-bound, generation-checked rule the
  debris index and the collider bridge already follow, so a stale
  generation's leftover binding cannot claim the gate.
- **The consumers run after the collider pass but see the same batch.**
  Collider decisions consume the events directly; the convergent passes
  read the resolver's *state*, which the weapon step already resolved —
  so both halves of the tick see the resolution that produced them.

## Test sensitivity (measured)

Five tests, all through `PhysicsSession::production_builder` at 64 Hz with
a real swept hit: the shooter fires the declared fixture gun at a victim's
`PartSweptBox`, the session's own weapon step resolves it, and every
assertion fails without the schedule installed.

| mutation | caught by |
| --- | --- |
| `DamageSchedulePlugin` not added in `production_builder` | all 5 `accept_f29_c_schedule_*` tests (every report assertion is `None`/zero) |
| the pending batch is not published by `drive_weapon_session` | `the_tick_drives_every_consumer`, `a_destroyed_mount_disables_the_victims_gun`, `a_lethal_hit_publishes_the_destruction`, `refusals_stay_visible`, `teardown_releases_the_debris` (no events → no transitions, no debris, no destroyed) |
| the mount pass drives `FireResolver`-only behavior (e.g. `is_closed` ignored) | `a_destroyed_mount_disables_the_victims_gun` (the victim's `WeaponSession` state is read back) |
| `publish_destroyed_actors` runs before the consumers | `a_lethal_hit_publishes_the_destruction` (the report's `destroyed` is read with the grounded-mount count from the same run) |
| the session teardown keeps the debris | `teardown_releases_the_debris` (`tore_down` and the world instance count) |
| the binding-release half is missing | `teardown_releases_the_debris` (`released` record after despawn) |
| foreign batches are consumed instead of dropped | `refusals_stay_visible` (the foreign actor's refusals would be joined by unclaimed/misattributed events) |

One timing detail the tests rely on and record in their module docs: a
routed contact emits one `HitEvent` per declared channel — the fixture's
armor 6.0 and internal 3.0 — so one accepted shot applies 9.0 and the
engine's 15-integrity pool and the mount's 10 both die on the *second*
shot. The assertions read the report of the tick the transition lands on;
a later tick's convergent no-op is also asserted, which is what would
catch a schedule that ran one tick late.

## Unknowns recorded (not guessed)

- **Whether the original drove damage consumers per fixed tick at all, and
  in what order.** Nothing in the recovered evidence says; the order here
  (colliders → gates/visuals/propulsion/debris → publication) is this
  engine's designed composition, chosen so a destroyed part's consequences
  are all settled before the destruction is announced.
- **What the original did with a wreck's debris on restart.** The
  generation-qualified release is the engine's own rule; the original's
  scene teardown is unmeasured.
- **Every numeric value in these tests** is the synthetic fixture's own:
  damage amounts, integrities, cadence, box geometry.

## Not claimed

No original-data verification, no claim that the original scheduled these
passes or reported them this way, no redesign of the consumer seams, no
change to the authoritative resolver, and no load of original content.
