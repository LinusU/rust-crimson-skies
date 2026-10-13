# F39-E-ACTORID-SPACE: the declared-vs-spawned `ActorId` contract

Date: 2026-10-13. Task: F39-E-ACTORID-SPACE "Decide the declared-vs-spawned
ActorId contract for mission importers" (Rally #594). The sheet
`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md` has no
`### F39-E-ACTORID-SPACE` section of its own, so the task-test prefix this stage
uses is `accept_f39_e_`, stated here as the sheet states a prefix per stage.
Shared contract: `docs/contracts/SCRIPT-MISSION.md`. Capability used: none —
every test is synthetic; no original data is read and nothing here is a claim
about the original game.

Status: designed behavior, **not** original-verified.

## The question F39-D left open

F39-C wired declared actors and spawned wave instances into one `ActorId`
space: `ObjectiveRuntime` allocated wave instances `ActorId(1)`, `ActorId(2)`,
… upward, while `cs_content::objectives::ProgramActor` carried declared,
pre-placed actors in the same numbers. The fixture kept `actor(7)` and
`actor(41)` above the six ids its three two-raider waves took, but nothing
enforced that relationship. A declared `ActorId(2)` collided with the second
admitted wave member, and the collision misattributed trigger watches, roster
membership, lifecycle facts, objective counts and teardown ownership. F39-D
recorded the choice — declare-and-compute, a reserved band, or a separate id
type — as task #594's to make.

## The decision

**A reserved high band for spawned instances, plus a typed declared reference
vocabulary.** Two numbers can never collide because they are drawn from
disjoint ranges, and the schema can refuse the references that pretend
otherwise:

- `SPAWNED_ACTOR_BASE = 0x4000_0000` is declared once in
  `cs_content::objectives` and once in `cs_sim::objectives::runtime`, pinned
  equal by a compile-time assertion at the `cs_app` lowering boundary. The
  band is `0x4000_0000..=u32::MAX`; the low range below it is the placed-actor
  space.
- `ObjectiveRuntime` starts `next_instance` at `SPAWNED_ACTOR_BASE` and still
  allocates upward in admission order: the k-th admitted wave member of a
  session is `ActorId(SPAWNED_ACTOR_BASE + k - 1)`. Monotonic, never reused
  inside a session; a retry restarts at the base, which is exactly why the
  F39-C teardown report is not optional.
- `ProgramActor` keeps meaning "a declared, pre-placed actor" — the
  `world_actors`/mission-importer shape F38 measured is untouched. Objective
  rosters and trigger subjects now carry `ProgramActorRef` instead:
  `Placed(ProgramActor)` for an authored actor, `Spawned(k)` (1-based) for
  "the k-th instance this program can admit". The declared side is typed so
  an importer cannot write an ambiguous number.
- `try_new` computes the program's `max_wave_instances()` — the most wave
  members the program can ever admit — and closes the space:
  - `Placed` at or above `SPAWNED_ACTOR_BASE` → `PlacedActorInSpawnedBand`
    (the id belongs to the waves);
  - `Spawned(0)` or `Spawned(k)` beyond the bound → `UnadmittedSpawnedActor`
    (no wave can ever wear it);
  - a bound beyond the band's capacity → `SpawnedBandOverflow`.
- `lower_actor` maps `Placed(a)` to `ActorId(a.0)` and `Spawned(k)` to
  `ActorId(SPAWNED_ACTOR_BASE + k - 1)` — the same ids the runtime hands out.

## How the wave bound is computed

`wave_instance_max` sums, per distinct spawn idempotency key, the largest
`count` declared on that key:

- Reusing one key across timers contributes its **maximum** count, not the
  sum — the emission ledger admits a key once, so the first admission takes
  the ids and every later use of the key is a refusal.
- Distinct keys sum.

The bound is deliberately conservative: a dialogue cue and a spawn action
share the ledger's key space, so a cue could consume a key before a spawn
ever runs. Counting the possible spawn anyway keeps the bound an upper
bound — a `Spawned(k)` inside it is always potentially admittable.

