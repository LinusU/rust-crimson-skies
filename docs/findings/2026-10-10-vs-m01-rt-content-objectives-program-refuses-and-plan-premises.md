# VS-M01-RT-CONTENT: M01's objectives program cannot lower, and main's launch plan is 10/11

- **Task:** #1214 `VS-M01-RT-CONTENT` — "Prepare M01's launch content through the production readers"
- **Measured by:** `bunny-alpha-2`, 2026-10-10, on `origin/main` `ad879fee`
  (branch `rally/1214-prepare-m01-s-launch-content-through-the`)
- **Capabilities used:** `retail` (read of `$CS_GAME_DIR` through production code), `CS_ENGINE_IMAGE`
  set, ordinary build/test. Nothing here is `verified_original`; no original executable ran and no
  original capture was produced.

This note records two measurements the task's own description asserts differently. Both were taken
before any `mission_session` code was written, because both change what that code is allowed to
claim.

## 1. M01's `ObjectiveRecovery::program()` refuses, so item 7 of #1214 cannot succeed

### What the task asks for

`crates/cs_app/src/mission_session/` must gather, per #1214's "What to build" item 7:

> **Objectives**: the mission's declared program through the production recovery at
> `crates/cs_app/src/objectives.rs:5609` (`program()` → `cs_content::objectives::DeclaredObjectiveProgram`)
> and `objectives::lower_program`.

and the acceptance retail test must assert, among its parenthetical list, that **`objectives program
lowers`** for M01 (the list maps 1:1 onto the nine "What to build" items).

### What production actually does

`ObjectiveRecovery::program()` is unconditional:

```rust
// crates/cs_app/src/objectives.rs:5603-5615
/// The declared program, or the refusal naming what cannot be recovered.
///
/// # Errors
///
/// [`ObjectiveRecoveryRefusal`], always today: no field family has a measured
/// recovery, so a program would be built from guessed semantics.
pub fn program(&self) -> Result<DeclaredObjectiveProgram, ObjectiveRecoveryRefusal> {
    Err(ObjectiveRecoveryRefusal {
        mission: self.mission.clone(),
        families: self.unrecovered_by_family(),
    })
}
```

It is the **only** production function in the workspace whose result type is
`Result<DeclaredObjectiveProgram, _>`:

```
$ grep -rn "Result<DeclaredObjectiveProgram" crates --include='*.rs' | grep -v tests
crates/cs_app/src/objectives.rs:5609:    pub fn program(&self) -> Result<DeclaredObjectiveProgram, ObjectiveRecoveryRefusal> {
```

So there is no alternate production route to a `DeclaredObjectiveProgram` for an original mission.
`cs_content` documents why: an `installation`-origin record is `DeclaredSupport::Original` with "no
recovery … unplayable until an importer attaches one"
(`crates/cs_content/src/objectives.rs:96-101`), and `missions/bindings/M01.json` still lists
`"objective graph: not bound from original data at this stage"` among M01's unknowns.

### Measured, not inferred

```
$ cargo test --locked -p cs_app --test accept_m01_lc_objectives_recovery -- --include-ignored
running 2 tests
test accept_m01_lc_objectives_names_every_field_it_cannot_recover ... ok
test accept_m01_lc_objectives_retail_m01_drops_no_record ... ok
test result: ok. 2 passed; 0 failed; 0 ignored
```

The retail test (`crates/cs_app/tests/accept_m01_lc_objectives_recovery.rs:59-92`) reads M01's real
record and ends with

```rust
// Semantics are unmeasured, so no program is emitted.
recovery
    .program()
    .expect_err("M01 semantics are unrecovered");
```

M01's `objectives.zrd` yields 58 blocks and 358 fields, **all** unrecovered
(`fields_recovered() == 0`). The refusal therefore holds for M01 on this installation, today.

### Why this is not only #1214's problem

The composed per-tick entry the line depends on takes an `ObjectiveSession` as a required argument:

```rust
// crates/cs_app/src/mission_animations.rs:1128-1134
pub fn step_mission_animations(
    world: &mut World,
    player: &mut MissionAnimationPlayer,
    markers: &mut MissionMarkerConsumer,
    objectives: &mut ObjectiveSession,   // not Option
    facts: &TickInput<'_>,
) -> Result<MissionAnimationStep, MissionAnimationStepRefusal>
```

