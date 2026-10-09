# M07-B-FU1: who writes the `DANGER_ZONES_COMPLETED` flag bytes

Date: 2026-10-09. Task: M07-B-FU1 "Lower DANGER_ZONES_COMPLETED conditions
from the measured flag evaluator" (#813, `missions/M07.md`, work order
`M07-B`). Shared contract: `docs/contracts/SCRIPT-MISSION.md`. Capability used:
`retail` for the record shapes and `CS_ENGINE_IMAGE` (the owner-supplied
decrypted executable, read-only, never committed) for the native side.

Follow-up of M07-B (#277), which left one lowering gap: the three
`DANGER_ZONES_COMPLETED` sites (blocks 37, 39, 41 of
`ZBD/C2/M02/zrdr.zbd`'s `objectives.zrd`) refused because "the danger-zones
flag evaluator is measured but this build lowers no condition for it"
(`crates/cs_script/src/conditions.rs`). The evaluator itself was already
measured (finding B); what was not measured is **who writes the flag bytes it
reads**. This note is that measurement, and the lowering it backs.

Everything native is static code evidence from `$CS_ENGINE_IMAGE` (image base
`0x400000`); no original executable was run and nothing here is
`verified_original` (AGENTS.md rule 8).

## What the evaluator reads, re-derived

`0x469ab0`, called from pass 2 of `CZMission::Update` at `0x46a884`, between
the `INACTIVE*` evaluator (`0x469a60`) and `ANIM_STATE` (`0x4697a0`):

```
count = rec->+0x56c                       ; zone count, 0 -> return false
flags = rec->+0x574                       ; byte pointer, one byte per zone
nonzero = |{ i < count : flags[i] != 0 }| ; signed compare
return nonzero >= rec->+0x568             ; `cmp esi,ecx; setge al`
```

So the predicate is fully known: *at least `+0x568` of the `+0x56c` flag bytes
are nonzero*, with `count == 0` answering `false` before any byte is read.
Because the compare is signed, a threshold at or below 0 fires on the first
armed tick — exactly what a threshold of 0 says, so the lowering stores
`count.max(0)`.

## Who fills the fields: `0x465ec0`, once per block

The objective-block parse (`0x468e44`) calls `0x465ec0(record, block, …)`
exactly once per block, beside the `ANIM_STATE` helper (`0x4691d0`). Its two
lookups are the record lookup `0x57a090` → `0x579ff0(container, key, 1)`, both
on **the block's own node**, so each takes the *first* occurrence of its key in
the block's depth-first order and returns the record right after it (`0x579ff0`
requires `[container] == 4`, descends into list children recursively, and
compares key texts case-sensitively).

| Field | Written by `0x465ec0` | Meaning, measured |
| --- | --- | --- |
| `+0x56c` | `[list]` word − 1 | zone count = the value list's child count |
| `+0x570` | `malloc(count*4)`, one `_strdup(child_i payload)` per child | **the zone-name pointer array** — this closes the "+0x570 … parser-side storage whose use is untraced" unknown `cs_content::mission_control` recorded |
| `+0x574` | `malloc(count)`, then `byte[i] = 0` in the same loop | the flag array, zeroed at parse |
| `+0x568` | first child payload of the block's first `DANGER_ZONES_COMPLETION_COUNT`, else `+0x56c` | the threshold (default = listed count) |

The threshold read (`[found+4]` → `[V+0xc]`, i.e. the value list's **first
child payload**) does not check that child's tag, and the zone-name read does
not check either: a non-string child would be `_strdup`ed as if it were a
name. No measured site spells that — T464 counts `DANGER_ZONES_COMPLETION_COUNT`
at six blocks, four times `1`, once `3`, once `4`, all ints — so the lowering
refuses such a shape rather than reproduce a read this build cannot state.
A `DANGER_ZONES_COMPLETED` text followed by no value list makes the parse take
whatever record comes next as the zone vector; that is refused too, and it is
the reason the bare-spelling refusal `accept_m01_lc_lowering_conditions_…`
pins still holds.

Parse, once per block, also means a **second** spelling of either key in the
same block is never reached: first occurrence wins, and the later one is
inert — the measured rule the `ANIM_STATE` lowering already follows.

## Who sets the flags: `0x446990`, the only runtime writer

A scan of every `0x574`/`0x570`/`0x56c`/`0x568` displacement instruction in
`.text` finds four writers of the flag bytes and no others:

* `0x465f4b` — parse-time zeroing (above);
* `0x446a78` — **the flag write**: `mov byte ptr [eax+edi], 1`;
* `0x4679c9` and the stack-relative hits (`0x424cd3`, `0x4387ad`, …) — the
  fresh-record zeroing (`rep stosd` over one 0x5e4-byte objective record) and
  unrelated stack buffers, neither touching an existing flag array;
* `0x466383` — mission teardown: free the array, store 0 into `+0x574`.

Nothing writes a flag byte back to `0`: **a flag, once set, stays set for the
mission's life**, which is why the lowering reads a sticky set rather than a
per-tick report.

The write site, in the method at `0x446990`:

```
0x446a4e  for i in 0..rec->+0x56c:
0x446a5a      name = rec->+0x570[i]
0x446a63      call MSVCP60 std::operator==(const basic_string&, const char*)   ; IAT 0xa20140
0x446a70      if equal: rec->+0x574[i] = 1
```

Each record it walks is gated the same way the pass-2 gate gates it: the
record's dependency index `+0x10 < 0 || dep->+0x5c8 == 1`, `+0xc != 0` (awake)
and `+0x14 == 0` (not completed); the array is the global objective table
(`count` at `0x71c0c8`, base at `0x71c0cc`), stride `0x5e4`.

### The call chain, and what gates it

`0x446990` has exactly one caller, `0x445d70`, which loops the elements of the
global container at `0x64fb60` (stride `0x50`) and calls it per element. Its
one caller is `0x48ea6f`, inside the player-object update — `cmp edi,
[0x71c298]` skips everything unless the moving object **is** the player — and
passes two positions (the player's own vector getter's result and a summed
local vector), i.e. a motion segment.

Before the objective loop the method requires, per element:

1. byte `+0x48` set — and `0x445da0` *clears* it from the `dzones.zrd` keys
   `disable` and `nosnapshot` (strings at `0x623e14`/`0x623e1c`) and writes
   `objective_numbers` (`0x623e28`) into `+0x4c`, so these elements are the
   mission's **`dzones.zrd` zones**, and `+0x48` is "enabled";
2. a non-empty entry vector at `+0x34..+0x38`;
3. after running `0x446930` per entry (which calls `0x55d6c0` over the entry's
   12-byte item vector with the two positions, and **toggles** the entry's
   `+0x10` byte when that call returns nonzero), at least **two** entries
   carrying `+0x10` — otherwise the method returns without touching any flag.
   When the gate passes, the entry flags are cleared and `0x4b86a0(0xf)` runs
   before the objective loop.

So the write path is: *the player moved, this `dzones.zrd` zone is enabled, it
has entries, and its entry test fired for two of them* — and then the zone's
**own name** is compared with each armed objective's stored zone names and the
matching flag byte is set to 1. What `0x55d6c0` computes and why the gate
counts two entries is **not** measured; it is the producer's world test and is
carried as the residual unknown
[`cs_script::ir::DANGER_ZONE_CROSSING_TEST_UNTRACED`], never as part of the
predicate.

## What this delivers

* `crates/cs_script/src/conditions.rs` lowers `DANGER_ZONES_COMPLETED` to
  `Condition::DangerZoneFlags { zones, required }` from that measurement, and
  refuses only what the original's own read cannot state: a key with no list
  after it, a listed name that is not text, a threshold site that is not one
  int in its own list. The bare-spelling refusal that
  `accept_m01_lc_lowering_conditions_refusals_name_the_block_and_the_key`
  pins keeps its arm.
* `crates/cs_script/src/runtime.rs` reads the condition against
  `MissionFacts::danger_zones` (a sticky set), fail-closed: a zone nobody
  recorded crossing counts as unset, an empty site never fires.
* `crates/cs_app/src/world_facts.rs` carries the operand list, the host's
  per-tick report (`WorldObservation::danger_zone`) and the facts row.
* M07's blocks 37, 39, 41 lower with it; M08's eight sites (M08-B #280's note)
  and the other readers' 31 blocks across seven campaigns lower with the same
  rule, so the M08-B suite's pins are updated in this same change, never
  deleted.

## Test inventory (`accept_m07_b_fu1_`, 2 tests)

| Test | What it pins |
| --- | --- |
| `accept_m07_b_fu1_the_danger_zones_predicate_is_the_measured_flag_count` (retail) | M07's three sites carry `Condition::DangerZoneFlags` with the zone names the record spells (read back through a second production walk, never repeated from the lowering), the threshold defaulting to each block's zone count because M07 spells no `DANGER_ZONES_COMPLETION_COUNT`, and the block's own `ObjectiveAwake` gate; the condition carries exactly the named residual unknown `DANGER_ZONE_CROSSING_TEST_UNTRACED`; evaluation is fail-closed — no recorded flags and a zone the record never listed both answer `false`, the block's own zones answer `true`, and one zone short of the threshold answers `false` |
| `accept_m07_b_fu1_the_threshold_defaults_to_the_zone_count_and_the_first_site_wins` (synthetic) | the parse's own selection rules on authored records: an absent count defaults the threshold to the zone count, a spelled count stands as the threshold, and a second spelling of either key is inert because the once-per-block lookup reads the first occurrence in the block's depth-first order — all three shapes lower, bind and validate |

Names updated in this change (never deleted to get green), with their evidence
lists updated beside them:

| Was | Is | Why |
| --- | --- | --- |
| `accept_m07_b_the_anim_state_gap_closes_and_danger_zones_is_the_remaining_one` | `accept_m07_b_every_block_lowers_including_the_three_danger_zones_sites` | there is no remaining refusal: 61 of 61 conditions lower, `validate` accepts and the row completes |
| `accept_m07_b_the_mission_stays_unready_and_the_campaign_gate_stays_closed` | `accept_m07_b_m07s_row_is_complete_and_the_campaign_gate_stays_closed` | M07's row is complete; the gate is still closed, now on the other rows' gaps (which the test proves exist) |
| `accept_m07_b_a_danger_zones_site_refuses_its_condition_and_the_block_without_it_lowers` | `accept_m07_b_a_danger_zones_site_lowers_its_predicate_and_an_unreadable_one_refuses` | the measured shape lowers; the refusal arm stays, on the site whose record after the key the parse cannot read |
| `accept_m08_b_the_danger_zones_condition_is_the_gap_that_keeps_m08_unlowered` | `accept_m08_b_m08s_record_lowers_completely_and_the_gate_stays_closed_on_other_rows` | the eight conditions lower, so M08's row completes |
| `accept_m08_b_the_danger_zones_condition_refuses_while_a_measured_condition_lowers` | `accept_m08_b_a_danger_zones_condition_lowers_and_an_unreadable_one_refuses` | same synthetic arm as M07's, on M08's own helpers |

The M07-B and M08-B findings documents and their committed
`docs/findings/evidence/*.json` reports describe the run they were written
for; this note supersedes them on the danger-zones gap only.


## Recorded unknowns (not guessed)

* **The flag producer's world test is untraced** — `0x55d6c0` and the
  two-flagged-entry gate inside `0x446990` (`DANGER_ZONE_CROSSING_TEST_UNTRACED`).
  Which crossing sets a flag, and whether a `dzpath<N>` name is a path volume
  or a node, stays open; the predicate never depends on it.
* **`0x4b86a0(0xf)`** — the call the gate runs before updating flags — is
  unidentified; it is a host effect beside the flag write, not inside the
  predicate.
* **No directive is implemented by a measured effect.** The condition lowers
  and evaluates against reported facts; nothing here runs the original code.
* **No original executable was run**, no mission was played, and nothing is
  `verified_original` or `human_play`.

## Sources

`$CS_ENGINE_IMAGE` (decrypted image, read-only) through capstone disassembly of
`.text`; `$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`SourceContext::read`/`control_program`, `measure_control_record`,
`lower_control_record`, `survey_mission_control_programs`;
`missions/M07.md`; `docs/contracts/SCRIPT-MISSION.md`;
`docs/contracts/CLI-EVIDENCE.md`;
`docs/findings/2026-10-06-m01-lc-directive-a-objective-directive-parser.md`;
`docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`;
`docs/findings/2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives.md`;
`docs/findings/2026-10-09-m07-b-compatibility-gaps.md`;
`docs/findings/2026-10-09-m08-b-compatibility-gaps.md`;
`docs/findings/2026-10-02-t463-stunt-encoding-and-gate-geometry.md`;
`docs/findings/2026-10-02-t464-stunt-reward-and-repeat.md`;
`docs/findings/2026-10-05-t513-dzones-framing.md`.
