# F20-C: review corrections to the stale consumer paragraphs (seam attribution, #509, the mission host)

Date: 2026-10-14. Task: F20-C-stale-consumer-docs (#670), review pass.
Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
section `### F20-C` (documentation accuracy of the integration paragraphs).
Capabilities used: ordinary build/test only — no `CS_GAME_DIR` read, no
render, no audio, so no `private/evidence/` report is produced.

Task #670 asked for the two animation-module paragraphs that still called the
mission-marker consumer absent to describe what exists. The implementer's
draft (commit `9cebe580`) did that, but the review found both new paragraphs
asserting two things that are not true on today's `main`, so the review
corrected them before merge. This records the evidence.

## 1. The 2026-10-02 finding recorded `drain` as the seam, not `take_markers`

`docs/findings/2026-10-02-f20-c-wired-session-integration.md`, item 7: *"The
seam is `AnimationLog::drain`, and it is filed as #507."* The draft said the
finding recorded the seam as the log's `take_markers`. `take_markers` is what
task #507's consumer actually binds (`mission_markers.rs`, hand-off row: it
takes the marker records and leaves the other kinds for their consumers;
`drain` remains the total take for a host that is the only consumer). The
corrected paragraphs now attribute exactly that: `drain` is the recorded
seam, `take_markers` is the marker-specific take the consumer binds.

## 2. "#509: no production composition drives it" was stale on both counts

- **#509's real scope and status.** Rally #509 (`F20-C-app-composition`) is
  *"Register AnimationPlugin in the production session/app composition"* —
  the `PhysicsSessionBuilder::configure` call site the same finding filed as
  missing — and it is **done** (2026-10-05). It never was the mission-marker
  composition gap; the 2026-10-02 finding filed that consumer as #507 itself.
- **The production composition exists.** `crates/cs_app/src/mission_session/`
  (VS-M01-RT-MISSION-HOST, #1217, landed 2026-10-11/12) is a production
  mission host: `mission_session::compose::build_windowed` /
  `build_headless` stage the records, `install_mission_host` adds the
  `mission_host_tick` system to the fixed-tick schedule of a real `App`, and
  `MissionHost::step` calls
  `mission_animations::step_mission_animations` exactly once per committed
  tick — which drains the world's `AnimationLog` markers through
  `step_mission_with_markers` into `ObjectiveSession`. The claim "no
  production mission host drives that path" is therefore false.

What is honestly still missing on that path is the declared program, not the
plumbing: `ObjectiveSession` stays host-owned (never a resource), and for
every original mission the host launches it from the **empty**
`LoweredObjectives` (`mission_session::host::no_declared_objectives`) and
carries `MissionHostRefusal::ObjectiveDeclarations`, because the F39
objective declarations cannot be recovered for any original mission
(`ObjectiveRecovery::program()` refuses unconditionally; Rally #1219 owns
that measurement). The corrected paragraphs say exactly this.

## 3. The same stale claim lives in two files this task does not own

`crates/cs_app/src/lib.rs` (the F20-C paragraph, *"no production mission host
drives it yet (the animation composition is filed as #509)"*) and
`crates/cs_app/src/mission_markers.rs` (the *"No production mission host"*
bullet) still carry the #509 claim this review corrected in
`crates/cs_app/src/animation/`. They were written by #507's slice and sit
outside #670's owner paths, so this review filed them as a follow-up task
(#1405, `F20-C-stale-509-refs`) instead of editing them here (per the agent
contract: unrelated or out-of-scope problems are filed, not fixed in this
task).

## Files changed by the review

- `crates/cs_app/src/animation/binding.rs`, `crates/cs_app/src/animation/mod.rs`:
  the consumer paragraphs, corrected as above.
- `crates/cs_app/tests/accept_f20_c_stale_consumer_docs.rs`: the acceptance
  test now pins the recorded seam (`AnimationLog::drain`), the bound
  marker-specific take (`AnimationLog::take_markers`), the true remaining gap
  (`crate::mission_session` + Rally #1219), and the absence of the stale
  `#509` / "no production composition drives it" claims; the compile-level
  test also references the newly linked production items
  (`MissionHost`, `MissionHostRefusal`, `LoweredObjectives` via
  `no_declared_objectives`, `step_mission_animations`).