and `ObjectiveSession::launch` (`crates/cs_app/src/objectives.rs:1316`) takes a
`LoweredObjectives`, which only `objectives::lower_program` (`:834`) produces from a
`DeclaredObjectiveProgram`. So:

* **#1214** cannot satisfy "objectives program lowers" for M01 — `prepare` would refuse at item 7
  with `ObjectiveRecoveryRefusal`'s own message, and a truthful retail test would have to assert
  that refusal instead of asserting preparation.
* **#1215** calls `MissionContent::prepare`; if item 7 is a hard refusal, its retail test
  ("never `Blocked`, never `NoRuntime`") cannot pass either.
* **#1217** ("drive M01's mission host … `ObjectiveSession::launch`") needs the same program.

### The two ways out (owner's decision)

1. **Measure M01's `objectives.zrd` field semantics into a recovery** that `program()` can accept
   (the F13/F39 measurement work: 358 fields across 58 blocks), or
2. **Amend #1214 item 7 (and #1217's objective source)** to name the production source the
   composition is really meant to run — today the only original mission program that *does* lower is
   the control/directive path (`mission_control::RetailControlRow::lowering()` → `complete()`,
   `crates/cs_app/src/mission_control.rs:299`), which the `mission_objectives` launch surface is
   already judged by (`crates/cs_app/src/mission_launch.rs:908-921`).

Neither route is inside #1214's owner paths (`mission_session/**`, wiring-only `lib.rs`,
`tests/campaign/**`, `docs/findings/**`), so #1214 cannot resolve it itself.

## 2. On `main`, `cs --mission M01` is 10/11, not 11/11 — `world_geometry` still reads a stale verdict

#1214's Context says the command "now measures **all 11 launch surfaces Satisfied** and then
returns `MissionLaunchError::NoRuntime`" and asks to confirm it on the task branch. Measured on
`origin/main` `ad879fee`:

```
$ CS_ENGINE_IMAGE="$CS_ENGINE_IMAGE" ./target/debug/cs --cs-path "$CS_GAME_DIR" --mission M01
cs: M01 cannot launch: 1 of 11 launch surfaces are not satisfied:
  world_geometry: unknown: the container imports to a WorldDefinition (144 partition cells,
  346 objects) with the `identity` axis map applied as ObservedTool under
  f18-world.world-axis-convention-measured, and every `fvol*` record classified as fog (#716);
  what no stage has answered is the collision role of the 4 grid-named `fvol*` volume records
  (f18-world.grid-named-fog-volume-role-unmeasured); and what the 1 grid record that stores no
  mesh index drew (...)
EXIT=1
```

The verdict on `main` is the pre-#771 reading: `geometry_verdict` still treats
`partition_records_fog_volume()` as an unanswered question
(`crates/cs_app/src/mission_launch.rs:635-653`), although that accessor measures an **overlap** —
how many grid-named records the original's fog consumer keys — and #771
(`docs/findings/2026-10-08-m01-lc-world-residual-roles.md`) answered those records.

The correction exists, but only on **#359's un-merged branch**: commit `737cc00b` "Read the
world_geometry verdict off #771's measured residual counters" (touches
`crates/cs_app/src/mission_launch.rs` and `crates/cs_app/tests/campaign/vs_m01_runtime.rs`).

Consequences:

* `MissionLaunchPlan::launchable()` is `false` on `main`, so `launch_mission` returns
  `MissionLaunchError::Blocked`, not `NoRuntime`, for `--mission M01` on a branch cut from `main`.
* #1214's Context premise (and #1215's "All 11 surfaces measure Satisfied on main today") was
  therefore measured on #359's branch, not on `main`.
* #1215's retail acceptance ("the retail plan reaches the composition, never `Blocked`") cannot
  pass on a branch cut from `main` until `737cc00b`'s verdict reaches `main`.
* The dependency is circular as things stand: #359's branch can only merge after its subtasks
  (#1214–#1218) merge, and those subtasks branch from `main`, which does not have the fix. The
  verdict change needs its own task/branch to land independently.

## What was not done

No `crates/cs_app/src/mission_session/` code was written and no `accept_vs_m01_runtime_content_*`
test exists yet. Writing them before the two questions above are answered would either commit a
test that asserts a refusal the acceptance describes as a success, or drop item 7 from a task whose
acceptance enumerates it — both are decisions for the owner, not for the implementer
(AGENTS.md rules 4 and 5).

#1214 was blocked with this note attached; the follow-up work is filed as separate tasks.
