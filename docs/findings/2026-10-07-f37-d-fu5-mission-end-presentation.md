# F37-D-FU5: the measured mission-terminal branch order applied to mission-end presentation

Date: 2026-10-07. Task: F37-D-FU5 "Apply the measured mission-terminal branch
order (loss before win) to mission-end presentation" (`#731`), the follow-up
F37-D (`#140`) recorded when F37-D-FU2 (`#589`) closed. Spec stage:
`specs/F37-mission-ir-and-deterministic-runtime-core.md` (`### F37-D`).
Shared contract: `docs/contracts/SCRIPT-MISSION.md`. Required capability:
ordinary build/test — no `retail`, `gpu` or `audio` input was used, this stage
reads no original data at run time, renders nothing and plays nothing, so no
`private/evidence/` report is produced.

This entry closes `f37.d.limit.terminal_branch_delay_and_sound`, recorded by
`docs/findings/2026-10-07-f37-d-fu2-mission-terminal-precedence-and-tick-ordering.md`.
The rule itself, its addresses and its provenance are unchanged there and are
not re-measured here.

## The short version

The loss-before-win branch of `CZMission::Update` chooses the presentation and
**nothing else**: the recorded result stays *success iff the WON flag is set*
(`0x4194e0`). That split is now in the runtime as
`MissionEndPresentation` (`crates/cs_script/src/runtime.rs`), produced on the
tick that ends the session and read by the session as
`MissionTick::presentation`, with **three independent reads** so the branches
cannot be collapsed into one:

| Input | What it decides | Address |
| --- | --- | --- |
| LOST flag, tested **before** WON | `branch` (`Loss` even when WON is set, else `Win`, else none) and with it the `OBJECTIVES_LOST_SOUND` (`+0xc74`) / `OBJECTIVES_WON_SOUND` (`+0xc70`) slot | `0x46af7a` then `0x46afad` |
| WON flag | `MISSION_WON_SOUND` (`+0xc78`) / `MISSION_LOST_SOUND` (`+0xc7c`) and `WIN_ANIM` (`+0x6e4`) / `LOSS_ANIM` (`+0x6e8`) — the flag, never the branch and never the recorded result | `0x463c30`, `0x46ba10` |
| instant outcome fired that tick | end delay **0.1 s**, else **3.0 s** | instant byte beside `0x463c10`/`0x463c20`; terminal check `0x46af7a`/`0x46afad`; end call `0x463c30` |
| the precedence policy | `result`: success iff WON | `0x4194e0` |

So a tick that requests both outcomes runs the **loss** branch —
`OBJECTIVES_LOST_SOUND`, and the end delay the instant marker selects — while
recording the **success**, showing `WIN_ANIM` and reaching for
`MISSION_WON_SOUND`. That is exactly the case the limitation said this layer
could not express, and `accept_f37_d_fu5_loss_branch_runs_while_the_recorded_result_is_success`
pins it on the production path.

A *selection* is what this is. Which of those slots a mission fills with a
handle is mission content (a null handle plays nothing — M01 spells none of the
seven `*_SOUND` keys), and playing, showing or timing anything on screen needs
the audio/presentation stages and their own capabilities. Nothing here claims
an audible or visual result, and nothing self-awards `verified_original`.

## Provenance

Same evidence as the rule it implements, not a new measurement:

* Image: `$CS_GAME_DIR/crimson.decrypted.exe`, sha256
  `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` —
  the owner's decryption of `crimson.icd`, analysed at the owner's request
  (owner note on Rally #589, 2026-10-05, plus its correction of the same date).
* Addresses are **virtual addresses**; for `.text`, `.rdata` and `.data` below
  `0x643000`, file offset = VA − `0x400000`.
* Provenance only: addresses and behaviour are committed, **never** the image,
  a disassembly listing, decompiled code or any other executable byte.
* Static code evidence, `inferred`, never an original run and never
  `verified_original`. A `human_play` observation would still be needed to
  raise it.

## What changed in the code

| File | Change |
| --- | --- |
| `crates/cs_script/src/runtime.rs` | `INSTANT_END_DELAY` (0.1 s) / `STANDARD_END_DELAY` (3.0 s); `TerminalBranch`, `ObjectivesCue`, `MissionCue`, `EndAnimation`, `EndDelay`, `MissionEndPresentation` with `new(lost, won, instant, result)` written loss-first; `MissionState` stores the selection, exposes `MissionState::terminal_presentation()`, sets it where `step` resolves a terminal request and where `abort` ends a session; the record carries it (`MissionStateSnapshot::presentation`), `SNAPSHOT_VERSION` 1 → 2, and `RestoreDefect::PresentationMismatch` refuses a record whose terminal state and presentation disagree; `f37.d.limit.terminal_branch_delay_and_sound` removed and the `loss_branch_before_win` fact now carries no limitation. |
| `crates/cs_script/src/ir.rs` | `Action::Finish`'s documentation records the mapping below and names the gap it leaves (a non-instant ending cannot be expressed). |
| `crates/cs_sim/src/mission.rs` | `MissionTick::presentation`, filled by `MissionSession::advance` from the state, so the host reads the selection with the tick that ended the mission. |
| `crates/cs_script/tests/accept_f37_d_fu5.rs` | The new acceptance tests. |
| `crates/cs_script/tests/accept_m01_lc_lowering_signatures.rs` | The save-record fixture gains the new field. |
| `docs/findings/2026-10-07-f37-d-fu2-…md` | The limitation table row records the closure and points here. |

## The two mapping decisions, and what stays open

**1. A program's terminal request is the instant-outcome marker.**
The original distinguishes an `INSTANTWIN`/`INSTANTLOSS` firing *this tick*
(0.1 s) from a mission whose flags were already set (3.0 s). The measured
lowering vocabulary reaches the mission IR's one terminal action
(`Action::Finish`) through exactly those two keys — `cs_content::mission_control::terminal_outcome_of`,
"the mission IR has an action for it", with `WON`/`LOST` spelled as
`DirectiveOperation::OutcomeClass` instead — so a request that resolves the
mission on the tick it arrives *is* that marker, and a transition with no such
request takes the standard 3.0 s delay. Both halves are reachable on the
production path and both are pinned.

**2. The no-flag shape comes from the countdown path, not from a guess.**
When neither flag is set the terminal check's objectives-sound branch does not
run at all, and the countdown-expiry path calls `0x463c30(1, 3.0)` directly:
no `OBJECTIVES_*_SOUND`, the standard delay, and the loss side of the mission
sound and animation because WON is clear. That is the shape a host teardown
(`MissionState::abort`) and an `Action::Finish(Outcome::Aborted)` request get
here. The **result** of those paths stays designed — the original has no
Aborted state — which `f37.d.limit.aborted_outcome` still records and
`accept_f37_d_fu2_measured_precedence_records_success_iff_won` still pins;
this stage adds no claim about them beyond the structure above.

What is **not** closed by this entry, recorded so it cannot be lost:

* **Playback.** Filling the sound slots with handles, playing them, showing the
  end screen and running `WIN_ANIM`/`LOSS_ANIM` are the audio/presentation
  stages' own work and their own evidence (F41, F40/F45, `audio`/`gpu`/`human_*`
  capabilities). The F37 layer never claimed them; it claims the selection.
