# M01-LC-DIRECTIVE-LOWERING: the three stages compose, and the lowered program runs

Date: 2026-10-07. Task: `M01-LC-DIRECTIVE-LOWERING` (#717), the integration
step after stages `.01` (#724), `.02` (#725) and `.03` (#726). Capabilities
used: `retail` (read-only `$CS_GAME_DIR`) plus build/test. No `gpu`, no
`audio`.

## What this step is, and what it is not

The three stages each proved their own layer: the multi-shape binding
vocabulary, the side-effect-free block conditions, and the record→`RawProgram`
adapter with the two lowering rows derived from its attempt. This step adds the
seam none of them covered — that what they produce is **runnable**: the lowered
`MissionProgram` is accepted by `cs_sim::mission::MissionSession` (the runtime
performs its own validation at launch), steps inside the work budget, and,
once a caller supplies the facts the record's own spellings declare, drives to
a terminal outcome with every measured directive site reaching the host's
directive log.

It is a **second production observation**, not a paraphrase: no original
executable was run and no mission was played, so nothing here is
`verified_original`, and a Rally merge awards `checked` and nothing more.

## The two cases

`crates/cs_app/tests/accept_m01_lc_directive_lowering.rs`, test prefix
`accept_m01_lc_directive_lowering_`:

| Case | Retail | What it pins |
| --- | --- | --- |
| `..._a_lowered_record_runs_to_a_terminal_outcome` | no (authored record) | a two-block record lowers (4 sites bound, validation clean), launches, and the record's own spellings drive it: the block that starts dormant does **not** latch at tick 0 while the awake block completes and emits `WakeAnimation` with the args it spelled; the `BEGIN_DORMANT` child0 (mission-clock seconds, finding B) wakes the dormant block through `BlockLifecycleTable::tick`, whose `DormantStart` + `WakeAnimation` then reach the directive log and the `INSTANTWIN` marker resolves `TerminalState::Succeeded` |
| `..._m01_lowers_launches_and_steps_in_the_runtime` | yes (`CS_GAME_DIR`) | for `zbd/c1c/m01`: every lowering row is met and `RetailControlRow::is_complete()` is true — the two rows a launch path reads, i.e. the `mission_program` / `mission_objectives` surfaces — the bound program is accepted by the runtime, and three ticks advance with no budget stop and no terminal state |

The retail case's second half is the fail-closed half: with an unpopulated
`MissionFacts` no numbered block can be awake, so the mission completes
nothing and emits nothing. Both of those "nothing" assertions are the point —
an unpopulated fact map completes nothing rather than everything.

Suite: `cargo test --workspace --locked -- accept_m01_lc_lowering_adapter_
accept_m01_lc_directive_lowering_ --include-ignored` → 14 tests, 4 retail.

## What "runnable" still does not mean

* **The world-side fact maps had no producer — closed by M01-LC-WORLD-FACTS
  (#751).** `MissionFacts::actors` and `MissionFacts::objectives` have
  production writers in `cs_sim`; `members`, `groups`, `generators` and
  `animations`, which the lowered `InactiveMembers`, `EnemyGroupDepletion`,
  `Travelers` and `AnimationStates` conditions read, were populated by nobody
  when this step recorded the gap, because mapping an original
  node/part/part-state chain onto a live world object needs a name resolver
  and world registries this tree did not have. #751 built them:
  `crates/cs_app/src/world_facts.rs` resolves each chain against the
  installation's own node hierarchy, carries the world's report for presence,
  group living counts, generator pending spawns and animation state bytes, and
  folds all four maps through `MissionFacts::absorb` before the tick
  (`compose_mission_facts`). So **24 of M01's 58 blocks** — the 12 INACTIVE
  ladders, the 8 DEDG blocks, `OBJECTIVE3`'s TRAVELERS and the 3 ANIM_STATE
  blocks `OBJECTIVE11`/`OBJECTIVE15`/`OBJECTIVE18` — are no longer
  *permanently* blocked; `docs/findings/2026-10-08-m01-lc-world-facts.md`
  states which of them can complete on what a host can observe today, with its
  evidence.
* **Gating, as discharged:** while #751 was open, no fidelity /
  `verified_original` / `release_approved` claim about **M01 objective
  completion** could stand, and any evidence reporting M01's objectives as
  completable had to name #751 as an unmet condition. #751 has landed the
  writers, so the "no production writer" form of that limit is closed; what it
  gated is unchanged — no original run has shown M01's objectives completing,
  and a Rally merge awards `checked`. Source of the original limitation and of
  the 24-block figure: bunny-alpha-2's review hand-off note on #717
  (2026-10-07).
* A bound call still only emits `Action::Directive` for the host:
  `DirectiveDisposition::is_implemented` stays `false` for every measured key,
  and only the two outcome spellings reach an engine operation. The measured
  residual unknowns (IDENTITY's third child, the in-play flag's writers, DEDG's
  member-field rewrites, TRAVELERS' polarity, the sound-group handles, the
  animation call's trailing arguments) are unchanged by this step and stay
  named on the rows and keys that carry them.

## How to re-derive every number here

```sh
cargo test --workspace --locked -- accept_m01_lc_lowering_adapter_ accept_m01_lc_directive_lowering_ --include-ignored   # 14 tests, 4 retail
cargo test --workspace --locked -- accept_m01_lc_lowering_ --include-ignored                                            # the three stage suites, 35 tests
```

The evidence report for the parent task is regenerated with the command in
`docs/findings/2026-10-07-m01-lc-directive-lowering-adapter.md` and validated
with `tools/validate_evidence.py ... --require-pass`; its copy is
`docs/findings/evidence/M01-LC-DIRECTIVE-LOWERING.json`. Its `review.method`
first carried the world-side fact limit as a **limit on the claim** — affected
content, resolving task and gating — rather than as an `unknowns` entry, and
since #751 it carries the world-side fact **writers** in the same slot: the
resolved task, what a host still has to observe before each of the 24 blocks
can complete, and the standing refusal of any `verified_original` claim. This
report's claim is `implemented` about the *lowering*, and `unknowns` is the
repo's slot for unresolved **measurements** of that claim (see
`M01-LC-MISSION-PROGRAM.json`), which is why `--require-pass` requires it
empty.

## Status and limits

* `implemented` only: this task awarded itself no `checked`, no
  `verified_original` and no `release_approved`.
* Affected content and what still blocks a fidelity claim: the 24 blocks
  above now have production writers (#751) but still need a host that observes
  their world state before they complete — `docs/findings/2026-10-08-m01-lc-world-facts.md`
  names that per condition — plus the residual unknowns named by findings A–D.
  Resolving tasks: **VS-M01-RUNTIME (#359)** for the launch path that will
  supply those observations, and the stage findings for any residual a later
  reading corrects.
* `docs/findings/2026-10-07-m01-lc-directive-lowering-adapter.md` remains the
  record of stage `.03`; this document adds only the runtime seam and the
  follow-up.