## Why this over the alternatives

- **Declare-and-compute** (reject declared actors inside the wave range the
  program computes) keeps one undifferentiated number space: an importer
  writing `actor(2)` still cannot say whether it means "the placed actor
  numbered 2" or "the second wave member", and every program change silently
  moves the forbidden range. Rejected.
- **Separate id spaces** (a newtyped spawned-instance id through `cs_sim`)
  is the right *distinction* but the wrong *blast radius*: `ActorId` flows
  through lifecycles, counters, teardown, net facts and the display. The
  band keeps one runtime id while `ProgramActorRef` keeps the type
  distinction exactly where confusion lives — the declared record. Chosen,
  with the band mirroring the split onto the id space itself.
- **Precedent already in the tree:** `cs_sim::capital::CapitalShipSet`
  allocates session aircraft ids **above** registered ship ids
  (`next_aircraft` advances to `max(current, registered + 1)`), i.e. the
  engine already separates runtime-minted identities from authored ones by
  range. The band is the same move with a fixed boundary instead of a moving
  one — a fixed base is what lets the schema refuse `Placed` ids inside it.

## Files

- `crates/cs_content/src/objectives.rs` — `SPAWNED_ACTOR_BASE`,
  `ProgramActorRef`, `DeclaredCondition::roster` and `DeclaredTrigger::actor`
  retyped, `DeclaredTriggerDuplicate` carrying the ref, the three new schema
  errors, `wave_instance_max`/`max_wave_instances`, validation in `try_new`,
  module docs, the synthetic fixture's roster/trigger spelled
  `ProgramActorRef::Placed`, five `accept_f39_e_actorid_` unit tests.
- `crates/cs_sim/src/objectives/runtime.rs` — `SPAWNED_ACTOR_BASE`,
  `next_instance` initialized to the base, module docs.
- `crates/cs_app/src/objectives.rs` — `lower_actor(ProgramActorRef)`, the
  compile-time base-equality pin, wiring docs.
- `crates/cs_sim/tests/accept_f39_e_actorid_space.rs` (new, 3 tests):
  every admitted instance takes a band id; each session starts at the base;
  a placed actor is never counted as a wave instance.
- `crates/cs_app/tests/accept_f39_e_actorid_space.rs` (new, 4 tests): the
  two crates' bases are one boundary; `Spawned(k)` lowers into the band; a
  placed actor at the old wave indices is never counted; a retry rebinds the
  wave into the same band ids.
- Renumbered consumers whose expectations were the old low allocation:
  `accept_f39_b_objective_runtime.rs`, `accept_f39_c_objective_session.rs`,
  `accept_f29_c_04_bailout_session_consumer.rs`,
  `accept_f39_d_objective_branching.rs`,
  `accept_f39_e4_count_category_producers.rs` (import only).

## Measured sensitivity

The band is enforced structurally, not by test convention: reverting
`next_instance` to `1` fails
`accept_f39_e_actorid_a_placed_actor_is_never_counted_as_a_wave_instance`,
`accept_f39_e_actorid_each_session_starts_the_band_at_its_base` and
`accept_f39_e_actorid_every_admitted_instance_takes_a_band_id`
(verified by a temporary revert during development, then restored).

## Unknown / deferred

- **Which ids original missions used is still unmeasured.** The band, the
  base and `ProgramActorRef` are designed vocabulary. `ProgramActor` remains
  what F38's importer measured — dense record indices — and now those ids are
  structurally incapable of meeting a wave member.
- **A `Placed` id below the band that still collides with a *real* world
  actor is the importer's problem, not the objective program's.** The schema
  can only bound what the program declares; which placed ids the world
  actually instantiates is the mission importer's contract.
- **`Spawned(k)` names an admission ordinal, not a group member.** If a
  future stage needs "the third raider of wave 2 specifically", the bound
  arithmetic is the place to add it; nothing today decomposes `k` by wave.
