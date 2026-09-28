# Reference capture record protocol (worksheet and admission rules)

**Task:** #357 `REF-CAPTURE-PROTOCOL` — early reference preparation, not original verification.
**Status:** protocol and validator implemented; **no original capture exists yet**. Real first-mission
and baseline-flight acquisition is #358 `REF-OWNER-FIRST-CAPTURE`.
**Code:** `tools/cs_inspect/src/reference_capture.rs`; tests
`tools/cs_inspect/tests/accept_ref_capture_protocol.rs` (prefix `accept_ref_capture_protocol_`).

This note is the operator side of the record: what to write down, what must stay unknown, and how the
validator classifies a record. It deliberately contains **no original unit, spawn coordinate, tick
rate or timing** — those are captured per record by whoever runs the original, or recorded as
`Unknown` with a reason. Nothing here is evidence about the original game.

## Why a record, and what it is not

A capture record is one row of observed data plus everything needed to reproduce or reject it. It is
not a claim: `verified_original` still requires the F01 evidence ledger, and a structurally valid
record only says the data is complete and self-consistent. Two layers stay separate on purpose:

| Layer | Question it answers | Where it lives |
| --- | --- | --- |
| Capture record | Was this measurement captured, from what, on which clock, in which unit, and does the file on disk still hash to what was recorded? | `cs_inspect::reference_capture` |
| Evidence ledger | Does this observation back `verified_original` for a compatibility claim? | `cs_types::evidence` |

## Record fields

Every field is present in the type; "not captured yet" is written `Unknown` with a reason, never
left out and never defaulted to a plausible value.

| Field | Content | Rules |
| --- | --- | --- |
| `id` | Stable record id within its reference set | — |
| `source` | `original_observed` / `remake_sample` / `synthetic_fixture` | must agree with `claim`; must equal the series' source |
| `claim` | `original_behavior` / `remake_behavior` / `worksheet_only` | `original_behavior` requires `original_observed` **and** a flight-observing basis |
| `role` | `calibration` or `holdout` | a record carrying the reserved maneuver may not be `calibration` |
| `identity` | original edition, executable SHA-256, installation SHA-256 | all three required for `original_observed`; `Unknown`+reason otherwise |
| `settings` | difficulty, assists, other settings | recorded, never defaulted |
| `mission`, `airframe`, `loadout` | original identities as observed | `Unknown`+reason until measured; no guessed catalog ids |
| `worksheet` | `first_mission_branch` (branch + spawn context) or `baseline_flight` (probe maneuver) | see the two companion worksheets |
| `timebase` | clock, origin (what `t = 0` means), time unit, nominal sample rate, **measured** timing uncertainty and its method | all five required once samples exist; a non-positive rate, an empty origin/clock/unit or an uncertainty without a measurement method is ambiguous |
| `units` | quantity/unit pairs with conversion provenance (reported by tool / converted with a factor / unknown+reason) | every observed pair and the time unit must be declared; no unit is inferred |
| `series` | input/time samples and their source | empty series = no capture yet |
| `basis` | what the record was observed with: runtime observation, instrumented capture, file-access trace, synthetic simulation | non-empty for a usable record; only the first two observe flight |
| `observer`, `capture_method` | who observed, with what equipment/software/procedure | required for a usable record |
| `artifacts` | private artifact spellings + required SHA-256 + role | `/`-relative under one private root; no `..`, no absolute paths; re-hashed on every validation |
| `notes` | free-form, including fixture/limitation notes | — |

`FlightInput` uses the FLIGHT-PHYSICS contract ranges (`throttle [0,1]`, `pitch/roll/yaw [-1,1]`,
`boost` bool). Those ranges are **this project's contract**, not a claim about the original's input
encoding: a capture of raw axes converts them and declares the conversion in `units`.

## Admission outcomes

`validate_capture(record, context)` returns all three lists; a record is valid only when both are
empty. Defects are reported even when data is also missing.

* **Invalid (defect, must be corrected):** missing fingerprint, ambiguous timebase, non-finite
  value, artifact hash mismatch, unsafe artifact path, undeclared unit, unusable conversion factor,
  source/claim contradiction (a synthetic record labeled original), mixed original/remake sources,
  a behavior claim whose only basis is a file trace, the reserved holdout used as calibration data.
* **Unavailable (data not captured yet, never a pass):** no samples, no artifact, artifact root not
  supplied (unchecked ≠ matching), artifact not readable, missing observer/method/basis, no
  calibration record, reserved holdout not captured, holdout not original-observed.
* **Valid:** everything present, consistent and re-hashed.

"Missing capture data is unavailable not pass" is enforced by the unavailable branch, not by
convention: an empty worksheet, a missing file and an unchecked root all land there and can never
be counted as a pass.

## The reserved holdout

`ReferenceSet` carries a `HoldoutReservation` — one maneuver held back from every fit. The rule is
structural: `fidelity_comparison` stays `Unavailable` (holdout not captured) while a set holds only
calibration records, so **fitting acceleration alone can never establish handling fidelity**. When
the reserved maneuver has been captured as original data the outcome becomes `Ready`, which means
only that a comparison *can be run* — never that a tolerance was met. F26 owns the comparison, its
tolerances and its deviation reports.

Reserve the maneuver **before** selecting tolerances or fitting, and record the rationale in the
set. Do not read the holdout while fitting.

## Shared metadata with #341 (file-access trace)

Rally #341 `F04-D-original-order` requests a Process Monitor-style file-access trace of the original
executing: which colliding files are opened, in what order, and whether a lookup falls back between
directory levels. That trace is shared with this protocol as a `FileAccessTraceRef` so both
workstreams fingerprint the same private file:

| Field | Content |
| --- | --- |
| `task` | `Rally #341` |
| `tool` | capture tool and version (e.g. Process Monitor) |
| `platform` | host platform the trace was taken on (e.g. Windows version) |
| `relative_path` | spelling under the shared private artifact root — paths only |
| `sha256` | required digest of the trace file; re-verified like any artifact |
| `covers` | world/mission groups covered (e.g. `ZBD/C1`, `C1B`, …) as recorded |

Only paths, hashes and tool/platform identity are shared; the trace contents stay in `private/`.

**Limitation, enforced in code:** a file trace records *which files the original opened*, never
*how the aircraft flew* or *why a mission branch was taken*. A record offered as
`original_behavior` whose only basis is a file trace is refused
(`FileTraceCannotSupportFlightBehavior`). The same record shared as `worksheet_only` metadata
validates. The trace therefore cannot move `PRECEDENCE_ORDER_STATUS` or any handling claim by
itself; #341's own acceptance (summarise, compare with `SessionBuilder::mount_installation`, file
mismatches) still applies.

## Companion worksheets

* `2026-09-29-ref-first-mission-branch-worksheet.md` — what the first original mission run records.
* `2026-09-29-ref-baseline-flight-probe-worksheet.md` — what each baseline flight probe records and
  how the holdout is reserved.
