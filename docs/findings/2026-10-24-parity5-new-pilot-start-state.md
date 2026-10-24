# A new pilot's starting cash and plane

2026-10-24, task #2074 (P5-FEED-A, part of #2052). Implemented by `sonnet-2`.
Static readings of the owner-supplied engine image (`CS_ENGINE_IMAGE`):
nothing here is `verified_original`, the original executable was not run, no
disassembly is committed. No independent review yet.

## What shipped

* `data/owner-static/engine-facts.toml`: `[new_pilot_start]`, 46
  `[[new_pilot_plane_word]]` rows and 23 `[[empty_loadout_store]]` rows (claim
  `OWNER-STATIC-2026-10-24`, provenance `static`, each with its virtual
  address).
* `crates/cs_app/src/engine_facts.rs`: `EngineFacts::new_pilot() ->
  NewPilotStartFacts` (`starting_cash(allow_all)`, `template_words`,
  `template_word(offset)`, `empty_loadout_stores`, `apply_empty_loadout`).
* `crates/cs_app/tests/ui/parity5_new_pilot_start_state.rs` (prefix
  `accept_parity5_feed_start_state_`); the image test re-reads every cited
  byte from `CS_ENGINE_IMAGE`.

## Readings (all `static`)

### The reset routine

`0x4113b0` is what `A 9` (`0x407a7e`, the campaign page's new-pilot cue) and
the startup parameter routine (`0x411361`) call. It zeroes the 26 plane
records `0x64b78c..0x64cc44`, sets the selection `[0x64b67c] = 0`, patches
the thirteen templates at `0x619f58` (paint words from `0x61daf4` /
`0x61db00`, gun nibble from `0x619bc8`; for the pilot's template these write
the values it already holds), copies the template `0x61a81c` over record 0
(`0x411596`) and over the working plane `0x64cb78` (`0x4115ec`), and writes
the cash.

### Cash `[0x64b788]`

* Whole dollars: string 1036 is *PLANE COST: $%1!d!* over the cost, and
  `E 2213` compares `cost <= [0x64b788]`.
* **New pilot: 0.** `0x41146b` is `mov [0x64b788], ebx` with `ebx = 0`.
* The only other write on this path, `0x41160b`, stores **250000** (`0x3d090`)
  and also fills records 2..12 with the eleven airframe templates; it runs only
  when `[0x647b5c] != 0` (`cmp` at `0x4115d7`).
* `[0x647b5c]` is the profile variable **`fAllowAll`**: the profile table
  at `0x402000..` binds the names `nMission`, `fLaunchedGame`, **`nPlayerCash`**
  (`[0x64b788]`, `0x402002`), `fReturnToGame`, `fMultiPlayerConstruction`,
  `fIAConstruction`, `fHitPaintTab` (`[0x647b94]`), `fViewAll` (`[0x647b80]`),
  `fAllowAll` (`[0x647b5c]`) and more to their cells by address. This corrects
  the earlier statement (`2026-10-08-m01-lc-campaign-airframe-engine-state.md`)
  that no reference writes `[0x647b5c]`: the profile load writes it, through
  the pointer. Other writes of the cash: `E 2109` (`0x40a039`, sale), the
  mission payout (`0x405db9`, `+= reward`), `E 2263` (`0x40b56c`, purchase),
  and the profile load (`nPlayerCash` is persisted per profile).

### The starting plane

Record 0 and the working plane begin as the 204-byte template at `0x61a81c`
(the wingman's `0x61a8e8` is the same words, name aside): occupied (`+0`
= 1), airframe `+0x2c` = 5, `+0x30` = 1, boxes `+0x34` = `+0x38` = 2, `+0x3c`
= -1, paint `+0x40` = 4, gun bits `+0x84` = 7 (guns 0..2), guns `+0x88` =
[2, 1, 0, 5], ammunition `+0x98` = [0, 0, 0, 4], rockets `+0xa8` = [1, 1, 11,
11, 1, 1, 11, 11]. All 46 measured dwords are in the table.

### `[0x647b94]` and the default-loadout box

`[0x647b94]` is the profile variable `fHitPaintTab`. `E 2212` (`0x40aacc`)
saves the working plane's 13 paint dwords (`+0x40..`) when it is non-zero and
puts them back after the template copy; when it is zero the paint colours
come from the airframe's paint table (`0x61daf4` / `0x61db00`). A profile that
never set it holds 0 (`.bss`).

When the flag argument of `E 2212` is 0 (the Plane Name box unchecked) the
copied template is emptied by 23 stores (`0x40ab9b..0x40ac1b`): gun nibble
cleared, guns `+0x88..` = 5, ammunition `+0x98..` = [5, 4, 4, 4] (the store
order leaves `+0x98` at 5, not 4), rockets 11, `+0x30` = 6, boxes `+0x34` =
`+0x38` = 0, `+0x74..+0x80` = 0. Then the working plane is copied to the saved
copy `0x6480cc` (`0x40ac2c`). With the box checked the template is kept.

### The saved copy

`0x6480cc` (so `0x6480fc` is its `+0x30`) is written by `E 2212` only
(`0x40ac2c`) and by `E 2034`; the reset does not touch it, so a fresh run
holds zeros there until the first `E 2212`.

## What stays unknown

* **`fAllowAll`, `fHitPaintTab` in a real profile.** The profile loader's
  default for a missing name (`0x406ff0`, `0x5d39c0`) and the stored values are
  unread. Affected: whether a retail pilot opens with 0 or 250000 dollars, and
  whether paint survives an airframe change. The table gives both branches;
  `starting_cash(allow_all)` takes the flag from the caller. Resolving task:
  #2052 / #2075 (choose from the profile reading or an owner capture).
* **`+0x4` (name), `+0x68..+0x70`, `+0xc8`.** The name comes from string
  511 / 512; `+0x68..+0x70` are written by `0x406840` through the resolver
  `0x4067c0` (unread); `+0xc8` is a pointer. They are `None` in the accessor.
* **What the saved-copy compare (`E 2221`) answers on a new pilot** when the
  saved copy is still zero, and when the script calls `E 2212` first, are
  unread here.
