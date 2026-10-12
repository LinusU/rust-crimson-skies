# M01-B-WAKE: the world facts that wake M01's dormant `Finish` blocks

Date: 2026-10-12. Task: #1357 `M01-B-WAKE` (parent #1355 `M01-B`, itself under
`M01-B`'s mission-specific compatibility gaps). Test prefix `accept_m01_b_wake_`.
Capabilities used: `retail` (read of `$CS_GAME_DIR`) plus the measured objective
semantics of M01-B-OBJ-1/2/3/4. **No original executable was run**; nothing here
is `verified_original`. Every graph edge and gate below was dumped from
production reads of the owner's installation — `recover_retail_objectives` on
`zbd/c1c/m01` for the record's engine declarations and the
`survey_mission_control_programs` lowering for the gate conditions — and the
`accept_m01_b_wake_` suite drives the real program through the production host.

Follow-up of `docs/findings/2026-10-11-vs-m01-rt-mission-host-integration.md`'s
known limit: in a headless run M01's real program reaches no terminal, because
both `Finish` blocks start dormant behind the `-1` sentinel and every wake path
needs world facts a headless run does not have. This note names each path and
the fact it needs, and the providers now wired into `MissionHost::step`.

## The two terminals

M01 loads 58 objectives (`OBJECTIVE1` … `OBJECTIVE58`; number *N* is zero-based
index *N−1*). Its only outcome blocks:

| Block | Index | Outcome | Load state |
| --- | --- | --- | --- |
| `OBJECTIVE24` | 23 | `INSTANTWIN` → `Action::Finish(Succeeded)` | `BEGIN_DORMANT -1 -1 -1 -1` |
| `OBJECTIVE40` | 39 | `INSTANTLOSS` → `Action::Finish(Failed)` | `BEGIN_DORMANT -1 -1 -1 -1` |

`BEGIN_DORMANT -1` disarms all four measured timer slots, so neither block ever
wakes on a timer. Census of every block's `BEGIN_DORMANT`: exactly one block
carries an armed self-wake — `OBJECTIVE1`, `wake_at = 2` — and no block anywhere
spells `WAKE_OBJECTIVE_WHEN_I_SLEEP` or an armed `nap_after`/`sleep_at` slot.
Every other wake in the record is a completion effect (`WAKE…`, `NAP…`,
`KILL…`, `SLEEP…`) applied when some other block completes.

Five blocks load awake (no `BEGIN_DORMANT`), and each one's non-lifecycle gate
is a world fact — these are the roots of the whole graph:

| Block | Gate beyond `ObjectiveAwake` |
| --- | --- |
| `OBJECTIVE2` | `DEDG [1, 0]` — enemy group 1 living ≤ 0 |
| `OBJECTIVE3` | `TRAVELERS player APPROACHING workersvoyagezep 700` |
| `OBJECTIVE6` | `INACTIVE` — ≥ 1 of `workersvoyagezep`'s 18 engine-part chains out of play |
| `OBJECTIVE7` | `INACTIVE` — ≥ 1 of `workersvoyagezep`'s 9 turret chains out of play |
| `OBJECTIVE36` | `INACTIVE` — ≥ 2 of `piratezep`'s 12 engine-part chains out of play |

