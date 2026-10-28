# T452-R.03 (partial): the rocket-type -> weapon-record map and what the purchase totals read

Date: 2026-10-28. Task #2161. Capabilities used: `retail`, and static reading of
`$CS_ENGINE_IMAGE` (image base 0x400000). Provenance of every row: `static`,
never `verified_original`. Nothing was run. Offsets are virtual addresses in the
decrypted image; no disassembly or script text is reproduced.

## 1. The eleven selectable rocket types map to `wep_05`..`wep_15`

The loadout binder (`0x443de0`, see `[gun_loadout]` in
`data/owner-static/engine-facts.toml`) turns each plane-record rocket id
(`record + 0xa8 + 4*slot`) into a weapon index by calling `0x4440f0`. That
function range-checks the id (`> 10` answers -1, the "none" id 11 included) and
jumps through the dword table at `0x444148`; each target is `mov eax, N; ret`:

| rocket id | weapon index | | rocket id | weapon index |
| --- | --- | --- | --- | --- |
| 0 | 5 | | 6 | 13 |
| 1 | 6 | | 7 | 12 |
| 2 | 7 | | 8 | 10 |
| 3 | 8 | | 9 | 11 |
| 4 | 9 | | 10 | 14 |
| 5 | 15 | | 11 | none (-1) |

So the player-selectable set is `wep_05`..`wep_15` (eleven records), **including**
`wep_12`, `wep_13` and `wep_15`. `wep_04`, `wep_24`, `wep_25`, `wep_26`, `wep_27`
are **not** reachable through a rocket id: they are spawn-only as far as this
table is concerned. The names shown for an id are string ids `3380 + id` (long,
handler for callback 5019 at `0x410b30`) and `3395 + id` (short), as already
recorded.

## 2. The ordnance screens carry names only

Callback 5019 (`0x40e5d1` -> `0x410b30`) returns count 11 and the name string
`3380 + index`. Callback 5058 (`0x40edc6..0x40ee15`) loops eleven times building
a name row through `0x40fdc0` (string `3380 + id`, or `3395 + id` for mode 1).
Neither returns a stack count, weight or cost.

## 3. The purchase weight and cost totals do not read the rocket slots

The totals the purchase screen shows (callback 2262, `0x40b368`) come from
`0x405550` (weight, stored at `record + 0x3c`) and `0x405680` (cost, stored at
`record + 0x28`). They read: the airframe row (`0x619bb0` stride 44), the engine
row (`0x619d98` stride 12, with `0x619e1c`), the four gun slots through
`0x4055c0`/`0x405700` (rows at `0x619e68`, stride 28), armor (`record + 0x34`,
`+ 0x38`) and four counters at `record + 0x74..0x80`. **Neither function reads
`record + 0xa8` (the rocket slots).** The rocket and hardpoint weight/cost named
by the engine dictionary (`ohardpointweight`, `ohardpointcost`) are therefore not
part of these totals. The per-type weight and cost table, if it exists, is not
where the purchase code reads.

## Still unmeasured (rule 4), for the follow-up tasks

- the per-row stack count issued per slot, the hardpoint weight and cost (not
  found in 1-3 above; the plane-construction callbacks 2243/2245/2230 are the next
  place to read);
- the `AMMO_LIMIT` parser field, its default and its consumer;
- the `FIRE`/`FLYOUT`/`IMPACT`/`SOUND_NAME` media mapping and the launch sound
  of records without `SOUND_NAME`;
- the equipment-rule census over the sixteen records;
- which `weapons.zrd` short NAME each of `wep_05`..`wep_15` is (a retail read
  through the production reader, then the reader surface in
  `crates/cs_content/src/ordnance.rs`).
