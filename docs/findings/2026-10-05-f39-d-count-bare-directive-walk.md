# F39-D-COUNT: the flat block walk swallowed the key after a bare directive

## What was wrong

`cs_content::stunts::zrd_flat_fields` reads `[key, value, key, value, …]`. An
objective block does not have that shape: a directive the original spells
**bare** (`INSTANTWIN`, `INSTANTLOSS`, a few English words) is followed directly
by the next key, so the flat walk paired the bare key with the next key's
spelling and skipped that key. The swallowed key was counted nowhere.

## What changed

`cs_content::stunts::zrd_directive_fields` walks a block with the measured
grammar (a text followed by a text, or by the end, is bare and advances by one;
anything else is its argument). `objective_state_machine`,
`measure_dormant_declarations`, `measure_block_precedence`,
`measure_count_conditions`, `measure_detached_vocabulary` and
`ObjectiveRecovery::read` use it for the keys *inside* a block. The outer
(block key, block) walk stays on `zrd_flat_fields`, which is right there.
`zrd_flat_fields` is kept for the flat records (`ia.zrd`, scenario, animation
programs) and documents its assumption. `scope_member_keys` still uses it
recursively and is **not** corrected here; see Open.

## Re-measured on `b4e780ab…` (53 readers, 1338 blocks, unchanged)

| number | published | corrected |
| --- | ---: | ---: |
| blocks carrying `BEGIN_DORMANT` | 1096 | **1118** |
| … with the `-1` sentinel | 992 | **1014** |
| … with a positive argument | 104 | 104 |
| sentinel blocks with no stage | 836 | **858** |
| blocks declaring neither dormancy nor stage | 127 | **105** |
| dormant blocks that also carry an `IDENTITY` | 86 | **91** |
| M01 fields read by `ObjectiveRecovery` | 356 | **358** |

Unchanged (re-run, tests pass): 1335 `INACTIVE<n>` declarations, 271 staged
blocks, 156 sentinel blocks with stages, 115 staged non-dormant blocks, 130
`INACTIVE_COMPLETION_COUNT`, 112 `IDENTITY` declarations, and F39-E2/E4/E7's
numbers.

## F39-E1 conclusions

All 22 swallowed `BEGIN_DORMANT` sites carry the `-1` sentinel (992 + 22 = 1014;
the dated 104 is unchanged), so the positive-argument domain, the 41 distinct
values and the single fractional `13.5` stand. The sentinel family is larger by
22; the disjointness of dated and staged blocks still holds (asserted). The five
families still partition the 1338 blocks.

## Open

* `scope_member_keys` (F39-E3 installation-scope inventory) walks every nested
  record with `zrd_flat_fields` and may undercount keys the same way; its
  numbers are not re-measured here.
* Findings F39-D, F39-E1, F39-E2 and M01-LC quote the old numbers in prose; each
  carries a pointer to this note rather than a silent rewrite.
