# First-mission branch worksheet (original observation)

**Task:** #357 `REF-CAPTURE-PROTOCOL`. **Acquisition task:** #358 `REF-OWNER-FIRST-CAPTURE` (blocked
until the owner can run the original).
**Companion:** `2026-09-29-ref-capture-record.md` (record fields and admission rules).

This worksheet is the `first_mission_branch` variant of a capture record: what to write down while
watching the original run its first campaign mission, so a later comparison has something exact to
compare against. It records observations only. **No branch, coordinate, timer, count or timing is
pre-filled here** — every value is either observed on the original or written `Unknown` with a
reason. Nothing in this note is evidence about the original.

## When it is used

One worksheet row (one `CaptureRecord` with `worksheet = first_mission_branch`) per observed branch
decision or per observed run, whichever gives a reproducible unit. Each row is `source =
original_observed` and `claim = original_behavior`, because these rows exist to describe the
original. A row that has not been observed yet stays `worksheet_only` and validates as
*unavailable*, not as a pass.

## Fields to capture

| Group | What to record | If not known |
| --- | --- | --- |
| Provenance | `identity`: edition, executable SHA-256, installation SHA-256 (run `cs-inspect inventory`/`audit` for the installation digest) | `Unknown` + reason — required for `original_observed`, so an unrecorded original run cannot validate |
| Mission identity | `mission`: the canonical mission id actually resolved for the run (title string + catalog/program identity as observed) | `Unknown` + reason; the research label "The Lost Treasure" is not an id (see `missions/M01.md`) |
| Branch | `worksheet.branch`: the branch identity as observed — source location/program label if the program is readable, otherwise the operator's label for the branch taken | `Unknown` + reason; never invent a program counter or opcode |
| Initial conditions | `worksheet.spawn_context`: what the run actually started with (world group/variant, initial player and wingmate configuration, loadout, difficulty, assists) as observed | `Unknown` + reason; **no assumed spawn coordinate, heading or altitude** |
| Airframe/loadout | `airframe`, `loadout` identities as observed | `Unknown` + reason |
| Timebase | `clock`, `origin` (what `t = 0` means for this run: recording start, mission start, first input), `unit`, measured `nominal_rate_hz`, measured `uncertainty` with its `method` | All five required once samples exist; the original's frame pacing is a measurement, never an assumed tick rate |
| Samples | `series`: input sequence and observed events over time (objective transitions, dialogue cues, spawn/despawn, terminal outcome), each observation with its declared quantity/unit | empty series = not captured yet = unavailable |
| Units | Every observed quantity/unit pair plus the (`time`, time unit) pair, each with provenance: reported by the capture tool, converted with a stated factor, or unknown+reason | never inferred from a guess about the original |
| Basis | What it was watched with: runtime observation (video/screen capture) and/or instrumented capture (telemetry, input log); optionally the #341 file trace as *additional* metadata | a behavior claim with no flight/mission-observing basis is refused |
| Observer/method | Who observed, with what equipment, software and procedure | required; blank/unknown = unavailable |
| Settings | difficulty, assists, other settings in effect | `Unknown` + reason; difficulty changes branches |
| Artifacts | Private artifact spellings under one private root with their required SHA-256 (video, telemetry log, input log, screenshot, trace) | no artifact = unavailable; paths only, contents stay in `private/` |

## Comparison hooks (for later, not filled in here)

* Keep the pre-transition and post-transition normalized state with the record — actor identity,
  ownership, objective state, pending events — as the M01 acceptance matrix asks (private evidence
  directory, paths and hashes only in Git).
* Two original runs differing in exactly one controlled event are the way to separate a timer
  trigger from a kill-count trigger (F13 AC04). Record both runs as separate records; do not merge
  them into one row, and keep their sources equal.
* Difficulty variants are discovered, not assumed: one worksheet row per observed variant, with the
  variant named as observed.

## Refusals to expect

| Situation | Outcome |
| --- | --- |
| Row written but no run captured (no samples, no artifacts) | `unavailable` — never a pass |
| Original claimed without edition/executable/installation fingerprints (or with a blank edition) | `invalid: missing fingerprint` |
| Samples present but clock/origin/rate/uncertainty not established, or uncertainty has no method | `invalid: ambiguous timebase` |
| A quantity observed in an undeclared unit | `invalid: undeclared unit` |
| Artifact file changed since the record was written | `invalid: artifact hash mismatch` |
| `source = synthetic_fixture` (or `remake_sample`) with `claim = original_behavior` | `invalid: source/claim mismatch` |
| Only a file-access trace (or authored synthetic data) behind a behavior claim | `invalid: no flight-observing basis` |
| Behavior row that does not state mission, airframe, loadout, difficulty or assists | `unavailable` |
| Behavior row that does not state its branch or spawn context | `unavailable` |
| Row whose samples carry no observed quantity | `unavailable` |
| Observer/method not recorded | `unavailable` |

## Out of scope

Acquiring the run itself (#358), deciding what the observations mean for the mission IR (F13/F37),
and any `verified_original` claim (F01 ledger) are separate tasks. This worksheet only fixes what a
record must say so that the acquisition, when it happens, is reproducible and can fail loudly.
