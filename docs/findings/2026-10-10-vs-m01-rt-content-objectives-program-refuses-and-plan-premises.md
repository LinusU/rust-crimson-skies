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

## Amendment (2026-10-10, owner decision): item 7 is the measured control lowering

After the block below, the owner decided (Rally #1214, 2026-10-10, "option 2. Amend acceptance
item 7"): **M01's declared objective program for preparation is the measured control/directive
program** — `mission_control::survey_mission_control_programs` → the mission's row → `lowering()` →
`complete()` → `lowering_attempt().program()` — the same program the `mission_objectives` launch
surface is already judged by (`crates/cs_app/src/mission_launch.rs:908-921`). Item 7 now means
"that program lowers completely", not `ObjectiveRecovery::program()`. The section 1 measurement
above stands unchanged and is the reason the amendment exists; the `ObjectiveRecovery` path stays in
`objectives.rs` untouched, its tests (`accept_m01_lc_objectives_recovery`) untouched, and measuring
it remains #1219's job — explicitly **not** a prerequisite of #1214.

Applied by `bunny-alpha-1` the same day, inside #1214's owner paths:

* `crates/cs_app/src/mission_session/content.rs` — `MissionContent` no longer carries a
  `LoweredObjectives` field and `prepare` no longer calls `recover_retail_objectives`/`lower_program`;
  the `control` field is documented as the single declared objective program. The
  `MissionSessionError::Objectives` variant is removed (nothing constructs it); the `Control` variant
  now carries both the script-host and the objective-source refusal.
* `crates/cs_app/tests/campaign/vs_m01_rt_content.rs` — the retail member flipped from asserting the
  `Objectives` refusal to asserting `accept_vs_m01_runtime_content_m01_prepares_every_record`: the
  world definition imports objects and uploads meshes, the start airframe (`player_pfighter`) and
  pose are `Known`, `pdevastator` imports with a positive fuel load, the weather session starts at
  tick zero, the world-actor binding is satisfied with its three measured actors, the animation join
  binds M01's scope and group, the control lowering produces a program with declared objectives, and
  the sound walk finds an archive.

Consequences for the dependents, stated precisely (the owner's note asks for this):

* **#1215 / #1217**: `ObjectiveSession::launch` (`crates/cs_app/src/objectives.rs:1316`) still takes
  a `LoweredObjectives`, which only `objectives::lower_program` produces from a
  `DeclaredObjectiveProgram` — a type no original mission yields today. `MissionContent::control` is
  a `cs_script::ir::MissionProgram` from the control lowering, a different type. The per-tick host
  must either bridge from that lowered program **where the measured semantics carry it** (never a
  synthesized objective) or block naming this exact mechanism as missing. #1214 supplies no
  `LoweredObjectives` and claims no bridge.

Nothing in this amendment is `verified_original`; no original executable ran.


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

## What was built, and what still cannot be claimed

*(State at the time of the block below, before the owner's amendment above; kept verbatim as the
measurement record. The amendment supersedes "Item 7 is not met".)*

`crates/cs_app/src/mission_session/` implements the whole stage:
`MissionContent::prepare(install_root, plan)` (and the module entry
`mission_session::prepare_mission_content`) reads all nine records the task
lists — world container → import → uploaded meshes → `WorldInstance`, player
start, campaign flight record, weather session, world-actor binding, animation
join, control lowering, sound archives, objective recovery — each through the
named production reader, each refusal a `MissionSessionError` variant carrying
that reader's own message and its source key.

**Measured on the owner's installation** (`cargo test --workspace --locked --
accept_vs_m01_runtime_content_ --include-ignored`, 2 tests, both passing):

* `accept_vs_m01_runtime_content_an_unreadable_installation_is_refused_with_its_source`
  — an empty directory is refused with the source it failed at.
* `accept_vs_m01_runtime_content_m01_prepares_every_record_but_the_objectives_program`
  — M01's real plan runs through preparation and stops at an `Objectives`
  refusal whose text is the recovery's own. Because preparation asks the
  objective recovery **last**, that variant is the proof that the world
  imported, the start airframe and pose arrived `Known`, `pdevastator`
  imported with its fuel load, the weather session started, the world actors
  launched, the animation join bound, the control lowering completed and the
  sound walk found an archive — a refusal in any of those would have been
  reported under its own variant.

So items 1-6, 8 and 9 are done and measured. Item 7 is not, and cannot be
until one of the two routes above is chosen. The task's acceptance — "asserting
M01's content prepares … objectives program lowers" — is therefore **not met**,
and no test in this branch claims it is.

Checks run on this branch, after rebasing onto `origin/main` `0155e295` (Rust
dev profile, from the repository root):

| check | result |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 — 491 `test result` lines, 0 failed (run on `ad879fee` + this branch, before the rebase; the four commits the rebase brought in touch none of this branch's files, so the owner's 2026-10-01 re-push rule applies and CI re-runs it on the pushed commit) |
| `cargo test --workspace --locked -- accept_vs_m01_runtime_content_ --include-ignored` | 0 — 2 passed (the retail member runs 549 s) |

#1214 was handed back **blocked** with this note attached, not submitted for
review: an acceptance criterion production cannot satisfy is a decision for the
owner, not something an implementer quietly rewrites (AGENTS.md rules 4 and 5).
The follow-up work is filed as #1219 (the objective recovery) and #1220 (the
stale `world_geometry` verdict on `main`).
