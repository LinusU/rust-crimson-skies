# VS-M01-RT-MISSION-HOST.01: the stage's host records, and the one composed per-tick entry

- **Task:** #1278 `VS-M01-RT-MISSION-HOST.01` — "Add the mission host's records to
  the stage and run them through one composed per-tick entry"
- **Implementer:** `bunny-alpha-2`, 2026-10-10, branch
  `rally/1278-add-the-mission-host-s-records-to-the-st` cut from `origin/main`
  `ad912d75`
- **Capabilities used:** `retail` (read of `$CS_GAME_DIR` through production code),
  `CS_ENGINE_IMAGE` set, ordinary build/test. Nothing here is `verified_original`;
  no original executable ran and no original capture was produced.

## What was built

### The seed on the stage

`MissionStage` gained one field, `host: MissionHostSeed`
(`crates/cs_app/src/mission_session/host.rs`). `stage_for` fills it from the
`MissionContent` it already prepares, which now carries one more record:

* `MissionContent::resolver: MemberResolver` — built in `prepare_world` by
  `MemberResolver::from_container(&container)`, the retail constructor over the
  very container the world definition was imported from, so the resolver and the
  definition can never come from different content. This is the fact-8 production
  path; nothing re-reads the world to learn how a chain resolves.

The seed holds: the `EnvironmentSession`, `Option<LoweredWorldActors>` (from
`MissionWorldActors::lowered()`), `Option<MissionAnimationBinding>`, the control
`MissionProgram`, the sound archives, the `LoweredObjectives`, the
`MissionMarkerBindings` cue table, the `MemberResolver` and the stage's own
`Vec<MissionHostRefusal>`.

### The host and its one entry

`MissionHost::launch(stage, generation, served)` runs each record's own
constructor — `WorldActorSession::launch`, `MissionAnimationPlayer::new`,
`MissionMarkerConsumer::new`, `ObjectiveSession::launch`,
`MissionSession::launch(program, generation, [])` — and returns the refusing
constructor's message as `MissionHostLaunchError` rather than working around it.

`MissionHost::step(&mut World, elapsed)` advances, in the documented order:

1. environment — `set_paused` from `PlaytestState`, then `advance_frame(elapsed)`;
2. world actors — `WorldActorSession::step` to the host tick, `commands: []`,
   `probes: []`;
3. animations + markers + objectives — exactly one `step_mission_animations`
   call (fact 4: the host's records are out of their `Resource` for it, because
   the system takes the whole host out of the world first);
4. script host — `compose_mission_facts` folded, then `MissionSession::advance`.

It answers with `MissionHostTick { tick, environment_ticks, world_actors,
animation, markers, objectives, script }` — every record's own output, none of
it built here.

`mission_host_tick` is a `&mut World` system installed by
`install_mission_host(app)` from `add_composition`, ordered
`.after(PhysicsSystems::StepSimulation)` in `FixedPostUpdate` — the same
ordering `SpawnTickTriggerPlugin` and `AnimationPlugin` already use. It takes
the host out of the world, steps it, puts it back, and writes
`MissionHostReport { tick, answer: Result<MissionHostTick, MissionHostStepError> }`
after **every** attempt. `teardown` removes both resources, so a second
composition starts from nothing.

### Startup rows

Startup rows of the join are grouped by their own event and offered at
`Tick(0)` with `physics::BASELINE_FIXED_HZ` as the player's rate — the same
grouping `mission_launch::started_rows` already runs for the launch-surface
measurement, so the two offer the rows identically and differ only in the rate
they state (see #1281).

### The absences, each named

| refusal | when | whose words |
| --- | --- | --- |
| `ObjectiveDeclarations` | `stage_for` | `ObjectiveRecovery::program()`'s own `Display`, or the census/`lower_program` refusal |
| `AnimationJoin` | `launch`, stage has none | the stage's own |
| `WorldActors` | `launch`, scope lowered none | the stage's own |
| `WorldObservation` | `launch`, always today | the fail-closed read |
| `BlockLifecycles` | `launch`, always today | the empty table's own rule |

Four were specified; `BlockLifecycles` is a fifth this task had to add (see
"Residues" below).

## Decisions this task had to make

### `elapsed` is one fixed tick's timestep, not the render frame's delta

The composed entry runs once per **fixed** tick, and `advance_frame` is a wall-time
accumulator: `carry + elapsed_ns · hz) / 10⁹` with the remainder carried
(`cs_sim::time::GameClock::advance`). Handing it the render frame's delta once
per fixed tick would advance the weather by the frame's wall time twice (two
fixed ticks per 1/60 s frame at 120 Hz) — the weather would run at twice real
speed. `Time<Fixed>::timestep()` is the gameplay time one committed tick covers,
and with `MissionContent::tick_rate()` = `BASELINE_FIXED_HZ` = the environment's
own rate, one composed step commits exactly one authored weather tick, with the
carry absorbing the `8_333_333 ns` vs `1/120 s` rounding. Measured: after two
composed steps the environment clock is at tick 1, and both acceptance members
assert it left zero.

