# Baseline flight probe worksheet (original observation)

**Task:** #357 `REF-CAPTURE-PROTOCOL`. **Acquisition task:** #358 `REF-OWNER-FIRST-CAPTURE`.
**Consuming feature:** F26 (handling probes and original-behavior calibration), shared contract
`docs/contracts/FLIGHT-PHYSICS.md`.
**Companion:** `2026-09-29-ref-capture-record.md` (record fields and admission rules).

This worksheet is the `baseline_flight` variant of a capture record: one row per probe maneuver for
one airframe/loadout. It records what was observed while flying the original, plus everything a
later fit needs to be falsifiable. **No top speed, turn rate, frame rate, stall speed or timing
appears here** — those are measured per row or written `Unknown` with a reason.

## Probe rows

One `CaptureRecord` (`worksheet = baseline_flight`, `role = calibration` unless reserved) per probe.
The maneuver vocabulary follows the F26 probe list; picking a name implies nothing about the
original's values.

| Probe (`maneuver`) | What the row must pin down |
| --- | --- |
| `acceleration` | initial state, full-throttle input sequence, observed speed/time samples |
| `coast-down` | throttle-cut point, observed deceleration samples |
| `climb`, `dive` | entry state and attitude, observed rate samples |
| `turn` | bank/attitude held, observed rate and radius samples |
| `roll`, `yaw` | control input sequence, observed rate samples |
| `stall-recovery` | entry condition as observed, recovery sequence and outcome |
| `damage` | damage state as observed, control response samples |
| `boost` | boost state transitions as observed, consumption and recovery samples |

Per row:

| Group | What to record | If not known |
| --- | --- | --- |
| Provenance | edition, executable SHA-256, installation SHA-256; `mission`/`airframe`/`loadout` identities as observed | `Unknown` + reason; `original_observed` requires the fingerprints |
| Settings | difficulty, assists (every modern assist off unless the row says otherwise, and then named), other settings | `Unknown` + reason |
| Initial conditions | altitude/attitude/speed state as observed at `t = 0`, mass/loadout configuration | `Unknown` + reason; **no assumed spawn or start state** |
| Timebase | clock, origin, time unit, measured nominal rate, measured timing uncertainty + method | all five required once samples exist; original frame pacing is measured, never assumed |
| Input/time series | `FlightInput` per sample in contract ranges plus observed quantities, each with a declared unit | empty series = unavailable |
| Units | every observed pair + (`time`, time unit), each with conversion provenance | never guessed |
| Basis | runtime observation and/or instrumented capture; the #341 file trace only as extra metadata | a behavior claim needs a flight-observing basis |
| Observer/method | who flew/observed, equipment, software, procedure | required |
| Artifacts | private artifacts (video, telemetry log, input log, screenshot) with required SHA-256 | missing file = unavailable, changed file = invalid |

## Reserve the holdout before fitting

A `ReferenceSet` names one `HoldoutReservation` — a maneuver whose record is `role = holdout` and
must never enter a fit:

1. Choose and record the reserved maneuver **before** selecting tolerances or fitting anything.
   Record the rationale in the reservation.
2. The reservation is enforced in code: a record carrying the reserved maneuver but marked
   `calibration` is a defect, and a set whose holdout is missing (or is not `original_observed`)
   never reaches `Ready`.
3. While only calibration rows exist — acceleration alone, however perfectly fitted — the outcome is
   `unavailable: reserved holdout not captured`. That is the point: **fitting acceleration alone
   cannot establish handling fidelity.**
4. `Ready` means only that a comparison can be run. F26 owns tolerances (selected before the final
   run), the deviation report and the coverage rows across airframes and loadout extremes.

## Tolerances and uncertainty

* Select tolerances from the recorded timing uncertainty and measurement method **before** fitting
  the final run; do not widen them afterwards to make a holdout pass.
* Record measurement error (camera/instrument reading, tool timestamp spread) in the row's
  `uncertainty.method`; an uncertainty with no method is refused as ambiguous.
* Do not fit one top-speed number: several independent maneuvers are required by F26, and the
  holdout is what catches overfitting.

## Coverage

Every supported airframe needs a row, including custom loadout extremes and forced mission
configurations (F26 non-negotiable behavior 5). A missing row is a coverage gap, reported as
missing — never as a pass.

## Refusals to expect

Same set as the first-mission worksheet: missing fingerprints, ambiguous timebase, non-finite
values, hash mismatches, undeclared units, source/claim contradictions, a file trace alone behind a
behavior claim, and missing observer/method — see
`2026-09-29-ref-capture-record.md`.

## Out of scope

Running the probes (#358), fitting `FlightTuning`, comparing envelopes and producing the F26
deviation report (F26-A..C), and any `verified_original` claim (F01) are separate tasks. This
worksheet fixes only what each row must state so the acquisition is reproducible and a comparison
has something honest to fail against.
