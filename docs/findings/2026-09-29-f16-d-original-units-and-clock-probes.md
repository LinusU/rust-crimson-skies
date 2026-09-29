# F16-D: original units still unmeasured, and fixed-tick behavioral probes

Date: 2026-09-29. Task: F16-D "Calibrate original units and compare fixed-tick
behavioral probes"
(`specs/F16-coordinates-units-origin-management-and-clocks.md`, section
`### F16-D`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. Required
capability: ordinary build/test. The machine also reports `retail`, `gpu` and
`audio`; **none was used** — this stage reads no original data, renders nothing
and plays nothing, so no `private/evidence/` report is produced.

## The short version

This stage built the two things the title asks for and **measured neither of
the original's answers**:

* A behavioral probe (`cs_sim::time`) that runs a scripted session, records a
  labelled trace of a weapon cooldown and an objective timer, and compares a
  measurement against an attributed reference with a tolerance selected before
  the comparison. The AC04 minimum scenario passes at 7, 30, 60 and 144 render
  FPS: 30 s of paused wall time advances both quantities by exactly zero ticks.
* A calibration record (`cs_content::coordinates`) that makes F16
  non-negotiable behavior 1 — three *independent* landmarks per quantity, at
  least one of them an observed behavior — checkable instead of remembered, and
  keeps "complete" separate from "original".

**The original's scale, handedness, axis order and angle units remain
unmeasured**, and the probe machinery cannot change that: a claim is decided by
an `EvidenceRecord`, and no evidence record in this tree was produced by an
original run. Every comparison this stage can make claims `observed_tool` at
best. Reaching `verified_original` needs an owner-supplied original run (see
*Recorded unknowns* and the follow-up task filed with `create_tasks`).

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/time.rs` (extended): F16-A's `SimClock`/`ClockPolicy` and
  F16-C's integration names stay; F16-D adds
  - `TimeError::ZeroTimerPeriod` — the one new clock-domain refusal;
  - `TimerKind` (`WeaponCooldown`, `ObjectiveTimer`, `label`);
  - `TickTimer` (`new`, `kind`, `period_ticks`, `remaining_ticks`,
    `elapsed_ticks`, `is_expired`, `expirations`, `period_seconds`, `restart`,
    and the **private** `commit`) — a countdown in whole ticks with no
    wall-time entry point;
  - `GameplayTimeline` (`new`, `clock`, `cooldown`, `objective_timer`, `tick`,
    `is_paused`, `set_paused`, `advance_frame`, `advance_fixed_ticks`, `fire`)
    — the production consumer of AC04;
  - `ProbeStep` (`Advance`, `OneFrame`, `Pause`, `Fire`, `Observe`),
    `ProbeSample`, `ProbeTrace` (`summary`, `sample`, …), `BehavioralProbe`
    (`new`, `name`, `render_fps`, `steps`, `run`), `MAX_FRAMES_PER_STEP`,
    `ProbeError` + `Display`/`Error`/`From<TimeError>`, `ProbeReference`
    (`new`, `trace`, `evidence`, `tolerance_ticks`), `ProbeDivergence` +
    `Display`, `ProbeComparison` (`new`, `agrees`, `divergences`, `claim`,
    `verified_original`, `summary`);
  - six `accept_f16_d_*` unit tests.
- `crates/cs_content/src/coordinates.rs` (extended): F16-A's adapters stay;
  F16-D adds `CalibratedQuantity`, `LandmarkKind`, `CalibrationError` +
  `Display`/`Error`, `Landmark`, `CalibrationGap` + `Display`,
  `UnitCalibration` (`new`, `source`, `landmarks`, `record`, `landmark_count`,
  `behavior_landmark_count`, `gaps`, `is_complete`, `claim_status`,
  `describe`) and `CoordinateSource::calibration`.
- `crates/cs_sim/tests/accept_f16_d_pause_and_probe_evidence.rs` (new): the
  AC04 minimum scenario, frame-rate agreement, the attributed-reference
  comparison and the named probe failure cases.
- `crates/cs_content/tests/accept_f16_d_unit_calibration.rs` (new): the
  three-landmark rule, its independence requirement and its claim discipline.
- Wiring only (no logic): none. `cs_sim` already depended on `cs_types`, which
  owns `evidence`, so no `Cargo.toml` or `Cargo.lock` edge changed.

**One observable failure:** if `GameplayTimeline::advance_frame` fed the timers
the frame's wall delta instead of the ticks the clock committed, a single
30 s frame would empty the one-second cooldown and most of the 9.375 s
objective deadline *while paused*, and both the unit and the integration
acceptance tests fail (probe 1: 5 of the 15 tests). The control run in the
integration test is the other half: the same script with the pause removed
*does* advance both quantities over the same 30 s, so the paused equality means
something.

## Design decisions

- **A gameplay quantity has no wall-time entry point.** `TickTimer::commit` is
  private to the module, so the only way a cooldown or an objective deadline
  moves is whole ticks some clock already committed. AC04 is then a property
  of the type: a paused frame commits zero ticks, so it cannot shorten a
  cooldown. There is no `TickTimer::advance(Duration)` to get wrong, and no
  "ticks down faster while paused" policy to remember.
- **A countdown saturates and counts one expiry.** A 10 s frame does not
  produce six expirations of a one-second cooldown: `commit` stops at zero and
  increments `expirations` once, at the moment it reaches it.
- **The render frame rate belongs to the probe, and it really drives
  delivery.** The first version took `render_fps` as a `run` argument and used
  it only for the trace's label, which made the 30/60/144 comparison vacuous —
  a reviewer would have been right to call that a decorative parameter. The
  probe now carries the rate, `frame_spans` derives the frame length from it
  (`10⁹ / fps` ns, integer) and emits whole frames plus a remainder frame, and
  `render_fps` is refused if a frame would be shorter than one nanosecond. 7 fps
  is in the fixture precisely because it does not divide a second evenly.
- **The frame split is bounded.** `ProbeStep::Advance { nanos }` is unbounded
  wall time: `u64::MAX` nanoseconds at 144 fps is about 2.6·10¹² frames.
  `MAX_FRAMES_PER_STEP = 1_000_000` refuses it with both numbers in the error,
  so a typo cannot turn into a hang or an out-of-memory.
- **A reference carries its evidence and its tolerance, and the comparison
  reports the claim the evidence supports.** The tolerance is a constructor
  argument selected *before* the comparison (`FLIGHT-PHYSICS`, "Calibration
  acceptance"), and the test pins both sides of it: a one-tick difference is
  inside a one-tick tolerance and outside a zero-tick one.
- **The claim is never awarded by the comparison.** `ProbeComparison::claim`
  is `Contradicted` when the traces diverge, `VerifiedOriginal` only when they
  agree *and* the reference's own `EvidenceRecord::verifies_original()`, and
  `ObservedTool` otherwise. A shorter trace also diverges (a sample present in
  the reference and missing from the measurement is a named divergence), so a
  probe cannot "pass" by observing less.
- **The AC04 scenario ships with an unpaused control.** Without it, the paused
  equality could be satisfied by an inert fixture. With it, the same 30 s is
  shown to empty the cooldown (0/64) and to move the objective (568 → 0) when
  the pause is not there.
- **"Complete" and "original" are different questions, and the calibration
  keeps them apart.** `UnitCalibration::is_complete` asks a *shape* question
  (three independent landmarks and one behavior per quantity);
  `claim_status` asks a *strength* question and only the `EvidenceRecord`s
  answer it. A complete calibration over synthetic fixtures is
  `ClaimStatus::Unknown`; a document-only one is `Documented`; a tool run over
  produced artifacts is `ObservedTool`; a set containing any synthetic landmark
  is `Unknown` even if the rest came from tool runs.
- **"A Blender transform is insufficient proof" is a code rule.**
  `LandmarkKind::{Artifact, Behavior}` and `UnitCalibration::gaps` require at
  least one `Behavior` per quantity, so three stored transforms never calibrate
  anything. `record` additionally refuses a repeated description and a reused
  `EvidenceRecord` for the same quantity, so one inspection cannot be pasted
  three times — while the same description for a *different* quantity is
  allowed, because one observation may legitimately speak to two properties.
- **The probe names its own refusals.** `ProbeError` covers an unusable render
  rate, an unbounded frame count, a repeated observation label, a script that
  observes nothing, and a timeline that refuses to be built; the clock's own
  errors are wrapped rather than flattened, so `ProbeError::source` still
  reports the `TimeError`.

## Test inventory (`accept_f16_d_`)

15 tests, all selecting production code: 6 unit tests inside `time.rs`, 4 in
`cs_sim/tests/accept_f16_d_pause_and_probe_evidence.rs` and 5 in
`cs_content/tests/accept_f16_d_unit_calibration.rs`. All are ordinary tests
(none `#[ignore]`d) and none needs `CS_GAME_DIR`.

| Test | Covers |
| --- | --- |
| `cs_sim::time::tests::accept_f16_d_pause_advances_no_cooldown_and_no_objective_timer` | **minimum mechanism** at unit level: 30 s of paused wall time as one frame, as 300 frames and as one frame again commits zero ticks and leaves tick, remaining and elapsed of both quantities untouched; resume banks no paused time |
| `…accept_f16_d_tick_timers_saturate_and_refuse_a_zero_period` | a zero-tick period is refused (`TimeError::ZeroTimerPeriod`); a 10 s frame over a 10-tick timer gives **one** expiration, not ten; `fire` re-arms the cooldown to its full period and leaves the objective timer alone; `period_seconds` reports the authored ticks at the session rate |
| `…accept_f16_d_frame_split_is_exact_at_every_delivery_rate` | one second at 64 Hz is 64 ticks at 1, 7, 30, 60, 144 and 1000 fps, tick and cooldown alike |
| `…accept_f16_d_probe_comparison_claims_only_what_its_evidence_supports` | agreement against fixture evidence is `observed_tool` and never `verified_original`; a divergence is `Contradicted` and names the field; a shorter trace diverges on `sample-present-in-measurement`; a tolerance selected before the comparison admits one tick and rejects it at zero |
| `…accept_f16_d_probe_propagates_a_refused_timeline_instead_of_tracing_it` | a timeline that cannot be built produces no trace |
| `…accept_f16_d_probe_refuses_unusable_rates_labels_and_frame_counts` | zero and sub-nanosecond render rates, a script with no observation, a repeated label, and an unbounded frame count are each refused by name |
| `cs_sim/tests/…::accept_f16_d_pause_produces_zero_weapon_cooldown_and_objective_advancement` | **AC04 minimum scenario**: the full script at 7/30/60/144 fps commits 48 ticks (32 + 16, never the paused 30 s), the two paused samples equal the pre-pause sample on tick, cooldown remaining and expiration, firing re-arms the cooldown without moving the objective, and the **unpaused control** shows the same 30 s emptying the cooldown (1 expiration) and the objective, with 3888 ticks committed |
| `…accept_f16_d_probe_traces_agree_across_render_frame_rates` | the four delivery rates commit the same ticks and produce byte-identical samples |
| `…accept_f16_d_measured_trace_matches_an_attributed_reference` | the comparison half: a reference over authored fixture evidence agrees and claims `observed_tool`; the unpaused trace used as a reference *contradicts* the measurement and names the label, field and both values; the fixture evidence is asserted not to verify the original |
| `…accept_f16_d_probe_failure_cases_are_named_and_propagate` | the probe's four own refusals plus the refused timeline, all by name |
| `cs_content/tests/…::accept_f16_d_a_quantity_needs_three_independent_landmarks` | two landmarks are not three, for each of the four quantities; filling all four makes the record complete and the gap list empty |
| `…accept_f16_d_artifacts_alone_never_calibrate_a_quantity` | **"a Blender transform is insufficient proof"**: three artifact landmarks per quantity are *not* complete, the gap names the missing behavior (`0/1 behaviors`), and one behavior closes that quantity and only that quantity |
| `…accept_f16_d_landmarks_must_be_independent` | an empty source and an empty description are refused; a repeated description is `RepeatedDescription`; the same evidence behind a new description is `RepeatedObservation`; a refused landmark is not recorded; the same description for a *different* quantity is allowed and counted once per quantity |
| `…accept_f16_d_a_complete_calibration_claims_only_what_its_evidence_supports` | the claim discipline: fixtures → `Unknown` (and `describe()` says so), documents → `Documented`, tool runs → `ObservedTool` with each record asserted not to verify the original, a mixed set containing one synthetic landmark → `Unknown`, and a set missing one landmark of one quantity → `Unknown` with the gap naming `angle-unit` 2/3 |
| `…accept_f16_d_no_declared_source_claims_a_measured_original_convention` | every declared source has an empty calibration, claims `Unknown`, does not claim installation data, and reports all four quantities as gaps |

The fixture: 64 Hz, a 64-tick (1 s) weapon cooldown and a 600-tick (9.375 s)
objective deadline, and a script that plays 500 ms, pauses for 30 s (once as a
single frame, once as many frames), resumes, fires and plays 250 ms. Every one
of those numbers is a newly authored development value.

## Mutation probes (implementation neutered → tests fail; all reverted and byte-compared)

Each probe was applied to a production file by a rerunnable driver, the full
`accept_f16_d_` selection was run with `--no-fail-fast` and
`--include-ignored`, the file was restored and its sha256 re-checked. No
`MUTATION PROBE` marker is left in the tree and `git status` is clean.

| # | Edit | Result of the 15 tests |
| --- | --- | --- |
| 1 | `GameplayTimeline::advance_frame` feeds the timers the frame's wall delta instead of the committed ticks | 5 fail, including `accept_f16_d_pause_produces_zero_weapon_cooldown_and_objective_advancement` |
| 2 | `SimClock::advance` ignores the pause policy | 3 fail, including the AC04 scenario |
| 3 | paused wall time is banked into `carry` instead of dropped | 3 fail, including the AC04 scenario |
| 4 | `ProbeComparison` always claims `verified_original` | 2 fail, in both `cs_sim` crates |
| 5 | `ProbeComparison` stops looking for samples missing from the measurement | 1 fail (the shorter-trace case) |
| 6 | the frame split drops the remainder frame | 4 fail, including the frame-rate agreement test |
| 7 | `TickTimer::commit` banks expiry counts per frame instead of once | 2 fail |
| 8 | `UnitCalibration::gaps` ignores the behavior requirement | 1 fail |
| 9 | `UnitCalibration::record` accepts a reused observation | 1 fail |
| 10 | `UnitCalibration::claim_status` derives the claim from completeness | 2 fail |
| 11 | `CoordinateSource::calibration` pre-fills a complete calibration | 1 fail |

`sha256` before and after every probe:
`crates/cs_sim/src/time.rs` `ae4689ed7b976827c1f6aa300bc188d794f4e27e881c8e67269ebb8b707697af`,
`crates/cs_content/src/coordinates.rs` `cad415cd88b8fd96d0b6396f93d575c9f65d7f06b6f77a6e9589f213242a6951`.

**One methodological note for the reviewer:** the first probe run used a plain
`cargo test` and reported that probe 1 failed only two unit tests. It had not
reported a hole — `cargo test` stops after the first failing test target, so the
integration binary never ran. With `--no-fail-fast` the AC04 scenario fails as
well. The driver above always uses `--no-fail-fast`; a probe table produced
without it understates coverage.

## Recorded unknowns (recorded, not guessed)

- **Original scale, handedness, axis order and angle units are still
  unmeasured.** This is inherited from F16-A and F16-C and this stage does not
  change it: `CoordinateSource::calibration()` hands every declared source an
  empty record claiming `Unknown`, and the only way to raise that is
  `EvidenceRecord`s that themselves verify the original. Rule 1's three
  landmarks have to come from an original run or from original bytes, and an
  agent has neither the run nor the right to claim one. The machinery that will
  consume those landmarks is in place and tested; the landmarks are not, and
  are filed as a follow-up task.
- **The pause/cooldown/objective pairing is a designed default.** Which
  subsystems freeze, whether a weapon cooldown keeps running while a mission
  timer does not, whether the original even has an objective *countdown*, and
  what the periods are — none of that is measured. The fixture's 1 s cooldown
  and 9.375 s deadline exist to make the zero-advancement property testable,
  not to resemble the original game.
- **The 64 Hz rate, the seven-second delivery rate, the rebase threshold and
  the 1 000 000-frame bound are development values** (the rebase threshold and
  the spatial side are F16-C's; the rest is this stage's).
- **`GameplayTimeline` is a time-domain consumer, not the combat system.** It
  holds one cooldown and one objective timer because AC04 names those two. Real
  weapon fire rates, per-weapon cooldowns, reloading, and objective variety are
  F13+/F23 gameplay work that will consume `TickTimer` rather than reimplement
  it; no combat balance is encoded here.
- **The tick counter's overflow path is unreachable through a probe.** A
  `GameplayTimeline` always starts at tick 0, and `u64` nanoseconds at 64 Hz is
  about 1.2·10¹² ticks, so no single step wraps a `u64` counter. The probe
  therefore tests error propagation (a refused timeline) rather than an
  overflow, and `SimClock`'s own overflow refusal stays where F16-A tested it.
  Reaching a wrapping counter through a probe would need a start-tick argument
  that no current caller wants.
- **No Bevy schedule binds any of this.** `GameplayTimeline` is plain Rust
  over `cs_sim`/`cs_types`; binding it to a `FixedUpdate` schedule and real
  components is the later fixed-step work. No claim is made that an original
  executable behaves this way.

None of the first three is a new problem, so the follow-up task below names the
resolving work and the affected content rather than silently closing it.

## Follow-up filed

- **Original unit calibration from owner-supplied evidence** (created with
  `create_tasks`): record at least three independent landmarks per
  `CalibratedQuantity` from original bytes or an original run, with the
  `EvidenceRecord`s that verify them, so `UnitCalibration::claim_status` can
  move off `Unknown`. It needs `retail` plus an owner-supplied capture, and
  `human_play`/`human_review` for the behavioral landmarks — none of which an
  agent has. It gates any claim that the project's units are the original's.
- **Original clock and pause behavior probe** (created with `create_tasks`):
  run the same `BehavioralProbe` script against the original game and feed the
  resulting trace in as a `ProbeReference` with the capture's evidence. Also
  needs the owner.

## Commands run

All commands from the repository root on branch
`rally/68-calibrate-original-units-and-compare-fix`, Rust 1.98.1, based on
`origin/main` (`6192a8b`).

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 (107 `test result: ok` lines) |
| `cargo test --workspace --locked -- accept_f16_d_ --include-ignored` | 0 (**15 tests**, all passing) |
| mutation probes 1–11 (above), each with `--no-fail-fast` | 0 driver; every probe's selection exited 101 and every file was restored byte-for-byte |

No command needed `CS_GAME_DIR`, and no `accept_f16_d_` test is `#[ignore]`d;
`CS_CAPABILITIES` (`retail,gpu,audio`) was not exercised by this stage.

## Wiring edits (outside owner paths, logic-free)

None. `cs_sim` already depended on `cs_types`, which owns `evidence`, so the
probe's evidence records needed no new crate edge, no `Cargo.toml` change and no
`Cargo.lock` change.

No protected path, original datum or binary file is involved.