### The tick authority is `PhysicsTickLedger.ticks`

`record_tick_boundary` increments it at the start of every fixed tick, before
`PhysicsSystems::Prepare`, so a system ordered after `StepSimulation` reads the
tick whose physics has already run. The host keeps no clock of its own, and a
tick it already stepped is not stepped (nor reported) twice — the record player
and the objective session both refuse a repeated tick.

### Session generations are minted, never invented

`mint_host_generation()` takes the next value from a process-wide `AtomicU32`
starting at 1, mirroring `SessionBuilder::open`'s own `NEXT_GENERATION`, and
`host_session_id(generation)` derives the `SessionId` the record player and the
marker consumer serve from it. Two compositions in one process therefore never
share a generation (`IDENTITY-CONTENT`), and the mission evaluator's
generation-seeded RNG stream replays within one process.

### The empty `LoweredObjectives` and its forward path

`stage_for` calls `recover_retail_objectives(install_root, &plan.mission_dir)` →
`program()` so the refusal detail is **the reader's own message**, then carries
`no_declared_objectives()`. Today that reader refuses unconditionally for every
original mission, so M01 launches an empty objective session — with no
`ObjectiveSpec`, `CountCondition`, `MissionTimer`, `SweptTrigger` or spawn group
constructed anywhere (AGENTS rules 4 and 5).

