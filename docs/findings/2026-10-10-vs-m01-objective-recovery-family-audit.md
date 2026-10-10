# VS-M01-OBJECTIVE-RECOVERY: which `RecordFieldFamily` reasons are stale, and what still blocks an M01 `DeclaredObjectiveProgram`

- **Task:** #1219 `VS-M01-OBJECTIVE-RECOVERY` — "Recover M01's objectives.zrd fields into a DeclaredObjectiveProgram the host can launch"
- **Measured by:** `bunny-alpha-1`, 2026-10-10, on `origin/main` `5b2fad4f`
- **Capabilities used:** `retail` (read-only `$CS_GAME_DIR` through production code). No original executable was run; every semantic claim below is either a production-parse measurement over the owner's installation or a citation of the M01-LC-DIRECTIVE stages' static disassembly of `$CS_ENGINE_IMAGE`. Nothing here is `verified_original`.

This note is task #1219's step-1 deliverable: the per-family audit of
`crates/cs_app/src/objectives.rs` `RecordFieldFamily::reason()` against the
measurements that landed after those strings were written (F39-E1/E2/E5/E6, then
M01-LC-DIRECTIVE-A/B/C/D of #675). It ends in a **block**: the audit shows the
refusal text is substantially stale, but no wiring inside this task's owner paths
can make `ObjectiveRecovery::program()` return `Ok` for M01 that
`objectives::lower_program` accepts. The blockers and the evidence that would
clear each one are named at the end.

## The measured M01 census this audit rests on

Production run over the owner's installation
(`recover_retail_objectives(&root, "zbd/c1c/m01")`, `CS_GAME_DIR` set):
**58 numbered blocks, 358 fields, 0 recovered today** — the walk and the
existing retail test (`accept_m01_lc_objectives_retail_m01_drops_no_record`)
agree with #1214's note. Per-family field counts (probe over the production
walk, counted by `RecordFieldFamily`):

| family | M01 fields | keys |
| --- | --- | --- |
| `CompletionEffect` | 56 | `NAP_OBJECTIVE_WHEN_I_COMPLETE` 27, `WAKE_OBJECTIVE_WHEN_I_COMPLETE` 20, `KILL_OBJECTIVE_WHEN_I_COMPLETE` 9 |
| `InactiveStage` | 140 | `INACTIVE1`…`INACTIVE18` (12/10×7/8×3/4×6) |
| `Dormancy` | 52 | `BEGIN_DORMANT` |
| `SoundCue` | 41 | `COMPLETED_SOUND_GROUP` 23, `WAKEUP_SOUND_GROUP` 18 |
| `Unmeasured` | 50 | 19 keys, see below |
| `CompletionCount` | 9 | `INACTIVE_COMPLETION_COUNT` |
| `Identity` | 5 | `IDENTITY` |
| `OrderDependency` | 3 | `TICK_DEPENDS_ON_OBJ` |
| `Outcome` | 2 | `INSTANTWIN` 1, `INSTANTLOSS` 1 |
| `Optionality` | 0 | (unreachable — see the last section) |

F39-E2's branch-precedence reading for M01, re-derived by the same run:
**15 multi-effect blocks, all 15 with disjoint target sets, 0 conflicts, 0
repeated effect keys** — so the schema's `AmbiguousCompletionEffect` and
`RepeatedCompletionEffect` refusals never trigger on M01.

The `Unmeasured` family's 50 M01 fields: `DEDG` 8, `WAKE_ANIM` 5,
`ADD_OBJECTIVE_TARGET` 4, `REMOVE_OBJECTIVE_TARGET` 4, `WAKEUP_GENERATOR` 4,
`ADD_OTHER_TARGET` 3, `ANIM_STATE` 3, `SET_AI_NET` 3, `STOP_QUEUED_SOUNDS` 3,
`WAKEUP_ZEP_TURRETS` 3, `SET_HELP_LABEL` 2, and one site each of
`COMPLETED_STOPPOINT`, `EXECUTE_ANIMS`, `INVALIDATE_ANIMS`, `MISSION_TIMER`,
`PLAYER_INIT`, `RESTORE_ANIMS`, `TRAVELERS`, `WAKEUP_ENEMIES`.

## The per-family verdict

"Stale" means the `reason()` text states something the later measurements
refuted or answered. It does **not** by itself mean the family can be wired into
a `DeclaredObjectiveProgram` — the carrier column is the second gate.

