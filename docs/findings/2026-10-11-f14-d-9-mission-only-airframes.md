# F14-D.9 (#564): the per-chapter load scripts declare no airframe

Date: 2026-10-11. Task #564, follow-up of #489 (F14-D.6) and #399 (F11-D2);
extends `2026-10-05-f11-d-2-1-chapter-load-scripts.md`. Capability `retail`,
read-only. Nothing was run, rendered or played; nothing here is
`verified_original`.

## Verdict

**The per-chapter shape does not identify airframes, so no mission-only
`ContentKind::Airframe` row is added.** The `airframe` collection keeps its
eleven shared rows, the coverage denominator does not move, and the collection's
`gaps` now record what was read so "read and found nothing" differs from "not
read".

## Correction to the F11-D2.1 measurement

F11-D2.1 read five scripts (`c1`..`c5`, 344 loads). `ZBD/interp.zbd` holds **eight**
per-chapter load scripts; `c1b`, `c1c` and `c2b` were missed.

| script | `LoadGameGen` | `NewObject3D` |
| --- | --- | --- |
| `support\c1\load.gw` | 55 | 2 |
| `support\c1b\load.gw` | 22 | 0 |
| `support\c1c\load.gw` | 23 | 0 |
| `support\c2\load.gw` | 86 | 0 |
| `support\c2b\load.gw` | 19 | 0 |
| `support\c3\load.gw` | 53 | 1 |
| `support\c4\load.gw` | 50 | 2 |
| `support\c5\load.gw` | 100 | 2 |
| **total** | **408** | 9 |

All 408 lines store exactly `<model> <alias>`; 0 are unreadable. 8 spell a model
the shared roster declares (another instance of a shared airframe), **400** have
no shared-roster equal and stay unclassified. Their aliases are bare load names
(`name_1`), not roots a roster could reference. The `NewObject3D` lines name
mission-local scene roots, not `%planeOutput%`: the shared declaration shape does
not occur in any chapter script. `support\load.gw` (127 loads) and
`support\planes.gw` are shared scripts, not per-chapter, and are out of this slice.

## What changed

`cs_content::catalog::baseline` runs the existing production
`discover_mission_only_airframes` over `CHAPTER_LOAD_SCRIPTS` while building the
airframe collection, and records `chapter_load`, `chapter_load_shared`,
`chapter_load_unclassified`, `mission_only_airframe` (always 0),
`mission_only_unknown` and the `chapter_*` issue counts in the collection's gaps.
No row is produced from a model name or alias (F11 non-negotiable behavior 3).

## Tests

`accept_f14_d_9_*` in `crates/cs_content/tests/accept_f14_d_9_mission_only_airframes.rs`:
three synthetic (counts and no row; unreadable line is a finding; denominator
unmoved) and one `#[ignore]`d retail test pinning 408 / 8 / 400 / 0. The prefix is
shared with the F14-D.9 material tests. Evidence:
`docs/findings/evidence/F14-D.9.mission-only.json`.

## Limitations

- **Mission-only airframes undiscovered.** Affected: every airframe a mission
  spawns outside the eleven shared ones. Resolving: F39 / F13 stages.
- **53 303 per-chapter node records unattributed.** Resolving: F18 world import.
- **`support\load.gw` not read.** Resolving: F11 scene-roster stages.
- Library/inventory path only; evidence class `observed_tool`; no independent review yet.
