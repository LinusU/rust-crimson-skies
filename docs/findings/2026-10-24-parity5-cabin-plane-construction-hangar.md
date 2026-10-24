# The Plane Construction hub's Sell hangar, picture slots and Default loadout box

2026-10-24, task #2046 (P5-CONS-E, part of #2036). Implemented by `sonnet-2`.
Static readings of the installation's own data and of the owner-supplied engine
image (`CS_ENGINE_IMAGE`): nothing here is `verified_original`, the original
executable was not run, no disassembly or original screenshot is committed. No
independent review yet. A scripted run is never `human_play`.

## What shipped

* `crates/cs_app/src/ui/front_end/construction_hangar.rs`:
  * `HangarPage`, the model of `HANGAR.SCRIPT` over `[@Hangar@]`: the 25-row
    saved-plane list (`HA_D_PILOTPLANE`), the engine's row/record mapping
    (`slot_for_index`, `index_for_slot`), `E 2018`'s count (`saved_count`),
    the plane and value lines (`E 2011`, `E 2135`), Done, Export and Sell with
    its yes/no box and the sale (value into the cash, record emptied, row 0
    selected).
  * `SlotIcons`, the hub's four picture panes (`PX_P_PLANE0..3`) with their
    drawing order (the `Z` column: 5, 2, 3, 4, so slot 1 is the back one) and
    `E 2243`'s file names (`slot_icon_art`, `blueprint_art`).
* `HubPage` owns the `HangarPage` and the icons (`hangar`, `hangar_mut`,
  `slot_icons`); `set_saved_planes` hands the hangar the pilot's planes and takes
  Sell's count from them. The Plane Name dialog's box now reaches the hub
  (`set_default_loadout`, `starting_loadout`, `StartingLoadout`).

## Readings (engine image, all `static`)

Dispatcher `0x4093a0`.

| call | handler | reading |
| --- | --- | --- |
| records | `0x64b78c..0x64cb78` | 25 records of 204 bytes; first word 0 is empty; name at `+4`, airframe at `+0x2c`. The working plane's record is `0x64cb78` (slot 25). |
| `E 2018` | `0x4097ea`, `0x40fd10` | occupied records, minus 1 while `[0x64b340]` is 13 or 17. |
| `E 2013(-1, i)` | `0x4094eb`, `0x4101d0` | `[0x64b67c] = slot_for_index(i)`; the walk skips empty records and, in state 13/17, returns the selected record when it reaches it; off the end it answers 25. |
| `E 2012` | `0x4094ce`, `0x410220` | row of the selected record: occupied records up to it, not counting the selected one in state 13/17, minus 1. |
| `E 2011` | `0x409443` | string 207 over the name (`+4`) and the string `3000 + airframe`. |
| `E 2103` | `0x40a1c1` | first word 2: string 704 over the short name (`3020 + airframe`) and the name, answers 0; else string 700 over the short name and the value (`0x405680`), answers 1. The script opens an OK-only box on 0, a yes/no box (type 4) on 1. |
| `E 2109` | `0x40a039` | `[0x64b788] += round(value)` (the cash), first word of the selected record = 0. |
| `E 2105` | `0x40a0f7` | string 702 over the short name of a plane (Export's box). |
| `E 2135` | `0x40a167` | string 1258 over the value. |
| `E 2243(R, art, 1)` | `0x40d54d` | `Assets\Graphics\PX_Icon_%1!d!_%2!d!_%3!d!.tga` (`0x61f370`) over airframe `[0x64cba4]`, paint `[0x64cbb8]` (13 reads 11) and `R`; last argument 0: `Assets\Graphics\PX_%1!d!_Blueprint.tga` (`0x61f3a0`). |
| `E 2212(flag)` | `0x40aacc` | copies the airframe's template record (`0x619f58 + 204 * airframe`) over the working plane `0x64cb78`, keeping its name; with `flag == 0` it then clears the guns' bits, sets the gun kinds to 5 (the empty position) and zeroes the hardpoint words; finally copies the working plane to the saved copy `0x6480cc`. |

## PN_B_DEFAULT: resolved

`PLANENAME.SCRIPT` never reads the box, but `PLANECONSTRUCTION.SCRIPT` does:
`UMA = mail(10008, @planename@OOA); callback($$E$$, 2212, (UMA))`. Checked (the
box starts checked: `PN_B_DEFAULT` row `1,1`) the new plane keeps its airframe's
template, weapons included; unchecked it starts with empty guns and hardpoints.
`StartingLoadout::{Stock, Empty}` carries that.

## What stays unknown

* **The window presenter of the hangar.** The window still answers Sell with the
  marked not-implemented notice: the `Hangar` section's start-flow state, the
  list and the plane lines are not drawn. Affected: the Sell screen. Resolving
  task: filed with this one (see the hand-over).
* **Export.** `A 22` and what exporting writes are unread; the page answers the
  box text only.
* **The too-few notice** (`NB 701`), **the list rows' text**, **the statistics**
  (`E 2015`, `E 2019`, the plane-weapons text) are unread.
* **What a record's first word means** besides 0 and 2.
* **The row mapping in construction** is transcribed as read; its behaviour when
  the working plane's record is not the highest occupied one is not verified.
* **The template loadouts** (`0x619f58`): which guns and hardpoints each airframe
  starts with is unread, so `StartingLoadout::Stock` has no content yet.
  Affected: every new plane's starting loadout. Resolving task: #2052 (the feed).
* **The picture slots' art** is not drawn; `R` is the layer of the airframe's
  small icon (inferred from the four panes sharing a place); the files are
  named, not verified present.
* **The saved-plane store.** The start flow keeps no per-pilot planes, so the
  hangar's list is empty in a real run until #2052.
* `[0x647b94]` (a flag `E 2212` and `E 2230` test) and `[0x647b6c]`/`[0x647b68]`
  (`E 2105`) are unread.

## Verification

`cargo test --locked -p cs_app -- accept_parity5_cabin_plane_construction_hangar_`
(`--include-ignored` with `CS_GAME_DIR`): ten synthetic tests and one retail test.
No owner-command smoke: nothing the player sees changed (Sell still shows the
marked notice).