| family | old reason | verdict | measured evidence | carrier in `DeclaredObjectiveProgram`? |
| --- | --- | --- | --- | --- |
| `CompletionEffect` | "what the effect does is an inference, and precedence between effects is unmeasured (F39-E2, E5, E6)" | **stale** | M01-LC-DIRECTIVE-B (#680) measured the whole completion pipeline: wake array walk `0x469af0` (cap 15, skip killed/completed, early-terminate on an already-awake target) at step 7, `NAP` at step 21 (target → state 2, `+0x5d8 = +0xd4` seconds, **clears the target's completed flag `+0x14`**), `KILL` at step 22 (`+0x8 = 0`, stops all processing). The same-tick effect order F39-E2 left open is now measured: wake, then nap, then kill, in that pipeline order. Nap's number is measured as **seconds** (state-2 auto re-wake), refuting `UnmeasuredQuantity`'s "no measured unit" for this key. | yes — `DeclaredCompletionEffect` + `BranchEffectKind`; M01 raises none of the schema's per-block refusals (0 conflicts, 0 repeats, and — per the same census — every target names a declared block, `is_closed_over_its_record`). |
| `OrderDependency` | "the objective it sequences behind is named but the ordering rule is undecoded (F39-E2)" | **stale** | DIRECTIVE-B measured both passes: the dependent objective runs no timers and evaluates no conditions unless `dep->+0x5c8 == 1` (the dependency is *currently awake*); a completed dependency freezes the dependent for the rest of the mission. The ordering rule is measured. | **no** — `DeclaredObjective` has no tick-dependency field; a carrier needs a `cs_content` schema addition **and** an `ObjectiveSpec` counterpart in `cs_sim`, which is outside #1219's owner paths. |
| `Outcome` | "the terminal precedence between win and loss is unmeasured (F39-D)" | **stale** | DIRECTIVE-B: `+0x554 == 3` (INSTANTWIN) → mission WON on completion, checked **inside** the not-ended guard; `== 4` (INSTANTLOSS) → mission LOST, checked **outside** it, so a loss still fires on an already-ended mission while a win is skipped — a measured loss-beats-win precedence — plus the post-pass all-WON→won / all-LOST→lost aggregation. | partially — `on_complete: Requests(Success/Failure)` carries the outcomes, but lowering requires `Resolved::Known(DeclaredPrecedence)`, and the only precedence variant that exists is `SyntheticConservative` (designed, synthetic-only); a measured variant needs a `cs_sim` `TerminalPrecedence` counterpart — outside owner paths. |
| `Dormancy` | "the unit of the argument and the reveal rule are unmeasured (F39-E1)" | **stale on the unit, open on the mapping** | DIRECTIVE-B: `BEGIN_DORMANT` presence sets `+0xc = 0`/`+0x5c8 = 0`; child0 writes `+0x5d0`, the **mission-clock second** at which the block self-wakes (`this+0x6f0 += frame dt`); the measured `-1` sentinel disables the timed wake (dormant until a `WAKE_*` list names it); children 1–3 are measured in code but never spelled in M01 (all 52 M01 sites spell exactly one float). F39-E1's two populations (positive vs sentinel) are thereby explained, not just counted. | open — the original's 4-state lifecycle (dormant/awake/napping/done) does not map one-to-one onto the seven declared states, and F39-E1 measured that display identity is **independent** of dormancy (86 corpus blocks are both dormant and identity-carrying), so "dormant ⇒ hidden" is refuted; choosing the initial state + reveal rule per block is a design inference, not a measurement. |
| `InactiveStage` + `CompletionCount` | "what satisfying the named actor/part/attribute condition means is unmeasured (F39-E1, E7)" / "reachability and monotonicity unmeasured" | **partially stale** | DIRECTIVE-B measured the evaluator (`0x469a60`): count the resolved member handles whose object exists and whose `+0x24` **bit 4 is clear**, fire iff `count >= +0x55c` (default = number of rows). What *clears* the bit is **not** measured — DIRECTIVE-B unknown #3: the world code that sets/clears the in-play bit and the despawn byte is untraced (producers outside the bound). | **no** — two structural gaps: the conditions are **member/part-granular** (`geminizep`/`reng11`/`healthy` chains resolved through `0x4cc990`) while `DeclaredCondition`/`CountCondition` are **actor-granular** over five end-state categories, none of which is "in-play bit clear"; and F39-E4's schema gate refuses any category but `Destroyed` for an original record. |
| `Identity` | "the message ids are not resolvable to text and the reveal timing is unmeasured (F39-E1)" | **partially stale** | DIRECTIVE-B measured the consumers: class (`PRIMARY/SECONDARY/TERTIARY` → `+0x0`) picks the announcement channel and ordinal (`+0x4`) the HUD slot; child2 (the `MSG_*` id, spelled at 4 of M01's 5 sites) is **never read by the parser**, so identity does not drive reveal — that half of the old reason is answered. What remains: the ids are still unresolvable to text (F39-E1: the shipped headers define 0 `MSG_*`). | no — `DeclaredObjective` has no identity-class/ordinal carrier at all. |
| `SoundCue` | "when the cue is emitted is unmeasured (F39-E1)" | **stale** | DIRECTIVE-B/D: `+0x54c` (`WAKEUP_SOUND_GROUP`) is played **at the wake transition** (`0x46cc70`), `+0x550` (`COMPLETED_SOUND_GROUP`) **at completion** (`0x46cc50`); M01's four keys' full dispositions are in #680/#DIRECTIVE-D, including `STOP_QUEUED_SOUNDS` and `SET_HELP_LABEL` at completion. | **no** — the schema has no objective-level sound carrier (`DeclaredTimerAction::Cue` is timer-bound and needs a dialogue `ContentId`), and DIRECTIVE-D unknown #1 keeps the handle→sound binding unmeasured anyway. |
| `Optionality` | "what an optional declaration changes is unmeasured (F39-D)" | **unreachable in `of_key`** | `is_optional_objective_key` matches exactly `INACTIVE_COMPLETION_COUNT` and `INACTIVE<n>` — both classified earlier in `of_key` as `CompletionCount`/`InactiveStage` — so no key can ever classify as `Optionality`. M01 has 0 such fields, by the same probe. The variant is dead in the classifier, not merely unmeasured. | n/a |
| `Unmeasured` (19 M01 keys) | "no F39 stage measured this key; the mission-language instruction table is undecoded (F13-B/C, F38)" | **stale** | M01-LC-DIRECTIVE-A (#679) measured the objective-directive **parser table** itself from the executable; B/C/D measured the call sites of `DEDG`, `TRAVELERS`, `ANIM_STATE`, `WAKE_ANIM`, `WAKEUP_ENEMIES`/`GENERATOR`/`ZEP_TURRETS`, `ADD`/`REMOVE_*_TARGET`, `SET_AI_NET`, `SET_HELP_LABEL`, `STOP_QUEUED_SOUNDS`, `COMPLETED_STOPPOINT`, and the record-level keys (`MISSION_TIMER`: M01's `[0.0]` **never starts** the timer per DIRECTIVE-D's measured `> 0.0f` gate; `PLAYER_INIT`; the three anim lists are parser keys with no measured consumer). The control-lowering adapter (#726) then bound all 43 of M01's distinct keys into a validated `RawProgram` — 353/353 sites, `RetailControlRow::is_complete() == true` for M01. | **no, and none needed here** — these are world-effect directives, carried faithfully by the **control path** (`cs_script` `Action::Directive`); the declared-objectives schema has no carrier for them and was never meant to. |

## Why this does not add up to a wireable M01 program

Three independent blockers; any one of them is sufficient, and all three are
outside what #1219 may decide alone.

### 1. The support gate refuses every installation-origin program, wired or not

`lower_program` refuses a record whose `DeclaredSupport` is `Original`
**first, by name** — "the gate, not a warning" (F39-D). `support_for(origin)`
maps `Origin::Installation` to `Original` unconditionally, and no third variant
exists (`cs_content::objectives` greps clean for any reconstructed/recovered
variant). So even a fully wired recovery cannot pass acceptance today.

Making original-derived programs playable is exactly what
`docs/contracts/SCRIPT-MISSION.md` reserves to the owner: *"A handwritten
declarative compatibility reconstruction is permitted only after the owner
approves that method, it remains labeled reconstructed, and it reproduces all
measured branches and source-derived data."* F39-D's unknown #8 records the
same: a future reconstruction "needs its own variant rather than a widened
`Original`". An agent adding a playable variant would self-approve the very
fidelity claim the contract gates — AGENTS rule 8 forbids it.

### 2. The measured families that carry real semantics have no carrier reachable within owner paths

- `OrderDependency` (3 M01 fields): no field on `DeclaredObjective` / no
  counterpart on `cs_sim` `ObjectiveSpec` — `cs_sim` is not an owner path.
- `Outcome` (2 fields): the measured loss-beats-win precedence needs a measured
  `TerminalPrecedence` variant in `cs_sim` — not an owner path; reusing
  `SyntheticConservative` would stamp a designed policy on an original record.
- `InactiveStage` + `CompletionCount` (149 fields, the largest family): the
  evaluator is measured but its conditions are member/part-granular over an
  untraced "in-play bit", while the schema and runtime are actor-granular over
  five end-state categories; F39-E4's gate additionally refuses every category
  but `Destroyed` for an original record. Faithful lowering needs a new
  condition kind in `cs_sim` — not an owner path — plus the producer trace
  (blocker 3).
- `Identity` (5) and `SoundCue` (41): no carriers in the schema at all.
- The 50 `Unmeasured`-family fields: correctly carried by the **control
  path** (`MissionProgram`/`Action::Directive`, complete for M01), and by
  design absent from the objectives schema.

That leaves only `CompletionEffect` (56 fields) and `Dormancy` (52) as
wireable-shaped — and `Dormancy` only after a design decision (the
lifecycle→seven-state mapping) that F39-E1 deliberately left to a later,
owner-facing stage. Wiring two families while the other 250 fields stay
refused would not move `program()` off `Err` for M01 by one byte of contract:
the refusal is all-or-nothing by design (`fields_read() ==
fields_recovered() + unrecovered().len()`, `program()` refuses while anything
is unrecovered), and the existing retail test pins exactly that.

### 3. Genuinely unmeasured producers remain, and they gate the largest family

- **The in-play bit's writers** (DIRECTIVE-B unknown #3): the vehicle/zeppelin
  code that sets/clears `+0x24` bit 4 — i.e. what *makes* an `INACTIVE` member
  count — is untraced. Without it, even a member-granular condition kind could
  not be bound to any of the five count categories without a guess (AGENTS
  rule 4).
- **Sound-group handle → audible binding** (DIRECTIVE-D unknown #1): which
  sound a `WAKEUP_SOUND_GROUP`/`COMPLETED_SOUND_GROUP` name ultimately plays
  is measured nowhere in shipped files.
- **Identity `MSG_*` text** (F39-E1): 0 resolvable defines in the shipped
  headers; the id pairing lives in `strings.dll` and is unmeasured.

## What would resolve each blocker (for the owner / the follow-up tasks)

1. **Support variant:** an owner decision that DIRECTIVE-A/B/C/D's
   static-disassembly measurement qualifies as the "measured probe" the schema
   docs anticipate, plus an owner-authored task adding a `reconstructed`-labeled
   playable `DeclaredSupport` variant with its approval recorded — or an
   explicit ruling that M01's objectives are launched through the control path
   (`mission_control::RetailControlRow::lowering()`, already complete for M01)
   and `ObjectiveRecovery::program()` stays a refusal surface. #1214's note
   already frames this as the owner's two-ways-out decision.
2. **Carriers:** a task whose owner paths include `cs_sim::objectives`
   (`ObjectiveSpec`, `TerminalPrecedence`, the count-condition surface) to add:
   a tick-dependency field, a measured loss-beats-win precedence variant, and —
   if the INACTIVE semantics are to live in the objectives runtime — a
   member-granular or in-play-bit count kind.
3. **Producer trace:** a DIRECTIVE-style static probe of the vehicle/zeppelin
   spawn/despawn paths for the `+0x24` bit-4 writers (which transitions clear
   it: despawn only, or destruction too), which is what would let the INACTIVE
   evaluator be bound to a count category at all.

## What was done, and what was not

- Done: the step-1 audit above, re-derived from production code over the
  owner's installation; the branch-precedence figures for M01 re-measured.
- Not done, deliberately: no wiring, no schema change, no reason-string edit,
  no new tests. Partial wiring cannot satisfy #1219's acceptance (a program
  `lower_program` accepts) while blockers 1–3 stand, and editing the refusal
  strings without the wiring would leave production text describing a state
  nothing implements. `missions/bindings/M01.json`'s "objective graph" unknown
  is therefore **unchanged and still accurate**; the file is protected anyway.
- The note cited by the task description,
  `docs/findings/2026-10-10-vs-m01-rt-content-objectives-program-refuses-and-plan-premises.md`,
  is not on `main`; it lives on #1214's branch (commit `2ac316d2`) and was read
  from there.

## Commands

```sh
cargo test --locked -p cs_app --test accept_m01_lc_objectives_recovery -- --include-ignored   # 2 passed (baseline, unchanged)
cargo test --locked -p cs_app --test probe_m01_fields -- --include-ignored --nocapture        # scratch probe, not committed
```

## Sources

`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
`docs/contracts/SCRIPT-MISSION.md`, the F39-D/E1/E2/E4/E5/E6 findings, the
M01-LC-DIRECTIVE-A/B/C/D findings (`docs/findings/2026-10-06-m01-lc-directive-*.md`),
the M01-LC-DIRECTIVE-LOWERING finding (`2026-10-07-…-adapter.md`), #1214's
branch note (`2ac316d2`), and read-only `$CS_GAME_DIR` through
`cs_app::objectives`' production walk. No original executable was run and no web
source was consulted.
