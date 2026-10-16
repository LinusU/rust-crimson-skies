# M01-PLAY-AI-RESTART (#1509): a composed restart respawns M01's scripted aircraft flying

Date: 2026-10-16. Task: `M01-PLAY-AI-RESTART` (Rally #1509). Capabilities
used: `retail` (read-only `$CS_GAME_DIR`) plus build/test. No `gpu`, no
`audio`, no original run. Implementer: bunny-alpha-1 authored the fix and
the first draft of the acceptance legs and evidence harness before its model
stopped answering; bunny-alpha-2 finished them (rebased onto the current main,
carried the airframe table through the `StageBinding` seam #1481/#1499
introduced, finished and ran every check and produced this report). Reviewer:
pending — a separate Rally reviewer agent instance; the owner prefers a
different instance/model with a fresh context, and same-agent review is not
independent evidence. Nothing here is `verified_original`; a Rally merge awards
`checked` only.

## The defect

`mission_session::host::mission_host_restart` — the `PostUpdate` entry the
playtest's `R` reset and a terminal's retry reach — takes the `MissionStage`
**resource** out of the world before it calls `MissionHost::restart`. Inside
that call, `crates/cs_app/src/mission_actors.rs::spawn_mission_actors_under`
re-read the stage from the world for the AI half of the spawn:

```rust
let stage = world.get_resource::<MissionStage>();
let airframes = stage.map(|stage| stage.ai_airframes.clone()).unwrap_or_default();
let player_team = stage.and_then(|stage| /* the player record's measured team */);
```

so after **any** composed restart the airframe table was empty and every
non-asleep scripted record fell into the `airframe.is_none()` arm: it spawned
the kinematic held body with the reason *"the stage carries no imported
airframe for this record's name prefix"*, and `player_team` was `None`, so the
designed role split ([`MissionAircraftAi::for_record`], #1436) stated no side
for any record. M01's AI aircraft therefore stopped flying — and stopped being
rostered as hostile — for the rest of the run after a retry, while the parts
plan, the world and the sessions were all correctly rebuilt by the same
restart.

This is the **same class** of hazard `M01-PLAY-TRIGGERS-PARTS` (#1481) fixed
for the part-state chains and `M01-PLAY-TRIGGERS-TURRET-WIRE` (#1499) fixed for
the `TURRET` table: a stage-derived table read from a resource the composed
restart deliberately holds out of the world. The fix shape was already in the
file — `player_team` travels in the caller-built spawn state and
`parts::plan_part_states` is handed in — it simply had not been applied to the
airframe table.

## The fix

No new stage read and no original-data value: the airframe table now travels
the way the part-state chains and the `TURRET` table already do, and the player
team the way it already did.

| Piece | What it does |
| --- | --- |
| `mission_actors::StageBinding` | gains `ai_airframes: Option<MissionAiAirframes>` beside `parts`/`turrets`: the table the stage imported, or `None` when the composition carried no stage at all |
| `mission_actors::stage_binding(stage)` | fills it from the stage the caller holds — the **Startup** path as `world.get_resource::<MissionStage>()`, the **restart** path as `respawn_mission_actors`'s `&MissionStage` argument |
| `mission_actors::spawn_mission_actors_under` | reads `binding.ai_airframes` and `view.player_team` instead of `world.get_resource::<MissionStage>()`; the "no MissionStage resource" refusal is now driven by `binding.ai_airframes.is_none()`, so a stage that is present but imports nothing keeps its own refusal list and no spurious one is added |

The `MissionStage` resource is untouched, `mission_host_restart` is unchanged,
and no wiring in `mission_session/host.rs` was needed: `respawn_mission_actors`
already received the stage by reference for the parts (the task allowed a
wiring-only pass through `MissionHost::restart`; none was required).

## What the acceptance suite proves

`crates/cs_app/tests/campaign/m01_play_ai_restart.rs`, prefix
`accept_m01_play_ai_restart_` — 2 tests (1 CI, 1 retail):

* `…_synthetic_composed_restart_respawns_the_record_flying` (CI) — a synthetic
  stage whose one scripted record has an imported airframe and a measured team,
  beside the `player` record whose measured team splits the roster, is driven
  through the composed restart entry (a `MissionHostRestartRequest` consumed by
  `app.update()`, which holds the stage resource out of the world while the
  host rebuilds). After the restart the record comes back with
  `MissionAircraftAi`, a **dynamic** body and the same
  `Pursue { target: Player }` behaviour the first spawn selected, under a
  freshly allocated session actor, and the same hostile roster faction — the
  designed role split #1436 added. **Without the fix the record comes back
  held**: confirmed during implementation by restoring `main`'s
  `mission_actors.rs` and re-running the leg, which then panicked at the
  flown/held comparison with `flown ["synthetic_1"] → held ["synthetic_1: the
  stage carries no imported airframe for this record's name prefix"]`.
* `…_retail_composed_restart_respawns_every_flown_record` (`#[ignore]`,
  retail) — against the owner's installation, M01's own roster spawns through
  the imported airframes, the composed restart runs, and **every record that
  flew before flies after it**, with the same designed behaviour, a dynamic
  body and a freshly allocated session actor, while the one measured
  `deactivated` sleeper (`bsfury_1`) stays held on both sides of the restart.

```sh
cargo test --workspace --locked -- accept_m01_play_ai_restart_ --include-ignored   # 2 tests, 1 retail
```

## Evidence

The acceptance run of record (its recorded log is the first artifact), then the
harness on the same clean commit, then the structural check:

```sh
set -o pipefail
cargo test --workspace --locked -- accept_m01_play_ai_restart_ --include-ignored \
  2>&1 | tee private/evidence/M01-PLAY-AI-RESTART/cargo-test.log

CS_EVIDENCE_DIR=private/evidence/M01-PLAY-AI-RESTART \
CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_play_ai_restart_ --include-ignored" \
CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
  cargo test --locked -p cs_app --test evidence_report_m01_play_ai_restart -- --ignored
python3 tools/validate_evidence.py private/evidence/M01-PLAY-AI-RESTART/acceptance.json \
  --artifact-root private/evidence/M01-PLAY-AI-RESTART
```

`--require-pass` is deliberately **not** used: it refuses any report whose
`unknowns` array is non-empty (`tools/validate_evidence.py`), and this report
carries the same four unresolved issues its parent `M01-PLAY-AI` does (the
unrecovered AI control law `0x48c220`, the unapplied per-airframe
randomisation, the unmeasured `team`/`primary_target` semantics, and the absent
original restart reference). Deleting them to turn the flag green is exactly
what the owner directive forbids; they stay in machine-readable evidence. The
report's `claim` is `implemented`, its capabilities are `[retail, synthetic]`,
and the pass/fail facts are carried by `command.exit_code` and the `tests`
counts, which the plain validation does check.

The second artifact, `ai-restart.json`, is a **second production observation**
on the same tree: the harness re-reads M01's stage through `stage_for`,
composes the mission headlessly, records the first spawn's flown/held split and
each flown record's session actor, drives the composed restart entry, and
records the fresh generation's split — every flown record back with its law,
its designed behaviour and a freshly allocated session actor, the measured
sleeper still held. Record names, counts and `session:serial` spellings only;
no original bytes.

## One unrelated transient the evidence does not assert

Once the respawn's aircraft fly, their **arming** exposes a pre-existing,
restart-only ordering wart that is not this task's claim and is deliberately
not asserted by the report:

`arm_mission_ai` (`FixedPreUpdate`) registers each AI actor into the
`WeaponSession`; `ensure_mission_weapons` (`FixedPostUpdate`) re-opens that
session when the host's served generation changes. On the **first fixed tick
after** a restart the arming pass therefore still sees the previous
generation's session and records one refusal per awake record —

```
<record>: the weapon session refuses the guns: the actor belongs to session 2,
but this session is 1
```

— which `MissionAiReport::refusals` keeps for the rest of the run, even though
the session is re-opened later in that same tick and every record is armed from
the second tick on (the acceptance suite's driven roster assertions confirm
this: the freshly allocated actor is in the targeting store within
`DRIVE_TICKS`). Nothing here depends on it — before #1509's fix no aircraft flew
after a restart, so no arming pass ran and the refusal was unreachable; the fix
simply makes a latent ordering fact observable. It is filed as Rally #1518
(`M01-PLAY-AI-ARM-ORDER`) rather than fixed in this slice, which owns only the
airframe table.

The evidence harness reads `MissionAiReport` **right after the restart frame
and before any fixed tick**, so the report it writes is the respawn's own (its
`refusals` is empty with the fix, and would carry the *"no MissionStage
resource"* refusal without it); it never drives a tick, which is what would
append the unrelated transient above. The acceptance legs check their refusals
the same way.

## Unknowns, affected content and resolving work

| Unknown | Affected content | Resolves in |
| --- | --- | --- |
| the original's AI control law (`0x48c220`) is unrecovered | every scripted aircraft's manoeuvre, aim and throttle in any mission, before and after any restart | static analysis of `0x48c220`'s callers, or #358 `REF-OWNER-FIRST-CAPTURE` |
| per-airframe ±5 % AI randomisation (`0x4771c6`) is not applied | every AI aircraft's handling envelope | #805 `FLIGHT-ORIGINAL-AI-RANDOMISATION` |
| what the `aiv.zrd` `team` numbers and a stored `primary_target` mean engine-wide is unmeasured | who every scripted aircraft attacks in every mission record set | #1453's follow-ups or an original run |
| whether the original's own retry/restart respawned its scripted aircraft flying is unmeasured | M01's retry path and every mission restart | #358 `REF-OWNER-FIRST-CAPTURE`, or an owner `human_review` of a restart |

## Status and limits

* `checked` only when merged: no `verified_original` and no `release_approved`
  is claimed anywhere, and no original executable ran. `retail` is read access
  to the owner's installation.
* The restart's *session allocation* and its *body swap* are this
  reimplementation's contract for a composed restart, not the original's
  measured behaviour (see the last unknown).
* The synthetic leg proves the restart path and nothing about retail
  parameters: its stage, airframe and teams are declared fixtures, imported
  through the production `from_values` boundaries.
