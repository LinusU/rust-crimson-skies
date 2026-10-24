# The Plane Construction hub's Guns and Hardpoints panels

2026-10-24, task #2045 (P5-CONS-D, part of #2036). Implemented by `sonnet-2`.
Static readings of the installation's own data and of the owner-supplied engine
image (`CS_ENGINE_IMAGE`): nothing here is `verified_original`, the original
executable was not run, no disassembly or original screenshot is committed. No
independent review yet. A scripted run is never `human_play`.

## What shipped

* `crates/cs_app/src/ui/front_end/construction_slots.rs`: `SlotPanel`, the model
  of one panel (`SlotKind::Guns`, `SlotKind::HardPoints`), read from the
  `[@Guns@]` and `[@HardPoints@]` sections: the title, four gun boxes (305 wide,
  11 rows displayed) or two point boxes (5 rows), the point titles (`IDS_HP_POINT1_TITLE`,
  `_POINT2_`), the name line and the description box's rect. One list is open at a
  time; the name line of the guns follows the position that last took a press
  (the script's `BS`).
  `apply_slot_choice` puts a choice into the production `ConstructionScreen`
  (`fit_guns` / `fit_ordnance`) and `held_in_positions` reads the draft back;
  the empty entry removes the fitment.
* `HubPage` owns the two panels when the layout supplies their rows
  (`panel_built`); `press_at` / `hover_at` / `close_list` drive the lists, a
  choice is `HubOutcome::Choose { panel, slot, id }`, `set_slot_options` /
  `follow_slots` take the entries and the draft's holdings.
  The window routes the pointer to the open panel before the shell, Esc shuts an
  open list, a built tab no longer puts up the marked notice;
  `ShellResource::set_hub_slots` hands a run's entries in.
  `construction_hub_view.rs` draws the boxes, the chosen entries and the open
  list (stand-in ground, frame, highlight).

## Readings (engine image, all `static`)

Dispatcher `0x4093a0`; the numbers index the byte table `0x40f788` and the jump
table `0x40f5b4`.

| call | handler | reading |
| --- | --- | --- |
| `E 2248` (guns' list text callback) | `0x40be1b` | eleven entries: 0 to 4 string `3310 + kind` (`0xcee`); 5 to 9 the same with string 506 (`0x1fa`) formatted in front (read as the pair form); 10 string `3310 + 5`, the empty position. Matches the box's 11 rows. |
| `E 2249(mode, slot, value)` | `0x40bea2` | get: `kind + 5` when bit `slot` of the byte `[0x64cbfc]` is set, 10 when kind is 5, else `kind`. Set: bit set for 5 to 9, cleared otherwise; kind stored at `[0x64cc00 + 4 * slot]` (`index - 5`, 5 for 10); `[0x64cc10 + 4 * slot]` receives 0 for kind 5, else 4 (unread). |
| `E 2244` (hardpoints' list text) | `0x40b81d` | five entries (matches 5 rows): index 0 string 1165, index 1 string 1168, others string 1169 formatted over the index. |
| `E 2245(mode, point, value)` | `0x40ad0f` | get/set of `[0x64cbac]` (point 0) or `[0x64cbb0]` (point 1); a set also rewrites four words at `[0x64cc20 + 16 * point]` from a random number (`0x5d3240`, mod 20 > 10) or 0xb: unread. |
| `E 2220(4, text, slot)` | `0x40bbe6`/`0x40bc57` | the gun-name line: string `3310 + kind` of the slot. Mode 5 (the hardpoint name) is not decoded. |
| `E 2226`, `E 2227` | `0x40b993`, `0x40bac6` | the descriptions: formatted from tables at `0x619bc8..`/`0x619e68..` and strings 0x483..0x485 (1155 to 1157): unread. |

Strings 3310 to 3315, 1165, 1168, 1169 and 506 are present in the installation's
table (the retail test asserts it).

## What stays unknown

* **The hub has no rocket panel.** The eight rocket drop-downs (`object RKA[8]`)
  and four ammunition drop-downs belong to `ORDINANCELAYOUT.SCRIPT`, the Instant
  Action loadout screen (section `OrdinanceLayout`, `@globals@ZQ` pilot or
  wingmate), not to the hub. It is not built; resolving task: #2062
  (filed).
* **Which draft mount is position `n`** and what a *pair* means for a
  `GunFitment`'s positions: the caller passes the mounts and the positions
  (`apply_slot_choice`). Affected: every gun/hardpoint choice in a real run.
  Resolving task: #2052 (the feed).
* **Which content id is entry `n`.** The entries come from the feed.
* **The gun position titles** (`E 2250`), **the hardpoint name line**, **the
  descriptions**: sources unread, nothing drawn.
* **The lists' look.** The drop-down control's art (`ZTA`, scroll-bar, arrows)
  is a stand-in box and list. The windowed run feeds no entries yet, so the
  boxes are drawn empty until #2052.
* **The statistics bars and costs** the `mail(12001)` refresh: #2051.

## Verification

`cargo test --locked -p cs_app -- accept_parity5_cabin_plane_construction_guns_hardpoints_`
(`--include-ignored` with `CS_GAME_DIR`): five synthetic tests (rows, missing
rows, list behaviour, draft edits, window routing) and one retail test.
