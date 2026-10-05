# F29-C.4: the pilot bailout's distinct mission transition

Date: 2026-10-05. Task: F29-C.4 "Wire PilotBailout into its distinct mission
transition" (Rally #520), the re-homed slice of F29-C
(`specs/F29-damage-zones-armor-destruction-and-bailout.md`, section `### F29-C`,
acceptance case **AC04**: "Bailout and ordinary death trigger the correct
distinct mission transitions", with non-negotiable behaviors 3 and 4). Shared
contract: `docs/contracts/STATE-TRANSACTIONS.md`.

Task test prefix: **`accept_f29_c_04_`** (7 tests: 6 in
`crates/cs_sim/tests/accept_f29_c_04_bailout_mission_transition.rs`, plus 1 in
`crates/cs_app/tests/accept_f29_c_04_bailout_session_consumer.rs` added during
review to cover the consumer's dispatch). The re-homed suffix follows
`docs/TASK-SPLITTING.md`'s convention for a split stage
(`F38-B.01` → `accept_f38_b_01_`), and it nests inside the sheet's own
`accept_f29_c_` prefix so the F29-C suite still selects it.

Capabilities used: ordinary build/test **plus a bounded read-only `retail`
census** of the owner's installation for the vocabulary measurement below. No
render, no audio, no original run, so no `private/evidence/` acceptance report is
produced by this task — see "Evidence" for why the claim cannot exceed
**checked**.

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/objectives/bailout.rs` (**new**): `BailoutResultPolicy`,
  `BailoutConfirmation`, `MissionTransition`, `MissionTransitions`,
  `TransitionOutcome`, `AppliedTransition`, `BailoutRefusal`,
  `BAILOUT_RESULT_POLICY_UNMEASURED`. Module docs carry the measured/unmeasured
  split.
- `crates/cs_sim/src/objectives/runtime.rs`: two new
  `ObjectiveEventKind` variants (`PilotBailedOut`, `TransitionRefused`), the
  `transitions` field, `confirm_bailout` / `mission_transition` /
  `bailout_policy`, phase 1 rewritten to consult the ledger, and the event
  budget term raised from "one per *countable* transition" to "one per
  transition".
- `crates/cs_sim/src/objectives/mod.rs`: module declaration and docs (wiring).
- `crates/cs_app/src/objectives.rs`: the session consumer's dispatch — the two
  new event kinds, and `SessionRefusal::Transition` so a refusal reaches a
  consumer that does not want to re-walk the stream.
- `crates/cs_sim/tests/accept_f29_c_04_bailout_mission_transition.rs` (**new**).
- `crates/cs_app/tests/accept_f29_c_04_bailout_session_consumer.rs` (**new**, in
  review: the consumer dispatch this task added in `cs_app`, which no test
  covered).
- This file.

No protected path, no `Cargo.toml`, no binary, no original data committed.

**One observable failure, before the change.** F29-C's own finding listed
"bailout mission transition" as an open follow-up: *"Bailout and ordinary death
trigger the correct distinct mission transitions"* had no consumer.
`ObjectiveRuntime::apply_counters` folded each `LifecycleKind` through
`CountKind::from_lifecycle`, which answers `None` for `PilotBailout` — so a
pilot bailing out **emitted no event at all**. Every other vocabulary already
had a consumer and a refusal: the targeting store keeps a bailed-out airframe
eligible (F30-A), the mission fact table writes no actor state for it (F37-E1),
the counters do not count it (F39-E4). The mission layer alone had no answer,
and no way for a program to ask what a bailout did.

The second, sharper half of the gap: nothing kept the *first* report. "Not
counted" is not "cannot be counted later". With no latch, an airframe whose pilot
left could still be reported destroyed on a later tick and be counted as a kill.

## The designed behavior

`MissionTransition` has exactly the two variants the sheet separates, and
`MissionTransition::from_lifecycle` is the only place a `LifecycleKind` becomes
one. The other three kinds deliberately get `None` here — capture and despawn
are the counted categories F39-E4 measured, mission removal is F37-E1's
accounting — so this module never becomes a second, unmeasured answer to a
question another module already owns.

`MissionTransitions` is the per-actor **latch**, and it is why the AC04 case is
answerable at all:

| reported | actor's record | result |
| --- | --- | --- |
| `Destroyed`, no prior | empty | `Applied(Destroyed)`, counted once |
| `PilotBailout` + confirmation | empty | `Applied(PilotBailedOut)`, **never counted**, reported with what confirmed it |
| `PilotBailout`, no confirmation | empty | `Refused(Unconfirmed)` — a rendering parachute is not a confirmation |
| either, repeated | holds it | `Repeated`, silent, as the once-per-category counters already are |
| the *other* one | holds the first | `Refused(AlreadyTransitioned{requested, kept})`, naming both |

The confirmation gate is `BailoutConfirmation`: a player eject is the declared
`FlightCommand::Eject` edge under the **production** `InputContext::accepts`
predicate (F22's gate, reused rather than reimplemented), so a cinematic or a
text-entry context cannot confirm an ejection; a mission program's own
`Scripted` request needs no local input. `confirm_bailout` validates before it
writes, and `observe` re-asks the gate — every field is public, so a struct
literal cannot carry an ungated context past it.

`BailoutResultPolicy` is the declared mission-result policy, and it has **one**
variant, `Unmeasured`, which requests **no** terminal outcome and credits **no**
survival. Phase 1 asks the policy once per applied bailout and pushes what it
gets; today that is nothing, and that is the seam a measured rule fills without
editing the phase. A measured rule becomes a *new named variant* — the same shape
as `TerminalPrecedence::SyntheticConservative`, which the contract permits for
synthetic tests only until verified.

The consumer side (`cs_app::objectives`) reports the bailout and changes
nothing: the airframe is still in the world (F30-A's measured contract is that a
bailout does not end targetability), so it stays in the wave registry and stays
this session's to tear down.

## Measured on the owner's installation: the vocabulary, and nothing more

A bounded read-only `strings` census (no file written, no archive decoded):

| what | where |
| --- | --- |
| ejected pilot object | `zbd/zrdr.zbd` → `..\data\common\zrdr\objects\pilot_eject.zrd` |
| parachute | `..\data\common\zrdr\objects\chuteman.zrd`, nodes `chuteman`, `chutemanparent` |
| animation | `cpilot_eject.zan` / `cpilot_eject2.zan`, cues `cpeject1`, `cpeject2`, `cpejectstop`, driving `OBJECT_MOTION_SI_SCRIPT` on a `cpilot` child of `player`/`pilot_pos` |
| sounds | `snd_pilot_eject1` → `pilot_eject1.wav`, `snd_chuteopen` → `chuteopen.wav` |
| voice | `VO_id1_DA-Bail-A/B`, `VO_id1_DA-NoBail-A/B`, per-wingman `vo_*_DE-Bail-A.wav` |
| mission node name | `zbd/C2/MP2/mis_anim.zbd` (and siblings) carry a bare `eject` node |
| camera | every `zbd/C*/cam_anim.zbd` references `pilot_eject.zrd` and `cpilot_eject.zan` |

**What this measures:** an ejection exists as authored content, with a parachute,
a camera treatment and sounds — so it is not a guess. And the original's own
radio script distinguishes *bailing out* from **not** bailing out
(`DA-NoBail`), which independently supports AC04's premise that the two events
are not interchangeable.

**What this does not measure, and is not claimed:** anything about what a bailout
does to a *mission*. No `OBJECTIVE<N>` key in any reader archive's
`objectives.zrd` spells an ejected-pilot category or a bailout outcome (F39-E4's
census measured the five counted categories; none covers a pilot who is alive),
the compiled program behind those records is undecoded (F13-B/C, F38), and no
original executable was run in this project. A name is not a rule and a `.zan`
file is not a mission result. So the policy is `Unmeasured` and **withholds**:
no success, no failure, no extraction, no survival credit — and the original's
rule is not inverted into a plausible one. **Affected content:** every airframe a
mission can lose a pilot from, and every mission that would score such a loss.
**Resolving task:** F29-D (#108), with `retail` for the records and the owner's
`human_review` for the result question itself — an agent can measure what a
mission declares; it cannot measure what the game did with it.

## Tests

Every test drives production code: the real `DamageResolver` records the
transition and emits the `DamageEvent`s a mission host would consume, its
`Lifecycle` events are read back and handed to the real `ObjectiveRuntime` as
`TickInput::lifecycles`, and the runtime's own ledger decides. No test-only
bridge. The world's `cs_types::net::ActorId` → mission `cs_script::ir::ActorId`
mapping is performed explicitly in the test, because that identity bridge is the
host's surface (`cs_sim::mission` says so in the same words) and hiding it in a
helper would assert a mapping production code does not have.

| test | what it pins |
| --- | --- |
| `accept_f29_c_04_a_confirmed_bailout_fires_the_bailout_transition_and_not_a_kill` (minimum) | a confirmed bailout produces `PilotBailedOut { actor, confirmation }`, no `Counted`, `counted(Destroyed) == 0`, the protected roster's failure condition never latches, `outcome()` stays `None`, and `PilotBailedOut.is_kill()` is false |
| `accept_f29_c_04_an_ordinary_kill_fires_the_destruction_transition_and_not_a_bailout` | a real lethal hit through the resolver produces `Counted`, latches the condition, fails the mission, and carries no bailout transition and no refusal |
| `accept_f29_c_04_a_later_destruction_cannot_turn_a_bailout_into_a_kill` | a later real destruction is `Refused(AlreadyTransitioned{requested: Destroyed, kept: PilotBailedOut})`, counts nothing, grants nothing — **and the mirror**: a destruction first makes `confirm_bailout` refuse, and the later bailout is refused too, so the kill stands |
| `accept_f29_c_04_a_bailout_without_a_confirmed_input_is_refused_by_name` | no confirmation → `Refused(Unconfirmed)` and no event; `Cinematic` context → `InputContextRefused` while `Flight` is admitted; `FirePrimary` → `NotAnEjectCommand`; a UI action → `NotAnEjectEdge`; a `Scripted` request is a confirmation and appears in the reported event |
| `accept_f29_c_04_the_unmeasured_bailout_policy_grants_no_result_and_no_survival` | the policy is `Unmeasured`, `is_measured()` false, `reason()` names the absence, `terminal_outcome()` is `None`, `grants_survival()` false; a bailout settles nothing and no stream event claims otherwise; a bystander's kill still counts |
| `accept_f29_c_04_the_ledger_is_the_one_place_the_transition_is_decided` | `from_lifecycle` is total over the two kinds and `None` for the other three, round-trips through `lifecycle()`, and covers `MissionTransition::ALL`; an unconfirmed bailout latches nothing; a confirmed one applies once, repeats silently, consumes its confirmation and refuses a second; the ledger is per actor |

The same prefix also selects one test in the **consumer** layer,
`crates/cs_app/tests/accept_f29_c_04_bailout_session_consumer.rs`, added in
review: it drives a launched `ObjectiveSession` over the declared F39-C
fixture, pins that a refused transition is reported as
`cs_app::objectives::SessionRefusal::Transition` rather than dropped, that the
airframe stays in the live registry, and that the refusal shields nothing — a
real destruction afterwards still counts as the actor's one transition.

## Mutation probes

Each probe edited one production line, ran the `accept_f29_c_04_` selection,
recorded the failing tests, and restored the file from a saved copy.

| probe | edit | result |
| --- | --- | --- |
| the wiring is removed | `apply_counters` reverted to folding raw facts into the counters (the pre-task behaviour) | 4 of 6 failed: the minimum, the ordinary kill, the later-destruction case, and the unconfirmed case — while `accept_f39_b_objective_runtime` (21 tests) stayed green, so the failure is this task's wiring and not the counters' |
| the latch keeps the last report | `MissionTransitions::observe`'s "already holds a transition" branch disabled | 2 failed: the later-destruction case and the ledger test |
| the policy grants success | `BailoutResultPolicy::terminal_outcome` returning `Some(TerminalOutcome::Success)` | 3 failed: the policy test, the minimum, and the later-destruction case |
| the input-context gate is bypassed | `BailoutConfirmation::admits` returning `Ok(())` | 1 failed: the confirmation test |
| the consumer drops the refusal | `cs_app::objectives`'s `TransitionRefused` dispatch arm replaced with `{}` | 1 failed: the session-consumer test (`refusals` came back `[]`) |

Note on the last probe: disabling *only* `confirm`'s gate call
(`confirmation.admits()?` → `let _ = confirmation;`) left all 6 green, because
`observe` re-asks the gate. That redundancy is deliberate — the public fields mean
a struct literal is constructible — and the probe result is recorded here rather
than hidden.

## What is NOT claimed

No original-fidelity claim of any kind. The wiring, the latch and the
withholding are this engine's **designed** behavior; the retail census above is a
vocabulary measurement and bounds only what the original *names*. F29-D (#108)
validates AC04 against original data and needs `retail`; the mission-result
question additionally needs the owner's `human_review`, which no agent can
supply. Until then no mission may claim that a bailout ends, continues or scores
anything as the original did.

## Follow-ups left open (filed with `create_tasks`, not fixed here)

- **The kill award in the damage domain.** `DamageEventKind::KillAwarded` is
  emitted by the resolver from the lethal hit, not from the mission layer, so a
  bailed-out airframe shot down later still produces a resolver-side award even
  though the mission counts nothing. Reconciling the two is the scoring follow-up
  F29-C already named, and its consumer lives outside this task's owner paths.
- **The host that calls `confirm_bailout`.** `ObjectiveRuntime::confirm_bailout`
  is the declaration gate, and `cs_app`'s session forwards the stream, but no
  Bevy system yet reads `ControlBuffer`'s eject edge and calls it. Two things
  are missing, and the second is easy to miss: nothing reads the edge, **and**
  `ObjectiveSession` exposes only `runtime() -> &ObjectiveRuntime`, so a host
  holding the session has no mutable path to its runtime and cannot confirm a
  bailout through it at all. That producer surface is F39-C's ordinary-flight
  pass (F29-C.6, Rally #674), not this slice's. Until it exists, a bailout
  reported to a launched session is refused as `Unconfirmed` — which is the
  gate working, and is pinned by
  `crates/cs_app/tests/accept_f29_c_04_bailout_session_consumer.rs`.
- **The parachute visual.** `chuteman` is authored content and this engine has no
  consumer for it; nothing here depends on it, which is the point of the
  confirmation gate.

## Evidence

Synthetic fixtures plus a bounded read-only `retail` vocabulary census. No
original-data, visual, audible or ordinary-play claim. This stage can award at
most **checked**; `verified_original` remains out of reach until F29-D measures
the rule and the owner reviews it.

## Sources

- `specs/F29-damage-zones-armor-destruction-and-bailout.md` (`### F29-C`, AC04,
  non-negotiable 3 and 4), `docs/contracts/STATE-TRANSACTIONS.md`,
  `docs/contracts/SCRIPT-MISSION.md` ("Objective event ordering").
- `docs/findings/2026-10-02-f29-c-damage-consumers.md` — the open follow-up this
  task closes, and the refusal-log pattern its consumer follows.
- `docs/findings/2026-10-04-f39-e4-count-category-producers.md` — why a bailout
  counts toward none of the five categories.
- `docs/findings/2026-09-29-f22-a-command-schema-and-action-map.md` and
  `crates/cs_types/src/input.rs` — `FlightCommand::Eject` and the
  `InputContext::accepts` gate this task reuses.
- `crates/cs_sim/src/net_state.rs` — the existing first-terminal-report rule the
  per-actor latch mirrors.
- `crates/cs_sim/src/{damage,mission,objectives}/*.rs`,
  `crates/cs_app/src/objectives.rs`.