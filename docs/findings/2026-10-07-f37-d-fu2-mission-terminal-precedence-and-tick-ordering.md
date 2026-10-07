# F37-D-FU2: the mission-terminal precedence and tick-ordering rules, declared and labelled

Date: 2026-10-07. Task: F37-D-FU2 "Decide and record the mission-terminal
precedence and tick-ordering policy against an original observation" (`#589`),
the follow-up F37-D (`#140`) filed at the end of its stage. Spec stage:
`specs/F37-mission-ir-and-deterministic-runtime-core.md` (`### F37-D`).
Shared contract: `docs/contracts/SCRIPT-MISSION.md`. Required capability:
ordinary build/test; the machine reports `retail`, `gpu` and `audio` and
**none was used** — this stage reads no original data at run time, renders
nothing and plays nothing, so no `private/evidence/` report is produced.

Supersedes, on those two questions only, the "Unknowns" section of
`docs/findings/2026-10-03-f37-d-adversarial-corpus-and-ordering-probes.md`,
which recorded that "terminal precedence for simultaneous success/failure is
still unmeasured" and that "original event ordering inside one tick is still
unmeasured". Both are measured now; everything else in that document stands.

## The short version

The two rules F37-A deferred and F37-D could not resolve are settled by
**static code analysis of the owner-supplied decrypted executable**, which the
owner accepted in place of an original-run capture (owner note on `#589`,
2026-10-05, plus its correction of the same date). The rules are then
**declared in code** with an explicit source label and, where the recreation
differs, a machine-readable limitation:

* `RuleSource` (`crates/cs_script/src/runtime.rs`) has exactly two values,
  `measured-from-original` and `designed-and-unmeasured`, and maps them
  structurally to `ClaimStatus::Inferred` and `ClaimStatus::Designed`. No arm
  can produce `verified_original`: the method is code-derived and non-runtime,
  and this stage self-awards no claim.
* `TERMINAL_PRECEDENCE_RULE` and `TICK_ORDERING_RULE` are labelled
  `measured-from-original` and carry the facts, with the addresses that settle
  each. `EVENT_OBSERVATION_ORDER_RULE` — the recreated `EventKey` observation
  order — is labelled `designed-and-unmeasured`, because the original emits no
  comparable event stream and the key comes from the contract.
