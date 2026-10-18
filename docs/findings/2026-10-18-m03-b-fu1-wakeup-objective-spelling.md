# M03-B-FU1: the original does not recognise `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE`

Date: 2026-10-18. Task: M03-B-FU1 (#803). Parent: M03-B (#265); the gap is
recorded in `docs/findings/2026-10-08-m03-b-control-program-gaps.md`.
Capability used: `retail` (read-only `$CS_GAME_DIR`, `$CS_ENGINE_IMAGE`).
Implementer: sonnet-1. **Static evidence only**: the original was never run, so
nothing here is `verified_original` runtime behaviour.

## Question

`zbd/c1b/m03` `objectives.zrd` spells `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` twice
(block 13 `[14,15]`, block 14 `[49]`, 1-based). The measured parser keys are
`WAKE_OBJECTIVE` and `WAKE_OBJECTIVE_WHEN_I_COMPLETE`
(`2026-10-06-m01-lc-directive-a-objective-directive-parser.md`). Does the
original accept the `WAKEUP_` spelling as an alias, or ignore it?

## Measured

| Fact | Value |
| --- | --- |
| File | `$CS_ENGINE_IMAGE`, SHA-256 `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, image base `0x400000` |
| `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` in the image | **absent**: no match as ASCII, as UTF-16LE, or as a substring `UP_OBJECTIVE` anywhere in the file |
| Other `WAKEUP` strings | `WAKEUP_ENEMIES` `0x626664`, `WAKEUP_TURRETS` `0x626684`, `WAKEUP_ZEP_TURRETS` `0x626694`, `WAKEUP_GENERATOR` `0x62678c`, `WAKEUP_SOUND_GROUP` `0x6267b8`, `ON_WAKEUP` `0x626bdf`: each a whole key, none an objective key |
| Objective wake keys in the table | `WAKE_OBJECTIVE` `0x626840` (xref in the parser at `0x468c11`), `WAKE_OBJECTIVE_WHEN_I_COMPLETE` `0x626850` (xref `0x468c27`), `WAKE_OBJECTIVE_WHEN_I_SLEEP` `0x6269c0` |
| Key construction | the only `OBJ…` format strings are `OBJECTIVE%d` (`0x6260c0`, `0x629ee4`); no format string composes a `WAKEUP…` objective key at runtime |
| Lookup | every key is reached by `0x57a090(container, "KEY")`, an inline case-sensitive exact `strcmp` (finding A, "In-memory node model") |

The parser never asks the record for `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE`, and the
record's keys it does not look up "are ignored" (finding A). The two M03 sites
therefore have **no effect** in the original: no `+0x1c` wake array is written
from them, so blocks 14, 15 and 49 are not woken by blocks 13 and 14 through this
key. Their reachability is whatever the other (measured) keys give them.

## Residual limit

The conclusion rests on the string table and the parser's exact-match lookup.
No prefix or substring match was found in any dispatch, but a runtime
observation (a `verified_original` run of M03 blocks 13/14) has not been made.
M02-B-FU3's "tests read a spelled value as the index" fix is unaffected.

## What changed

* `cs_content::mission_control::ignored_directive` names the key;
  `MeasuredDirectiveKey::disposition` returns
  `Unmeasured { reason: UnmeasuredReason::IgnoredByOriginal }` for it, a
  measured absence rather than `MeaningNotMeasured`.
* The lowering adapter (`cs_app::control_lowering`) drops its sites as the
  original does: no `RawCall`, no condition directive, counted in
  `LoweringAttempt::ignored_sites`. The `call_arguments` row accounts for them
  and no longer refuses the key. M03's record therefore has no refused site.
