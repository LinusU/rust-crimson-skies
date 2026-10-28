# T452-R.03: the hardpoint price and the gate table's second column

Task `#2161` (T452-R.03) integrates four subtask measurements into the parent
reader surface. This note pins the one measurement the parent itself adds —
what the purchase surface charges for hardpoints — and the dispositions of the
handlers the integration had left open.

Everything here is **static** evidence on the decrypted engine image
(`crimson.decrypted.exe`, image base `0x400000`), never `verified_original`:
no agent ran the original executable. The byte-pinned rows live in
`data/owner-static/engine-facts.toml` under `[[hardpoint_price_fact]]`; the
reader surface is `cs_content::ordnance` (`ORIGINAL_HARDPOINT_*`,
`ORIGINAL_ROCKET_GATE_*`, `original_hardpoint_price`).

## 1. The hardpoint row handler: 480 weight, 410 cost, per enabled box

The purchase screen's hardpoint rows dispatch `E 2259..2261` through
`0x40b2b4` (byte table `0x40f788`, jump table `0x40f5b4`):

- `E 2259` id `-1` counts the non-zero cells `0x64cbac` and `0x64cbb0`
  (`0x40b2ba`); `E 2259` on a row formats that side's count through string
  `1176`/`1177`. The cells are the draft plane record at `0x64cb78` +
  `0x34`/`+0x38` — the same offsets the committed record uses.
- `E 2260`/`E 2261` (`0x40b2ff`) pick the side's box count, then compute
  `(480 - 70 * (id == 2261)) * count`: **480 weight** for `E 2260`, **410
  cost** for `E 2261`, formatted through string `1175` (`"$%d"`).

So a *row* prices a whole side's enabled boxes, and the price per box is flat.
No read of the loaded rocket type appears anywhere in the handler.

## 2. The totals: the same 480/410 per box, and nothing type-dependent

`0x405550` (weight total) adds, at `0x40558e`, `(record+0x34 + record+0x38) *
15 * 8 * 4` = **480 per box** plus `4 *` the four `+0x74..+0x80` counters (the
armor zones) to the airframe, engine and gun terms. `0x405680` (cost total)
adds, at `0x4056aa`..`0x4056ef`, `(record+0x34 + record+0x38) * 41 * 5 * 2` =
**410 per box** to the same terms. Neither function reads `record + 0xa8` —
the rocket slots — and no instruction in either reads a weapon record field.

The measured answer to "the hardpoint weight and cost" is therefore:

- each *enabled rocket box* costs 480 weight and 410 cost, on each of the two
  hardpoint sides (`DT[2]` holds the two counts; `RKA[8]` is the 4+4 slot
  array at `+0xa8`);
- the *type* loaded into a box contributes nothing — there is no per-type
  ordnance mass or cost table in the purchase path to pin. The measured value
  is the flat per-box price plus this absence, not an invented table.

The `ohardpointweight`/`ohardpointcost` dictionary names the parity5 finding
described do not occur as strings in the image; the callbacks above are what
the script row reaches.

## 3. The box counts are enabled-box counts (`E 2245`, `E 2031`)

- `E 2245` (`0x40ad0f`) get/sets a side's count. Its set path (`0x40ad4f`)
  writes the count at `0x64cbac`/`0x64cbb0` and fills the side's four draft
  rocket slots (`0x64cc20 + 16*point` = the draft record's `+0xa8` array)
  with a random `0`/`1` — the two starting rocket ids — or `11` (the none
  id) when the count is 0.
- `E 2031` (`0x409aec`), the Instant Action rocket-name handler, reads a
  slot's rocket id at `record + 0xa8` (`0x409b09`), reports it through the
  gate-table lookup `0x40ffa0`, and answers `-1` unless the slot index is
  below the side's count: slots 0..4 compare `record + 0x34` (`0x409b3b`),
  slots 4..8 compare `record + 0x38 + 4` (`0x409b54`). The two record fields
  are per-side counts of *enabled* rocket boxes.

## 4. The Instant Action gate table's second column is unread

The twelve-row table at `0x619efc` (facts `[ordinance_feed]` /
`[[ordinance_rocket_entry]]`, F2066-B) gates each rocket id on campaign
progress: a row passes when any flag cell `0x647b5c`/`0x647b68`/`0x647b6c` is
set, or when `first <= progress + 1` against `0x64b678`. The accessors
`0x40ff00` (count), `0x40ff50` (nth passing), `0x40ffa0` (record index → row)
are the only code references to the table, and each reads the row's first
dword only.

Every code reference to the `aux` dwords is inside a static data table — no
instruction reads `[ecx+4]`. Row 11's second dword is not table data at all:
`0x619f58` is the base of the 204-byte plane-record table (`lea esi,
[ecx*4+0x619f58]` copies `0x33` dwords, e.g. `0x40ab16`). The aux values
`4, 4, 2, 2, 2, 2, 2, 4, 4, 1, 0` match no `weapons.zrd` key of the mapped
records (not `CLUSTER_SIZE`, not `AMMO_LIMIT`, not any declared key — checked
against the full 41-key union). What the column means is **unknown**; nothing
in the image reads it.

## 5. What the integration resolves, and what stays open

Resolved (each with VA-pinned static evidence on the reader surface):

- rocket id → record map (`0x4440f0`, `0x444148`) and the unreachable
  spawn-only records — T452-R.03-A;
- the per-slot stack count is `CLUSTER_SIZE`, and `AMMO_LIMIT` is a separate
  parsed field (`record + 0x18`, default 50.0) with no found consumer —
  T452-R.03-B;
- the media ids map through the `FIRE`/`FLYOUT`/`IMPACT`/`DESTROY_ANIMATION`
  effect blocks; the launch sound is the record's own `FIRE>SOUND[0]` —
  T452-R.03-C;
- the equipment-rule census over the sixteen catalogued records is empty —
  T452-R.03-D;
- the hardpoint price: 480 weight / 410 cost per enabled box, no
  type-dependent term; box counts at record `+0x34`/`+0x38` — this note.

Still open (recorded, not fabricated):

- `AMMO_LIMIT`'s runtime consumer (the static scan is negative evidence, not
  proof of absence);
- the `FLYOUT>SOUND` consumer;
- the runtime stack decrement/reload behavior;
- the gate table's `aux` column;
- the weight unit's conversion to kilograms;
- the logical `snd_*` names → wave members (`sounds.zrd`) mapping;
- purchase callbacks `2243`/`2230` and handlers `5058`/`2037` (unread; not on
  the price path);
- registering the catalogued components as flyable runtime components —
  `t452.limit.runtime_lowering` stays until T452-RUNTIME-REGISTER (#2156)
  performs the lowering.
