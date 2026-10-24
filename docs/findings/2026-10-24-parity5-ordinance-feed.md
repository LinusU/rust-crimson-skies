# The OrdinanceLayout feed from a plane record

2026-10-24, task #2069 (F2066-B, follow-up to #2062 / #2066). Implemented by
`sonnet-1`. Static readings of the owner-supplied engine image
(`CS_ENGINE_IMAGE`) and the installation's string table: nothing here is
`verified_original`, the original executable was not run, no disassembly is
committed. No independent review yet.

## What shipped

* `crates/cs_app/src/ui/front_end/ordinance_feed.rs` (pure, no Bevy):
  `ordinance_feed(facts, record, gate, text) -> OrdinanceFeed`, `PlaneRecord`
  (guns `+0x88`, ammunition `+0x98`, rockets `+0xa8`, box counts `+0x34` /
  `+0x38`; `fresh_campaign` builds the pilot's or wingman's committed record),
  `RocketTable` (`passes`, `count`, `nth`, `row_of` = `0x40ff00`, `0x40ff50`,
  `0x40ffa0` under a `ProgressGate`) and `OrdinanceDraft` (`E 2035` load,
  `E 2036` / `E 2037` stores into a working copy, `E 2034` commit).
* `data/owner-static/engine-facts.toml`: `[ordinance_feed]` and twelve
  `[[ordinance_rocket_entry]]` rows (claim `OWNER-STATIC-2026-10-24`,
  provenance `static`, each with its virtual address), parsed by
  `engine_facts.rs` (`EngineFacts::ordinance`).

## Readings (all `static`)

* **Rocket table `0x619efc`** (12 records of two dwords): `(first, aux)` =
  (1,4) (1,4) (2,2) (8,2) (7,2) (12,2) (7,2) (7,4) (17,4) (17,1) (20,0) (1,0).
  A record is a row when any of the cells `0x647b5c`, `0x647b6c`, `0x647b68`
  is non-zero, or `first <= [0x64b678] + 1` (signed). `aux` is read by none of
  the three functions. `0x40ff00` counts rows, `0x40ff50(n)` answers the
  record of row `n` (the table length past the end), `0x40ffa0(id)` answers
  `rows among 0..=id` minus one, `-1` for a negative id.
* **`E 2031`** (`0x409aec`): held row is `0x40ffa0(record rocket)`; the box
  exists when slot `< [+0x34]` (slots 0 to 3) or slot `< [+0x38] + 4`
  (slots 4 and up), else the call answers `-1`. Both fresh-campaign roster
  records (`0x61a81c`, `0x61a8e8`) hold 2 and 2, which is why boxes 0, 1, 4,
  5 are live and the default rockets there are 1, 1, 1, 1.
* **`E 2037`** stores `0x40ff50(row)` (a table record) at `draft + 0xa8 +
  4*slot`, **`E 2036`** stores the row number at `draft + 0x98 + 4*slot`
  (`0x648164` / `0x648174` are those offsets into the working array at
  `0x6480cc`, stride `0xcc`). `E 2035` copies the live record (`0x64b78c`
  indexed by the slot's default cell, or `0x64abf4`) into the working copy,
  `E 2034` copies it back (both `0x33` dwords).
* The image-gated test re-reads the 12 records and both box counts from
  `CS_ENGINE_IMAGE` and compares them to the committed rows.

## What stays unknown

* **Which rockets pass in a given campaign state.** The three flag cells and
  the progress cell (`0x64b678`) are in the zero-initialised tail of `.data`;
  the image holds no starting value and the code that writes them is unread.
  The feed takes a `ProgressGate`; a real Instant Action screen needs the
  gate the original would have there. Affected: the rocket drop-downs of every
  real loadout screen (#2066, #2070). The gate's own meaning (a mission
  number? an unlock count?) is unknown, not guessed.
* **Box counts of other planes.** `+0x34` / `+0x38` are known only for the
  Devastator roster records; how `E 2035` / the plane choice sets them for
  other airframes is unread.
* The plane-info line (`E 2011`) and descriptions stay `None` / unread, as in
  #2062.

## How the Instant Action draft maps onto the plane record

`ScenarioActorSpec::slot_ordnance` (`SlotOrdnance`, cs_content
`instant_action.rs`) already carries exactly the two arrays the stores write:
`gun_ammo` = record `+0x98`, `rockets` = record `+0xa8` (the rocket **record
index**, not the row). So `OrdinanceDraft::working().ammo` / `.rockets` map
one-to-one onto it. What does **not** map yet: the gun ids (`+0x88`) come from
the airframe (`original_airframe_loadout`), the box counts (`+0x34`/`+0x38`)
have no counterpart in the draft, and nothing builds a `PlaneRecord` from a
`ScenarioActorSpec` or writes a committed record back into one. That
conversion is the presenter's job (#2066) and is not done here.
