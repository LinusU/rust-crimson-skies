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
`0x4055c0`/`0x405700` (rows at `0x619e68`, stride 28), the two hardpoint box
counts (`record + 0x34`, `+ 0x38`, priced flat per box) and the four armor
counters at `record + 0x74..0x80`. **Neither function reads
`record + 0xa8` (the rocket slots).** The rocket and hardpoint weight/cost named
by the engine dictionary (`ohardpointweight`, `ohardpointcost`) are therefore not
part of these totals. The parent task (T452-R.03) completed the measurement:
the hardpoint rows `E 2259..2261` and the totals charge a flat **480 weight /
410 cost per enabled rocket box** and no instruction reads a type-dependent
value — there is no per-type weight or cost table in the purchase path
(`docs/findings/2026-10-28-t452-r03-hardpoint-price-and-gate-table.md`).

## 4. The reader surface (T452-R.03-A, task #2164)

`cs_content::ordnance` now carries the map as `ORIGINAL_ROCKET_ID_WEAPON_INDEX`
(`[5, 6, 7, 8, 9, 15, 13, 12, 10, 11, 14]`, VAs `ORIGINAL_ROCKET_ID_FUNCTION_VA`
0x4440f0 and `ORIGINAL_ROCKET_ID_JUMP_TABLE_VA` 0x444148, provenance `static`),
`original_rocket_weapon_record(id)` and `read_original_rocket_type_table`, which
resolves each of `wep_05`..`wep_15` through the production `weapons.zrd` reader.
Retail result: all eleven records exist and each declares a non-empty `NAME`.
The classified records no rocket id reaches (spawn-only for the player's
selection) are exactly `wep_04`, `wep_24`, `wep_25`, `wep_26`, `wep_27`. The
ignored image test re-measures the mapper's head bytes and every jump target.
The rows are also in `data/owner-static/engine-facts.toml` (`[[rocket_id_fact]]`).

## 5. Where the per-slot stack count comes from (static, partial)

The loadout binder (`0x443de0`) fills two engine arrays, four gun weapon indices
at `0x62ae28` and eight rocket weapon indices at `0x62ae38` (the eight rocket
slots), plus four flag bytes at `0x62ae58`; `0x4b24d0` then installs every slot
through `0x4b2550(entity, slot, weapon index, -1, flag)`. For a slot with an
index, `0x4b2550` takes the slot's count from `0x4bad90(index)`: it formats
`wep_%02d` (format string at `0x62b394`), looks the record up by name in the
global record list (`0x5abfd0`, list `0xa1d78c`, stride 0x214) and returns the
dword at `(record + 0x210) + 0xc`. Only for the four **gun** slots without the
flag byte is that count scaled by 0.5 (`0x6032e0`) and rounded; the **rocket**
slots (slot - 4, entity rows from `+0x544`, stride 0x18) store the unscaled
count at row `+0xc` and again at `+0x10`. So:

- there is **no per-type stack-count table, hardpoint weight or cost table** in
  the plane-record or loadout path: a rocket slot's count is a field of the
  `wep_` record's runtime object, and the same count applies whichever slot or
  airframe holds the type;
- the string `ohardpointweight`/`ohardpointcost` does not occur in the engine
  image at all (searched as bytes), so those dictionary names are script-side;
  no engine read of such a value was found.

**Established since:** the key that fills `(record + 0x210) + 0xc` is
`CLUSTER_SIZE` (T452-R.03-B, `0x4baa17`); `AMMO_LIMIT` parses to `record +
0x18` with default 50.0 and has no found consumer. The `FIRE`/`FLYOUT`/
`IMPACT`/`DESTROY_ANIMATION` media mapping is measured (T452-R.03-C) and the
equipment-rule census is measured empty (T452-R.03-D). Of the purchase
handlers, `E 2245` (`0x40ad0f`) and `E 2031` (`0x409aec`) are now pinned —
both treat `record + 0x34`/`+ 0x38` as the two sides' enabled-box counts —
while `2243`/`2230`/`5058`/`2037` remain unread; the purchase side (section 3)
already showed no rocket weight or cost is summed, and the parent measured
the flat per-box price that is.
