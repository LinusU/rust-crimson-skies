# The Instant Action OrdinanceLayout screen

2026-10-24, task #2062 (follow-up to #2045). Implemented by `sonnet-2`. Static
readings of the installation's own data and of the owner-supplied engine image
(`CS_ENGINE_IMAGE`): nothing here is `verified_original`, the original
executable was not run, no disassembly or original screenshot is committed. No
independent review yet.

## What shipped

* `crates/cs_app/src/ui/front_end/ordinance_layout.rs`: `OrdinanceLayoutScreen`,
  read from `[@OrdinanceLayout@]` and `ORDINANCELAYOUT.SCRIPT`: seven titles, the
  plane line, four gun-name lines, four ammunition boxes (5 rows, x 136), eight
  rocket boxes (12 rows, x 136 for 0 to 3 and 410 for 4 to 7), the two
  description rects, the accept and cancel buttons. `init` takes an
  `OrdinanceFeed` (what `gui_init`'s natives return); a gun of kind 5 greys its
  name and deactivates its box, a rocket box the feed gives no row is
  deactivated. Presses report `OrdinancePress::{Ammo, Rocket, Accept, Cancel}`,
  the script's `E 2036`, `E 2037`, `E 2034` and the `XQ` rule for cancel.
* `construction_slots.rs`: `text_row`, `rect_row` and `NameFn` are `pub(super)`
  (wiring only).

## Readings (engine image, all `static`)

Dispatcher `0x4093a0`, jump table `0x40f518` (call `n` at index `n - 2000`).

| call | handler | reading |
| --- | --- | --- |
| `E 2027` | `0x409a5c` | ammunition list text: `-1` answers 5 rows, else string `3360 + index` |
| `E 2028` | `0x409b72` | rocket list text: `-1` answers a count from the twelve 8-byte records at `0x619efc` (`0x40ff00`), else string `3395 +` the n-th passing record (`0x40ff50`) |
| `E 2030(zq, slot, text, kind)` | `0x4099f2` | gun name string `3320 + kind`; answers the kind, writes the held ammunition row |
| `E 2031(zq, slot, text)` | `0x409aec` | rocket name string `3395 + value`; `-1` when the plane lacks the box (0 to 3 against `+0x34`, 4 to 7 against `+0x38 + 4` of the plane record at `0x64b78c + 204 * plane`) |
| `E 2036`, `E 2037` | `0x40962c`, `0x409654` | store the ammunition / rocket choice at `0x648164` / `0x648174` indexed by plane and slot |
| `E 2034`, `E 2035` | `0x409578`, `0x4095d3` | copy 0x33-dword plane records (`0x64b78c` or `0x64abf4` to `0x6480cc`/`0x64cb78`): the commit and the load |
| `E 2010`, `E 2011` | `0x4094a7`, `0x409443` | diagram art id (`[0x64b7b8 + ...]`) and the plane-info text |

Strings 3320 to 3325, 3360 to 3364 and 3395 to 3406 are present in the
installation's table (the retail test asserts it).

## What stays unknown

* **The entries**: which twelve rockets pass the `0x619efc` table (it compares
  campaign flags at `0x647b5c`, `0x647b68`, `0x647b6c` and `0x64b678`), the
  ammunition kinds behind the five rows, and which content id each is. The
  screen takes them from the caller. Affected: every real Instant Action
  loadout. Resolving task: filed with this one (the feed and presenter).
* **The draft the stores write** (`E 2034`, `E 2036`, `E 2037`): the press is
  reported, nothing is applied.
* **The description texts** (`E 2032`, `E 2033`), **the plane-info text**
  (`E 2011`), **the diagram choice** (`E 2010`), **what `XQ` means** beyond
  zero or not, and **the drop-down art**.
* **No presenter**: the window does not yet show this screen.

## The window presenter (task #2068, `sonnet-2`)

`ordinance_layout_view.rs` draws the screen in the shell window; `window.rs`
carries it (`with_ordinance_layout`, `headless_app_ordinance_layout`,
`ShellResource::ordinance_layout` / `ordinance_outcomes`): hover rolls a row,
a press opens, chooses or closes a list, Esc closes an open list, and the
accept and cancel buttons record `OrdinancePress::{Accept, Cancel}`. Boxes,
frames and the row highlight are stand-in colours; text is the default face at
the `[FONT]` size. Not drawn, still unknown: the description texts, the plane
diagrams and the drop-down art. The tests feed a fixed **synthetic**
`OrdinanceFeed`; the real feed and the move out of the screen are #2066 and
#2070. No smoke scene was added (the screen has no retail feed to show yet).
