# F13-C-RECKEYS: the record-level axis in the F13-C coverage row

Date: 2026-10-12. Task: **F13-C-RECKEYS** (#1364), the follow-up from the review
of #1356 (M01-B-OPC). Shared contract: `docs/contracts/SCRIPT-MISSION.md`.
Capabilities used: `retail` (read-only `$CS_GAME_DIR`) and ordinary build/test.

## The gap this closes

M01-B-OPC classified a reader archive's control member as a text directive
program and listed its **numbered-block** directive keys
(`probe.directive_program_rows`). The same `MeasuredControlRecord` also carries
the **record-level** axis — `record_fields()` (the measured vocabulary:
`MISSION_TIMER`, `PLAYER_INIT`, the three anim lists, the sound keys) and
`unclassified_record_keys()` (keys outside both vocabularies, counted and never
read). Neither appeared in the coverage row, so a mission that spelled an
unclassified record key would have got a row whose key list looked complete
while the anomaly was dropped.

## What changed

- `cs_formats::script_raw::probe`: `RecordKeyRow`, `RecordKeyStatus`
  (`Classified { support }` / `Unknown { code, detail }`) and
  `RecordFieldSupport` (`shape_measured` / `effect_measured`). A
  `DirectiveProgramProbe` now carries `record_keys()` beside `keys()`, plus
  `classified_record_keys()` / `unclassified_record_keys()` counts.
  `ProbeSession::record_directive_program` takes the record-level listing and
  refuses it exactly like the directive axis: an empty key, a key listed twice
  and an unknown key without a reason are `invalid_directive_program`.
- `cs_inspect scripts`: `directive_program_of` builds the rows from the
  measured census (`record_fields()`, `record_field_shapes()`,
  `unclassified_record_keys()`). Classified fields carry their sites, their
  distinct measured value shapes and their support level; an unclassified key
  is listed explicitly unknown with the stable reason
  `outside_both_record_vocabularies` and **no site count** — the census counts
  unclassified keys as distinct names only, so a site count for one is not
  measured and is never invented (`"sites": null`).
- Report version `cs-inspect-scripts/4`: each directive-program row gains
  `record_keys`, `record_keys_classified`, `record_keys_unclassified`; the
  probe section gains the summed `directive_record_keys_classified` and
  `directive_record_keys_unclassified`. Record rows are rendered as
  `{"record_key": …}` — never `{"key": …}` — so the directive-key axis the M01
  listing counts by is untouched.

## The `signatures_complete` decision (asked for by the task)

**An unclassified record key does *not* count against `signatures_complete`;
it is listed and counted instead.** The check asks whether every program was
*accounted for* — whether everything it spells was read and listed with a
status. A refused **block** fails the check because it is unread text that may
spell keys the listing never shows. The record-level walk reads every flat
field of the record, so an unclassified key **is** listed: explicitly unknown
with its reason, exactly like a directive key whose meaning no finding
measures (`meaning_not_measured`), which has never counted against the check
either. The anomaly surfaces per program (`record_keys`) and in the probe
totals (`directive_record_keys_unclassified`); a caller that wants to gate on
it has the count. The synthetic acceptance pins both sides of the boundary: an
unclassified record key keeps `complete: true` under `--signatures`, while a
refused block still fails it (exit 3).

## Measured over the installation (`cs-inspect scripts --coverage`, report v4)

1452 located programs, 40 directive programs (unchanged from M01-B-OPC); 773
directive keys measured or terminal, 6 unknown (unchanged). Record-level axis:
**205 classified rows, 0 unclassified anywhere in the retail corpus** — this
answers M01-B-OPC's open question ("whether any mission does is not answered
there"): no retail mission spells a record key outside both vocabularies.

- 39 of the 40 programs spell exactly the five shape-measured fields, each at
  one site: `MISSION_TIMER` (`[float]`), `PLAYER_INIT`
  (`[int,[float,float,float],[float,float,float],float,float]`),
  `RESTORE_ANIMS`/`EXECUTE_ANIMS`/`INVALIDATE_ANIMS` (`[]`).
- `zbd/c1/m02` is the one program that also spells record-level sound keys:
  all five of `PRIMARY_COMPLETE_SOUND`, `SECONDARY_COMPLETE_SOUND`,
  `TERTIARY_COMPLETE_SOUND`, `MISSION_WON_SOUND`, `MISSION_LOST_SOUND`, each
  `effect_measured` with shape `[text]` (M02-B-FU2's measured set).
- **M01 (`zbd/c1c/m01`)**: its listing is unchanged — 58 blocks, 353 sites, 43
  directive keys, 0 unknown — and its record-level axis is exactly the five
  classified fields, no unclassified key (retail acceptance
  `accept_f13_c_reckeys_retail_m01_lists_five_classified_record_fields_and_no_unclassified_keys`).

## Repair done here (pre-existing staleness from #1356)

`accept_f13_c_retail_cli_probes_every_program_and_reaches_every_record` still
pinned `programs_retryable == 1452` and `"unknown_opcode": 1452` from before
M01-B-OPC: since #1356 classifies 40 control members as directive programs
instead of walking them, only 1412 binary programs stop at `unknown_opcode`
(#1356's own finding records the 1412). Both pins are corrected to 1412 with a
comment. No assertion was weakened; the retail F13-C suite passes locally.

## Not covered

- The record-level **values** are still never read by this tool: the row
  reports the census's measured shapes and support level, nothing more.
- A mission that grows an unclassified record key will be listed with the
  reason `outside_both_record_vocabularies`; classifying new record-level
  vocabulary is `cs_content` work (measured there), not this row's.