* **Handle presence.** Whether a mission spells `MISSION_WON_SOUND` and its
  siblings is mission content read from the control record (M01 spells none of
  the seven), not a runtime answer. The measured "when that sound exists the
  countdown to the end screen is 0" therefore stays with the consumer.
* **A non-instant win/loss ending.** The IR has one terminal action, mapped to
  the instant spelling above, so a program cannot yet end a mission *without*
  the instant marker: the outcome-class aggregation (`WON`/`LOST`, the 3.0 s
  ending) has no terminal producer in this layer, and the countdown-expiry
  ending is `F37-D-FU4` (`#730`)'s. No content in this tree reaches it today —
  M01 ends through `INSTANTWIN`/`INSTANTLOSS` — so no `f37.d.limit.*` entry
  gates it; the gap is filed as its own task and named here
  (`F37-D-FU6`, see the Rally queue). Until it exists, a lowering that ends a
  mission by class aggregation would select the instant delay, and that
  statement must be revisited with it.

## Acceptance

`accept_f37_d_fu5_*`, 6 tests in `crates/cs_script/tests/accept_f37_d_fu5.rs`,
all calling production code (`MissionState::new`/`step`/`abort`/`snapshot`/
`restore`), plus
`accept_f37_d_fu5_session_tick_carries_the_mission_end_presentation` in
`crates/cs_sim/src/mission.rs`, which drives `MissionSession::advance`:

1. `..._loss_branch_runs_while_the_recorded_result_is_success` — both requests
   on one tick: recorded result `Succeeded`, branch `Loss`,
   `ObjectivesLostSound`, `MissionWonSound`, `WinAnim`, instant delay; no
   presentation while the mission runs.
2. `..._cues_follow_the_won_flag_not_the_recorded_result` — the designed
   policy records the failure while the mission sound and animation stay the
   win side: the cues read the flag, not the result.
3. `..._win_and_loss_branches_select_their_own_cues` — the two single-request
   cases, each consistent within itself.
4. `..._end_delay_follows_the_measured_instant_rule` — the 0.1 s / 3.0 s pair,
   the instant half from a win/loss request, the standard half from an abort
   request and from a host teardown, and the no-flag shape of both.
5. `..._presentation_latches_and_survives_save_and_restore` — latched across
   later ticks, exact save/restore round trip, and two forged records (a
   terminal record without its presentation, a running record with one) refused
   with `RestoreDefect::PresentationMismatch`.
6. `..._terminal_branch_limitation_is_closed_with_the_finding` — the
   limitation is gone from `RULE_LIMITATIONS`, the fact it gated carries none,
   and this entry exists and quotes the addresses, the image sha256 and the
   limitation id.

Mutation check, run against this commit: swapping the branch test in
`MissionEndPresentation::new` to WON-first fails tests 1 and 3 (the loss branch
would not run when both flags are set); deleting the `MissionState::presentation`
assignment in `step` fails 1–5; computing `mission_cue`/`animation` from
`result` instead of the WON flag fails test 2; dropping the restore consistency
check fails 5; removing the limitation only (without this entry) fails 6. Each
mutation was reverted and the tree re-verified clean and green.

`f37.d.limit.terminal_branch_delay_and_sound` is closed by exactly those tests
and this finding: the branch, the delay, the sounds and the animation selection
all exist here now, and the four AGENTS.md checks pass before push.

## Evidence class

Static code evidence (`inferred`), no original run, no original data read at
run time. What the tests certify is that the code applies the branch order the
owner's static analysis measured and keeps it separate from the recorded
result. F37 stays **checked**, never "recreated"; audible, visual and
ordinary-play claims stay gated on the capabilities and stages named above.
