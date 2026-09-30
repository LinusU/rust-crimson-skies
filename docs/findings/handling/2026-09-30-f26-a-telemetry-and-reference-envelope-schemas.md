# F26-A: handling telemetry and reference-envelope schemas

Date: 2026-09-30. Task: F26-A "Define telemetry and reference-envelope
schemas" (`specs/F26-handling-probes-and-original-behavior-calibration.md`,
section `### F26-A`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`,
sections "Inputs and outputs" and "Calibration acceptance". Capabilities used:
ordinary build/test only. No `CS_GAME_DIR` read, no `retail`/`gpu`/`audio`,
therefore **no evidence report** is required or produced: this stage ships
types, a synthetic fixture and a pure comparison, and awards at most
**checked** — never `verified_original`.

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/probes/mod.rs` (new, 149 lines): the module contract, the
  re-exports and the three core AC01 unit tests.
- `crates/cs_sim/src/probes/maneuver.rs` (new, 236 lines): the closed probe
  vocabulary — `ProbeKind` (the sheet's ten maneuvers), `ProbeQuantity` (the
  one quantity each maneuver's envelope primarily bounds) with labels and
  canonical units, and two unit tests.
- `crates/cs_sim/src/probes/envelope.rs` (new, 518 lines): the
  provenance-carrying record — `ReferenceEnvelope`, `EnvelopeEntry`,
  `Tolerance`, `TimingUncertainty`, `ProbeInitialState`, `ProbeInputStep`,
  `HandlingError`, and `ReferenceEnvelope::validate`.
- `crates/cs_sim/src/probes/comparison.rs` (new, 323 lines): the holdout rule —
  `ProbeMeasurement`, `ProbeTrace`, `ProbeVerdict`, `VerdictStatus`,
  `HandlingAssessment` and `compare`.
- `crates/cs_sim/src/probes/synthetic.rs` (new, 220 lines): the declared
  fixture (`synthetic_reference_envelope`) and the two candidates AC01 turns on
  (`synthetic_covering_trace`, `synthetic_acceleration_tuned_turn_outside`).
- `crates/cs_sim/tests/accept_f26_a_handling_envelopes.rs` (new, 297 lines):
  the six integration acceptance tests.
- `tools/cs_inspect/src/handling.rs` (new, 468 lines): the `handling` command
  that renders the production schema and both comparison outcomes as JSON, plus
  two unit tests.
- `crates/cs_sim/src/lib.rs`, `tools/cs_inspect/src/lib.rs`,
  `tools/cs_inspect/src/main.rs`, `tools/cs_inspect/Cargo.toml` and the root
  `Cargo.lock` (wiring only): `pub mod probes;`, `pub mod handling;`, the help
  and dispatch entries, and the new `cs_sim` dependency of `cs_inspect`. No
  logic was added there.
- This file.

**One observable failure:** with the holdout rule removed from `compare` — for
example by only comparing fitted entries (`filter(|entry| !entry.held_out)`) —
a candidate tuned to acceleration passes while its held-out turn radius is
outside the envelope. `compare` reports `passes() == true` and an empty
`failures()`, so the minimum scenario silently succeeds.
`accept_f26_a_tuned_acceleration_curve_cannot_pass_with_a_held_out_turn_outside_the_envelope`
fails on exactly that, in the module test and again in the integration test, and
the `handling` report test fails because the report then shows the out-of-
envelope candidate passing.

## What the slice does, per sheet rule

**Deliverable — the required fields.** The sheet names six things a
`ReferenceEnvelope` must record: input, initial state, difficulty, loadout,
timing uncertainty and units. `ReferenceEnvelope` carries identity
(`airframe_id`, `model_kind`), `difficulty`, `loadout`, the measured
`timing_uncertainty` and an `origin`; each `EnvelopeEntry` carries the recorded
`input` schedule (`ProbeInputStep { at_s, input }` over the production
`cs_sim::flight::FlightInput`), the `initial_state` (`ProbeInitialState`), the
reference value, and the `tolerance` selected before the fit. The declared
`unit` is checked against the quantity's canonical unit, so a record cannot
silently reinterpret metres as radians (`HandlingError::UnitMismatch`).

**Deliverable — justified tolerances, not blanket exact equality.**
`Tolerance::plus_minus` must be strictly positive
(`HandlingError::NonPositiveTolerance`) and its `rationale` must be non-blank
(`HandlingError::BlankToleranceRationale`), so the sheet's "justified
tolerances, not a blanket exact float equality" is a structural rule rather
than a comment. `EnvelopeEntry::accepted_window` is the inclusive accepted
window the comparison uses.

**Non-negotiable 1 — hold out probes to catch overfitting.** `validate`
refuses an envelope with no `held_out` entry (`HandlingError::NoHeldOutEntry`),
and `compare` walks **every** envelope entry — fitted and held out alike. A
candidate that matched only the data it was fitted to therefore fails AC01.
`HandlingAssessment::holdout_passes` reports the held-out subset separately,
and `failures()` names the specific entry, so a reader sees *which* maneuver
broke the fit.

**Non-negotiable 2 — record timing uncertainty, do not claim an exact tick
rate.** `TimingUncertainty { plus_minus_s, method }` requires a measured
half-width **and** a non-blank method (`HandlingError::BlankTimingMethod`); a
half-width with no stated method is refused rather than defaulted. Nothing in
the schema asserts an original tick rate.

**Non-negotiable 3 — numeric envelopes, not bitwise determinism.** This stage
defines the envelope and the tolerance, which is the representation the sheet
asks for across platforms; it makes no bitwise-determinism claim. Repeated
same-build probe hashing (AC02) is F26-B, and the comparison is a pure function
(asserted in the AC01 test, which compares the same pair twice and requires the
same assessment).

**Non-negotiable 4 — show original tuning versus calibrated deviations in an
inspector report.** `tools/cs_inspect handling` is a real consumer of the
production types, not a second implementation: it renders the probe
vocabulary, the declared envelope's recorded fields, and both comparison
outcomes with per-entry reference value, measured value and status
(`within_envelope`, `out_of_envelope { deviation }`, `no_measurement`). It
names its synthetic source (`synthetic-fixture`) and emits `retail:false`. The
"deliberate modern assist has an off switch and provenance" half is F24-C/F26-D
territory, not this stage.

**Non-negotiable 5 — every airframe needs a coverage row.**
`ReferenceEnvelope::missing_maneuvers`/`covers_every_maneuver` report a missing
maneuver instead of leaving it silently unmeasured; the `handling` report emits
the row. F26-C consumes it for the roster-wide audit.

**AC03's trace-side half.** A candidate that carries no measurement for an
entry is `VerdictStatus::NoMeasurement`, never a pass
(`HandlingAssessment::unavailable`), covered by
`accept_f26_a_a_missing_measurement_is_unavailable_not_a_pass`. The sheet's
F26-C minimum scenario ("a missing *reference* trace reports unavailable") and
AC04 (armour/mass extremes stay flyable) need the runtime and the roster; they
are out of this slice.

## What is deliberately *not* here

- **No integrator, no probe run, no runtime wiring.** This stage is types plus
  one pure comparison, as `### F26-A` requires. F26-B implements the headless
  maneuvers that produce a real candidate; F26-C wires the audit and deviation
  report; F26-D approves real envelopes.
- **No retail read.** The only declared envelope and traces are
  `Origin::SyntheticFixture`. `is_original_reference()` is true only for an
  `Origin::Installation` envelope, and
  `HandlingAssessment::supports_original_fidelity_claim()` additionally needs
  an original-observed candidate, so the fixture can never back an
  original-fidelity claim however complete it is.
- **No new asset format, no guessed layout.** The probe input is the existing
  production `FlightInput`; the vocabulary is authored design.

## Unknowns recorded (not guessed)

| Unknown | Affected content | How this stage represents it |
| --- | --- | --- |
| The original's exact force laws, units and tick rate | every real handling envelope | none is claimed; the fixture is `Origin::SyntheticFixture` and its timing method says "authored, not measured" |
| The tolerance each real maneuver needs | every real envelope | `Tolerance` is a required, justified field; the fixture's values are authored design, not selected from a fit |
| Which airframe/loadout/difficulty a real envelope belongs to | the roster-wide audit (F26-C) | `ReferenceEnvelope` carries `airframe_id`, `loadout` and `difficulty` as recorded free text; no original roster was read |
| The measured timing uncertainty method a real capture will use | every real envelope | `TimingUncertainty::method` is required and non-blank; the capture protocol is `cs_inspect::reference_capture` (#357), owner-gated |
| Whether the original can be captured at all (REF-OWNER-FIRST-CAPTURE, #358) | F26-D | not attempted; this stage reads no original data |

The F26 sheet's own "Research boundary" applies: no unrecorded compatibility
assumption is made, and no numeric field here is presented as an original
measurement.

## Three envelope-like records that must not be conflated

The tree now holds three distinct records:

1. `cs_sim::flight::autogyro::ReferenceManeuverEnvelope` (F25-A) says **which**
   maneuvers a calibration must contain.
2. `cs_inspect::reference_capture::ReferenceSet` (#357) is the operator's
   **capture worksheet and holdout reservation**.
3. `cs_sim::probes::ReferenceEnvelope` (this stage) is the **simulator-side
   schema** that carries what the original measured, in which unit, under which
   recorded conditions, with which tolerance — and `compare` reads it.

They are siblings, not replacements; the module docs say so. F26-B/C should not
reuse one for another's purpose.

## Test sensitivity (verified by perturbation, not asserted)

Each behavior was removed temporarily and the named tests re-run, then the file
was restored byte-identically from a backup (verified with `diff`):

| Behavior removed | Tests that failed |
| --- | --- |
| the holdout rule in `compare` (only fitted entries compared) | `accept_f26_a_tuned_acceleration_curve_cannot_pass_with_a_held_out_turn_outside_the_envelope` (unit **and** integration), `accept_f26_a_covering_candidate_passes_and_is_not_original_fidelity`, `accept_f26_a_a_missing_measurement_is_unavailable_not_a_pass`, and the `cs_inspect` `accept_f26_a_handling_report_names_synthetic_source_and_shows_both_outcomes` |
| the `NoHeldOutEntry` gate in `ReferenceEnvelope::validate` | `accept_f26_a_envelope_boundary_refuses_corrupt_records_by_name` |
| the `NoMeasurement` status in `compare_entry` (a missing row became `WithinEnvelope`) | `accept_f26_a_a_missing_measurement_is_unavailable_not_a_pass` |

## Limits of this pass

- Everything declared is synthetic. The fixture validates, covers every sheet
  maneuver and holds exactly the turn out, but none of its numbers is an
  original value and none may be read as one.
- `compare` is a pure function over supplied measurements; no integrator
  produced the candidate trace. AC02 (repeated same-build probe hashes) is not
  claimed here and belongs to F26-B.
- The probe vocabulary is the sheet's list, authored project design. Naming a
  maneuver implies nothing about how the original game flew it.
- `handling` accepts only `--out`; every other flag (including `--cs-path`) is
  refused with exit 2 because retail reading is not available in this stage.
- Coverage is per envelope, not per roster: "every supported airframe needs a
  coverage row" is reported by the schema and rendered for one fixture, but the
  roster-wide audit is F26-C.
- `supports_original_fidelity_claim` is a structural gate, not a measurement:
  it requires an installation envelope and an original-observed candidate and a
  pass, but this stage cannot produce either, so it is always false here and
  proves only that the fixture cannot leak into a fidelity claim.

## Implementer identity

Implementer: `deepseek-1/deepseek-1` on branch
`rally/101-define-telemetry-and-reference-envelope`, rebased on `origin/main`.
This stage ships a typed boundary, a synthetic fixture and one pure comparison;
it reads no original data and is **checked**, never `verified_original` or
`release_approved`.