Every dormant block the program can ever reach descends from one of these five
(or from `OBJECTIVE1`'s lone 2-second self-wake), and each of the five needs a
world fact to complete — so a run whose world reports nothing cannot start any
chain. That is the measured proof of the integration note's "every wake path
needs world facts a headless run does not have".

## The path to `OBJECTIVE24` (Succeeded)

Solid arrows are `WAKE`/`NAP` completion effects; the `NAP` edges carry their
record seconds. `6` and `36` load awake, everything below them dormant.

```
OBJECTIVE6  ──WAKE──▶ OBJECTIVE8 ──WAKE──▶ OBJECTIVE9 ──NAP 15s──▶ OBJECTIVE10
OBJECTIVE10 ──WAKE──▶ OBJECTIVE49 ──NAP 5s──▶ OBJECTIVE50 ──WAKE──▶ OBJECTIVE11
OBJECTIVE11 ──NAP 15s──▶ OBJECTIVE12 ──NAP 15s──▶ OBJECTIVE13 ──NAP 30s──▶ OBJECTIVE14
OBJECTIVE14 ──NAP 30s──▶ OBJECTIVE15 ──NAP 3s──▶ OBJECTIVE16 ──NAP 10s──▶ OBJECTIVE17
OBJECTIVE17 ──WAKE──▶ OBJECTIVE18 ──WAKE──▶ OBJECTIVE24 ──▶ Finish(Succeeded)
```

The world facts each rung gates on (`ObjectiveAwake` lifecycle gates omitted —
every block also requires itself awake):

| Block | World-fact gate | Provider family |
| --- | --- | --- |
| `OBJECTIVE6` | ≥ 1 of 18 `workersvoyagezep reng*/leng* healthy` member chains out of play | member presence (`INACTIVE`) |
| `OBJECTIVE8` | ≥ 5 of the same 18 | member presence |
| `OBJECTIVE9` | ≥ 9 of the same 18 | member presence |
| `OBJECTIVE10`, `49`, `50`, `12`, `13`, `14` | none — pure latch hops | lifecycle only |
| `OBJECTIVE11` | `ANIM_STATE wv_drop_copilot RUNNING` | animation states |
| `OBJECTIVE15` | `ANIM_STATE wv_pickup_copilot EXECUTED` | animation states |
| `OBJECTIVE16` | `DEDG [1, 0]` — group 1 living ≤ 0 | group living counts |
| `OBJECTIVE17` | `DEDG [2, 0]` — group 2 living ≤ 0 | group living counts |
| `OBJECTIVE18` | `ANIM_STATE hooked_to_klondike EXECUTED` | animation states |
| `OBJECTIVE24` | none — `ObjectiveAwake(23)` then `Finish(Succeeded)` | lifecycle only |

The `NAP` re-wakes on the path cost 15 + 5 + 15 + 15 + 30 + 30 + 3 + 10 = 123 s
of mission clock. `OBJECTIVE17` also wakes `OBJECTIVE25`, `49` and `58` and naps
`57`; those are side branches off the terminal path.

## The path to `OBJECTIVE40` (Failed)

```
OBJECTIVE36 ──NAP 15s──▶ OBJECTIVE37 ──NAP 15s──▶ OBJECTIVE38 ──NAP 15s──▶ OBJECTIVE39 ──NAP 15s──▶ OBJECTIVE40 ──▶ Finish(Failed)
```

Four thresholds climb the same 12 `piratezep` engine-part chains
(`reng11`…`reng32`, `leng11`…`leng32`, all `healthy`): 2, 6, 10, 12 of 12.
`OBJECTIVE39`'s completion then re-wakes `OBJECTIVE40` after 15 s, whose only
gate is `ObjectiveAwake(39)` before `Finish(Failed)`. No animation or group
fact is on this path — member presence and the lifecycle model alone.

## The providers implemented

All wiring lives in `crates/cs_app/src/mission_session/` (owner paths), with the
fold signature in `world_facts.rs` adjusted to carry whichever model the stage
runs:

* **`MissionHostSeed::engine_objectives: Option<EngineObjectives>`** — the
  recovered lifecycle model (M01-B-OBJ-1/2), read in `compose.rs` from the same
  `ObjectiveRecovery` the declared program comes from. A record whose
  declarations cannot build the model (the engine's own range check) is named
  by `MissionHostRefusal::EngineObjectives`, never silently dropped.
* **`ObjectiveLifecycles`** (`mission_session/lifecycles.rs`) — the
  `objectives` fact owner. It runs the recovered `EngineObjectives` when the
  stage carries one, else the declared `BlockLifecycleTable`, else the empty
  fail-closed read `MissionHostRefusal::BlockLifecycles` names. The engine's
  `+0x5c8` states project onto the runtime vocabulary (`Dormant`/`Awake`/
  `Napping`/`Asleep` → `Dormant`/`Awake`/`Napping`/`Done`); a `KILL`ed
  objective publishes no row at all.
* **The measured per-tick order in `MissionHost::step`**: the world
  observation is folded first, then `ObjectiveLifecycles::advance(elapsed)`
  (the engine's timer pass), then `MissionSession::advance` (the completion
  pass), then `ObjectiveLifecycles::complete(index)` for every
  `ObjectiveCompleted` event — so a block the timer pass woke this tick can
  complete this tick, and each completion's `WAKE`/`NAP`/`KILL`/`SLEEP`
  effects apply in the engine's own order. A paused frame advances no mission
  clock. The tick report (`MissionHostTick::lifecycles`) records what the pass
  did: woken, napped, slept, completed.
* **`MissionWorldObservation`** — a `Resource` port written by the world's
  observation system once per step and *consumed* by the step (a report lives
  exactly one tick). It carries `WorldObservation` member rows
  (`MemberObservation::no_longer_in_play` → the `INACTIVE` counts), group
  living counts (→ `DEDG`) and animation rows (→ `ANIM_STATE`). The host
  merges it with what it observes itself — the record player's running and
  finished animation records, folded under each record's startup identity —
  before `WorldFactTable::observe`. A step with no report folds the members it
  could not see as absent, which `MissionHostRefusal::WorldObservation`
  already names.

No gate was relaxed, no terminal requested: `INSTANTWIN`/`INSTANTLOSS` still
arrive only through the program's own `Action::Finish`.

## What the acceptance suite proves

`crates/cs_app/tests/campaign/accept_m01_b_wake.rs` — 8 tests, all in the
`campaign` binary:

* `…_retail_m01_carries_the_recovered_lifecycle_model` — M01's stage carries
  the engine model, loads all 58 objectives, and both `Finish` blocks start
  dormant behind `-1`.
* `…_a_driven_run_reaches_instantwin_through_measured_facts` — a driven run
  whose world reports `workersvoyagezep` dead (18 parts out of play, groups 1
  and 2 at 0, the three animation rows) settles `Success` from
  `MissionTerminalSource::ControlProgram`, requested by `SymbolId(23)` — the
  record's own `Finish`, not a shortcut.
* `…_a_driven_run_reaches_instantloss_through_measured_facts` — the same for
  `piratezep`, `Failure`, `SymbolId(39)`.
* `…_an_unobserved_world_reaches_no_terminal` — no report: `OBJECTIVE1`'s 2 s
  self-wake fires (the positive control) and both terminals stay dormant.
* `…_a_missing_member_stalls_the_chain_at_its_threshold` — 11 of 12
  `piratezep` parts: `OBJECTIVE36/37/38` complete, `OBJECTIVE39` wakes and
  honestly stays awake, `OBJECTIVE40` never wakes.
* `…_removing_the_lifecycle_model_blocks_the_effects` — without
  `engine_objectives`, `OBJECTIVE6` really completes but its `NAP`/`WAKE`
  effects never land; `OBJECTIVE8` stays dormant forever.
* The two synthetic legs (CI, no installation): a 3-block chain settles
  `Success` through the engine model's `NAP` re-wake and `WAKE`, and with the
  model removed dies at the first dormant block.

## Unknowns carried

These are inherited from the measured directive findings and stay open —
none is faked by the providers, which report facts only when observed:

1. **In-play-bit writers are untraced** (`IN_PLAY_BIT_WRITERS_UNTRACED`): what
   clears a member's in-play bit is spawn/despawn world code outside the
   measured bound; the `INACTIVE` evaluator itself is measured. The provider
   surface accepts the fact when the world reports it and answers fail-closed
   when it does not.
2. **`DEDG`'s member-field rewrites** (`DEDG_MEMBER_FIELD_REWRITES`): the
   original's evaluator rewrites three member fields per counted member; the
   lowered condition carries the residual and this runtime does not reproduce
   the rewrite.
3. **`TRAVELERS`' producers** — `OBJECTIVE3`'s proximity gate (player within
   700 of `workersvoyagezep`) is off both terminal paths; the fact family is
   measured at the consumer, unproduced at the world side today.
4. **`TICK_DEPENDS_ON_OBJ` roots** — `OBJECTIVE42/43` gate on `OBJECTIVE54`
   awake and `OBJECTIVE57` on `OBJECTIVE56`; those chains are likewise
   dormant-rooted world-fact consumers, off the terminal paths.
5. **Whether the engine's behavior matches the measured model when run** —
   static reading plus the synthetic and driven-headless proofs here; no
   original run exists.

## Evidence

`docs/findings/evidence/M01-B-WAKE.json` records the acceptance run, the
installation fingerprints and `wake-graph.json`, the production census of the
graph this note summarizes (rendered by
`crates/cs_app/tests/evidence_report_m01_b_wake.rs`).