* **One behaviour changed**: `MissionState::new` now selects
  `PrecedencePolicy::MeasuredOriginal` instead of the designed
  `SyntheticConservative`. The measured rule is *the recorded result is success
  if and only if the WON flag is set*, so a tick that requests both outcomes
  records the **success** where the designed policy recorded the failure.
  `SyntheticConservative` is kept and still selectable through the save record
  (the contract allows a designed policy "for synthetic tests only until
  verified" — it has been verified, so it is no longer the default).
* Five `f37.d.limit.*` entries record what this runtime does not follow or
  what the evidence cannot settle; each names the affected content and what
  resolves it. Three follow-up tasks were filed with this stage:
  `F37-D-FU3` (`#729`), `F37-D-FU4` (`#730`), `F37-D-FU5` (`#731`).

## Provenance

* Image: `$CS_GAME_DIR/crimson.decrypted.exe`, sha256
  `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` —
  the owner's decryption of `crimson.icd` (`0e3b4724…9833b`), analysed at the
  owner's request with the Kuna decompiler v1.692; the branch order was checked
  in the disassembly. The same image F16-F declared its clock policy from
  (`crates/cs_sim::time::ORIGINAL_IMAGE_SHA256`).
* Addresses below are **virtual addresses**; for `.text`, `.rdata` and `.data`
  below `0x643000`, file offset = VA − `0x400000`.
* Provenance only: addresses and behaviour are committed, **never** the image,
  a disassembly listing, decompiled code or any other executable byte. Nothing
  from the image is quoted here beyond virtual addresses and the sha256.
* This is static code evidence, not an original run and not
  `ObservationMethod::RuntimeObservation`. The claim is `inferred`, never
  `verified_original`; a `human_play` observation of a mission whose end
  conditions can conflict would still be needed to raise it.

## Key fact for this line of work: there is no mission bytecode

`objectives.zrd` is an ordinary reader (`.zrd`) tree; the native class
`CZMission` (assert path `D:\zipper\Crimson\mission.cpp`) interprets it by
keyword. Parser `0x466b70`, helpers `0x469070` (INACTIVE*),
`0x4693d0`/`0x469440` (COUNTER/TEST_*), per-frame interpreter
**`CZMission::Update` `0x46a490`**. Objective records are `0x5e4` bytes in an
array at `[mission+0xc4c]` with the count at `+0xc48`; the mission object is
at `0x71b480`. Measured field meanings: `[obj+0x554]` outcome kind (LOST=1,
WON=2, INSTANTWIN=3, INSTANTLOSS=4, none=0), `[obj+0]` identity (primary 1,
secondary 2, tertiary 3), `[obj+0xc]` awake, `[obj+0x14]` completed,
`[obj+0x5c8]` lifecycle state (0 dormant, 1 awake, 2 napping, 3 asleep/killed).
Mission flags: `[mission+0xc58]` WON (`0x463c10`/`0x463be0`), `[mission+0xc5c]`
LOST (`0x463c20`/`0x463bf0`), `[mission+0xc54]` ended (`0x463c00`).

The corrections on `#679` (2026-10-05) refine the effects list below: the
`WAKEUP_*`, `WAKE_ANIM` and `WAKEUP_SOUND_GROUP` keywords are **wake** effects
applied by the wake handler `0x469af0`, not completion effects; the completion
effects are read in `0x46a630`.

## Rule 1 — terminal precedence (`f37.rule.terminal_precedence`)

One tick = one rendered frame (see `#391`), and inside `CZMission::Update`:

| Fact | What the original does | Settled by |
| --- | --- | --- |
| `countdown_preempts` | The mission countdown (`0x71b468`) ticks by dt. If it expires (not with `NOLOSS`, not in network games) the mission ends **at once** with neither WON nor LOST, shows message `0x1772`, and does so **before** the objective passes — so a countdown expiry pre-empts any objective result of the same tick and, because the result is success iff WON, counts as a failure. | `0x46c640`, end call `0x463c30(1, 3.0)`, result `0x4194e0` |
| `one_completion_per_tick` | The completion scan starts at index 0 and the completed-this-tick flag is tested **after** the condition checks, so **at most one objective completes per tick**: the lowest-indexed one whose condition holds. Later satisfied objectives wait for later ticks, but their condition checks still run this tick. Through objectives, success and failure therefore cannot be requested on one tick. | `0x46a490`, cursor `0x71c128` (only normalised, never advanced), flag `0x46a94c` |
| `loss_branch_before_win` | The terminal check tests **LOST before WON** to choose the end delay (0.1 s when an INSTANTLOSS fired that tick, else 3.0 s) and which sound plays (`OBJECTIVES_LOST_SOUND +0xc74`, `OBJECTIVES_WON_SOUND +0xc70`). Which branch ran does **not** decide the recorded result. | `0x46af7a` then `0x46afad` |
| `result_iff_won` | The recorded mission result is **success iff the WON flag is set**; the LOST flag has no other reader. If both flags are ever set together (e.g. through debug commands `0x43d640`), the loss branch plays but success is what is recorded, what the end animation shows (`0x46ba10`: `WIN_ANIM +0x6e4` if WON, else `LOSS_ANIM +0x6e8`) and which sound the end call picks (`MISSION_WON_SOUND +0xc78` when WON, else `MISSION_LOST_SOUND +0xc7c`; when that sound exists the countdown to the end screen is 0). | `0x4194e0`, flags `+0xc58`/`+0xc5c`, `0x463c30`, `0x46ba10` |
| `no_aborted` | There is no Aborted outcome: an objective's outcome kind is LOST, WON, INSTANTWIN, INSTANTLOSS or none. | `[obj+0x554]` |

**Answer to the task's first question.** Success and failure cannot both be
recorded from objectives on one tick (only one completes). If both flags are
set, the code takes the LOST branch for delay and sound but **success wins the
result**. A countdown expiry beats any objective result of the same tick and
counts as a failure.

## Rule 2 — tick ordering (`f37.rule.tick_ordering`)

| Fact | What the original does | Settled by |
| --- | --- | --- |
| `mission_update_position` | The flight frame runs the world/node update first (planes, AI, weapons, damage, effects), HUD and rendering follow, and the mission update runs **near the end of the frame**. It is **skipped entirely** while the player-down flag is set. | frame `0x4a0220`, world update `0x4d0010`, mission call `0x4a09c3`, `[player+0x91d]` |
| `per_tick_order` | Inside the mission update: helper updates `0x46cdf0`/`0x46c870`; the countdown; mission time `[mission+0x6f0]` (returning immediately if the mission has already ended); the objective lifecycle timers in index order (dormant `[obj+0x5d0]`, awake `[obj+0x5d4]`, nap `[obj+0x5d8]`, end `[obj+0x5dc]`, gated by `TICK_DEPENDS_ON_OBJ [obj+0x10]`); the completion scan in index order with at most one completion and its effects in a fixed order (`0x46a630`); the WON/LOST set recount; then the terminal check with loss before win. | `0x46a490` and the addresses in the table above |

**Answer to the task's second question.** Event order inside a tick: world
simulation first, then mission — countdown, then lifecycle timers (index
order), then a single completion (lowest index), then its effects in the fixed
order, then the set recount, then the terminal check (loss before win).

The **observation** order of the recreated event stream
(`EventKey`: session, tick, source symbol, program sequence) is a separate,
`designed-and-unmeasured` rule (`f37.rule.event_observation_order`): the
contract fixes that key and the original emits no comparable stream, so nothing
about it is a claim about the original.

## What changed in the code

| File | Change |
| --- | --- |
| `crates/cs_script/src/runtime.rs` | `RuleSource`, `MeasuredFact`, `RuleLabel`, `RuleLimitation`, `ORIGINAL_IMAGE_SHA256`, `TERMINAL_RULE_FINDINGS`, the three rule constants and `RULE_LIMITATIONS`; `PrecedencePolicy::MeasuredOriginal` (labelled `measured-from-original`) with `source()` on both policies; `MissionState::new` selects it; module docs and the snapshot's `policy` field doc. |
| `crates/cs_script/src/ir.rs` | `MissionProgram`'s two orders now carry their source labels: execution order is declaration/index order (matching the measured scan) with the multi-completion divergence named, observation order is the contract's designed key. |
| `crates/cs_sim/src/mission.rs` | Module doc names the default policy's source; the conflicting-outcome session test now asserts the measured answer (below). |
| `crates/cs_script/tests/accept_f37_d_fu2.rs` | The new acceptance tests. |
| `crates/cs_script/tests/accept_f37_a.rs`, `accept_f37_d.rs`, `crates/cs_sim/src/mission.rs` | Existing expectations updated for the one behaviour that changed — see below. |

### The one behaviour change, and what it did to existing tests

`PrecedencePolicy::MeasuredOriginal::pick` records the success when both
outcomes are requested on one tick (`result_iff_won`), keeps the designed
ordering for `Outcome::Aborted` (`no_aborted` — the original has no such state,
so no measurement can order it), and otherwise records the single requested
outcome.

Three existing assertions expected the designed answer `Failed` for a tick that
requested both. They were **updated, not weakened**: each still asserts that
exactly one outcome is recorded, that it is latched and that later ticks cannot
flip it; only the value changed, to the measured one, and the new
`accept_f37_d_fu2_measured_precedence_records_success_iff_won` pins the value
from both directions (the measured default and the explicitly selected designed
policy):

* `accept_f37_a_simultaneous_success_and_failure_never_coexist` — terminal
  `Failed` → `Succeeded` (AC01's "success cannot coexist with failure" still
  holds: exactly one state is recorded and it never changes).
* `accept_f37_c_session_records_the_resolved_outcome_once_and_tears_down`
  (`crates/cs_sim/src/mission.rs`) — resolved and settled outcome
  `Failed` → `Succeeded`. Both `TerminalRequested` events still reach the host;
  the losing request stays a request.
* `accept_f37_d_emitted_order_is_the_reference_key_order_and_independent_of_declaration`
  — unchanged assertion (`Aborted` still wins that five-way conflict, because
  the abort ordering is the designed one), corrected comment.

No test was deleted, skipped or made more tolerant.

## What this runtime does not follow, and what stays open

Machine-readable, in `RULE_LIMITATIONS` (`crates/cs_script/src/runtime.rs`);
every entry names the affected content and what resolves it, and
`accept_f37_d_fu2_every_limitation_names_affected_content_and_a_resolving_task`
cross-references each entry against the facts that gate it:

| Limitation | What is open | Affected content | Resolving task |
| --- | --- | --- | --- |
| `f37.d.limit.one_completion_per_tick` | The original completes at most one objective per tick; `MissionState::step` completes every satisfied objective in declaration order on the same tick. | Every program run through `MissionState::step` with two or more objectives satisfied on one tick — today the F37-D corpus and the AC01 probe, from F38/F39 on every campaign mission lowered into this IR (starting with M01): per-tick completions, event sequence and same-tick write conflicts differ by one tick per extra completion. | `F37-D-FU3` (`#729`) |
| `f37.d.limit.mission_countdown_preemption` | The countdown expiry path does not exist here: `AdjustMissionTimer`/`EndMissionTimer`/`ResetMissionTimer` are host directives with no expiry consumer, a tick carries no timeout input, and the recreation's timer units are unmeasured. | Every mission that sets a mission countdown — the `TIMER_ADJUST`/`END_TIMER`/`RESET_TIMER` sites the source adapter lowers, i.e. every campaign mission with a time limit: its timeout neither fails the mission nor pre-empts an objective result. | `F37-D-FU4` (`#730`) |
| `f37.d.limit.terminal_branch_delay_and_sound` | The loss-before-win branch chooses the end delay and which sounds/animation play; this layer records the result only (the *result* half — success iff WON — is implemented and pinned). | Mission-end presentation for every campaign mission: end-screen delay, `OBJECTIVES_WON/LOST_SOUND`, `MISSION_WON/LOST_SOUND`, `WIN_ANIM`/`LOSS_ANIM`. | `F37-D-FU5` (`#731`) |
| `f37.d.limit.aborted_outcome` | The original has no Aborted outcome, so no observation of its precedence can exist; the measured policy keeps the designed ordering (abort above the measured results) so a torn-down mission is never recorded as a result a program asked for. | Any program or teardown requesting `Outcome::Aborted` together with WON/LOST on one tick (`Finish(Aborted)` sites and `MissionState::abort`): the recorded result of such a tick is designed, never measured. | none possible — owner decision only; the state does not exist in the original |
| `f37.d.limit.frame_phase_and_player_down` | The original's frame position of the mission update and its skip-while-player-down are caller behaviour; this runtime has no frame and no player-down input. | Every mission tick while the player is down and every mission's position in the frame, for the wired mission path that drives this session. | `VS-M01-RUNTIME` (`#359`), which wires the session into the application frame |

Other end paths the owner note records, kept here because they are measured but
belong to other stages: instant action ends the mission with WON and a 3.0 s
delay (`0x45b9d0`), and `0x480480`/`0x480570` **un-end** the mission
(`0x463c30(0, …)`) when a downed player is restored — the recreated
`MissionState` latches its terminal state and has no un-end path. On failure
the original increments a per-mission failure counter and, **every 4th
failure**, offers to skip the mission (string `0xbf`), where accepting sets WON
and records a success; that belongs to F43 (campaign progression), not to this
stage, and is recorded here so the rule is not lost.

## Acceptance

`accept_f37_d_fu2_*`, 5 tests in `crates/cs_script/tests/accept_f37_d_fu2.rs`,
all calling production code:

1. `..._measured_precedence_records_success_iff_won` — the default policy is
   `MeasuredOriginal`, both requests are reported, the success is recorded and
   latched; the explicitly selected designed policy still records the failure;
   an abort request still wins and its limitation exists.
2. `..._completion_scan_runs_in_declaration_index_order` — at the work-budget
   floor the objective declared first (symbol 9) is the one admitted first
   (symbol 3's own reward follows next tick), so the scan is index order and
   not symbol order.
3. `..._both_rules_carry_a_source_label_and_their_evidence` — both labels are
   `measured-from-original` / `inferred` / never `verified_original`, each
   cites the findings entry, the image sha256 and the addresses; the
   observation order is `designed-and-unmeasured` with no addresses and no
   facts.
4. `..._every_limitation_names_affected_content_and_a_resolving_task` — ids,
   uniqueness, affected content and resolving task on every entry; facts and
   limitations reference each other exactly; the four owner-note answers are
   recorded; the implemented fact carries no divergence; the findings entry
   exists in the tree and quotes the image sha256.
5. `..._recorded_divergences_match_what_the_runtime_does` — the runtime really
   completes two objectives on one tick and really has no timeout path, so the
   limitations that record those divergences cannot drift from the behaviour.

Mutation check for the behaviour change: restoring
`policy: PrecedencePolicy::SyntheticConservative` in `MissionState::new` fails
tests 1 and the two updated existing assertions; removing the label or
limitation tables fails tests 3 and 4.

## Evidence class

Static code evidence (`inferred`), no original run, no original data read at
run time. The F37 sheet's evidence clause — "synthetic fixtures alone cannot
certify original-data behavior" — is still not satisfied by this stage's tests
alone: what they certify is that the *code declares and applies* what the
owner's static analysis measured. F37 stays **checked**, never "recreated", and
nothing here self-awards `verified_original`.