The `Ok` arms are deliberate and unreachable today: **if** `program()` ever
produces a `DeclaredObjectiveProgram` (#1219), `stage_for` lowers it and carries
the real program with no refusal, because silently running an empty session
where a declared one exists would be exactly the invention the refusal is there
to prevent. When #1219 lands, the retail member's refusal assertion must break —
and that break is the correct signal to rewrite it against the real program.

`TickInput::terminal_requests` stays empty on every step (fact 3): the only
`TerminalPrecedence` this build has is the designed conservative policy
`SCRIPT-MISSION.md` reserves for synthetic tests, and with no declarations the
field is never consulted. A retail terminal outcome belongs to the control
program's own `INSTANTWIN`/`INSTANTLOSS`, which is `.02`'s job.

## Residues, each with its own task rather than a fix here

* **Cadence divergence — #1281** (`VS-M01-RT-HOST-CADENCE`). The composed entry
  steps the world-actor session to the host tick at `BASELINE_FIXED_HZ` = 120/s,
  while `LoweredWorldActors::ticks_per_second` is
  `mission_world_actors::SESSION_TICKS_PER_SECOND` = 64 and
  `WorldActorSet::step` applies `dt = 1/64` **per tick**
  (`cs_sim/src/world_actors/runtime.rs:1120`), so route followers advance
  120/64 = 1.875x their declared speed while the host runs. The task text says
  "to the host tick" and the acceptance says "reached the host tick", so this
  task stepped it literally and filed the decision: either
  `SESSION_TICKS_PER_SECOND` aligns with the fixed timeline, or the host
  converts host ticks into the session's own domain. `mission_world_actors.rs`
  is not an owner path of #1278, so it was not touched. The record player's
  120 Hz is self-consistent (a record's stored second then equals a real
  second); `mission_launch::started_rows` still measures it at 64 and says only
  "the consumer takes these rows".
* **No block-lifecycle reader — #1282**
  (`VS-M01-RT-HOST-BLOCK-LIFECYCLES`). No production code anywhere in the
  workspace turns a record's `BEGIN_DORMANT`/`TICK_DEPENDS_ON_OBJ` spellings
  into `BlockLifecycleTable::declare` calls (`LifecycleDecl` appears only in
  `cs_sim` and in tests), so the fold runs over an empty table and every
  `Condition::ObjectiveAwake` answers `false` — which makes the whole program
  inert, since `lower_block_condition` gates every block on its own wakefulness.
  That is #1278's fifth, unnamed-in-the-description refusal; naming it is not
  building it, so #1282 owns the reader.
* **`App::run` consumes the app (fact 10).** `run_windowed`'s
  `teardown(&mut app)` after `app.run()` is a no-op, because
  `App::run(&mut self)` moves the app into the runner and leaves `App::empty()`
  behind. It is harmless — the runner drops the app — and this task neither
  fixed nor widened it; the line belongs to the window task's owner paths.
* **Facts 9 and 11 did not apply yet.** This stage neither unloads nor reloads
  the world, and it latches no reset, so none of the three `load_world`/`unload_world`
  options and no `PostUpdate` reset seam was needed; both belong to `.03`.
  When `.03` picks one, the three options in fact 9 still stand unchanged.

## Acceptance

Tests named `accept_vs_m01_runtime_host_01_*` in
`crates/cs_app/tests/campaign/vs_m01_rt_host.rs`, registered with one `mod`
line in that directory's `main.rs`:

* **`..._synthetic_stage_drives_every_record_through_one_composed_tick`** (CI):
  composes the same stage `vs_m01_rt_window.rs` builds — the synthetic stage now
  carries a seed of its own (fixture clear-sky environment, an empty-but-real
  `LoweredWorldActors`, no animation join, a synthetic one-objective control
  program whose condition is `true`, the empty objective program and an empty
  cue table) — and asserts **produced** state: the environment clock left tick
  zero and committed ticks, the world-actor session's tick equals the composed
  tick on both the report and the live session, the record player's
  `TickReport::tick()` and `advanced_through()` are the composed tick, the
  `MarkerDelivery` in the report is the consumer's own applied batch, the
  objective `SessionTick` and the live runtime both stand at the composed tick,
  and the control program's `MissionTick` is for that tick with the objective
  the program's own condition admitted **completed**
  (`host.script().state().is_completed(SymbolId(0))`). The program's answer is
  then recomputed independently — the stage's program launched again over the
  same `compose_mission_facts` fold and stepped over the same ticks — and must
  equal the reported `MissionTick` field for field. No expected value in the
  test is constructed by hand.
* **`..._retail_m01_runs_its_real_program_through_the_composed_entry`**
  (`#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`): M01's real plan,
  stage and composition through the same entry, asserting the same produced
  state, that M01's session carries **no** synthesized objectives, that the
  script half is `Running` (M01 cannot reach a terminal headlessly — its two
  `Finish` blocks start dormant behind the `-1` sentinel and every wake path
  needs world facts a headless run does not have; that is fact 7, a property of
  the content), and that the F39 refusal is present and quotes the reader's own
  words (`no declared objective program can be recovered`), together with the
  `WorldObservation` and `BlockLifecycles` refusals.

Both fail if the composed entry is removed: `MissionHostReport` is written by
that entry and by nothing else, so deleting the step or the system leaves no
report to assert on.

The window suite's synthetic stage was updated in place (it must still compile
and run without an installation) and its `Scratch`, `game_dir` and
`synthetic_stage` are now `pub(crate)` so the host suite drives the very same
stage — the suite-sharing seam `f50_c.rs` already uses.

## Checks run on this branch

| check | result |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 — exit 0, 495 `test result: ok` lines, none reporting a failure |
| `cargo test --workspace --locked -- accept_vs_m01_runtime_host_01_ --include-ignored` | 0 — 2 passed (1 synthetic in CI, 1 retail over `$CS_GAME_DIR` + `$CS_ENGINE_IMAGE`, 541 s) |
| `cargo test -p cs_app --test campaign -- accept_vs_m01_runtime_window_` | 0 — 2 passed, 1 ignored without `CS_GAME_DIR` |

## What is not claimed

No original executable ran; `retail` is read access to the owner's installation.
Nothing here is `verified_original`. M01 is not asserted to reach a terminal
state headlessly. No terminal exit mapping and no restart exists yet — those are
`.02` and `.03`. The marker consumer drained no cue, because nothing in this
composition publishes one (a world with no `AnimationLog` drains nothing, which
is that layer's own "no producer, no consumer" rule); the empty cue table is the
stage's declared absence, not an invented one.
