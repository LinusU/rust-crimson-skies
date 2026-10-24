# The Plane Construction hub's statistics, bar graphs and blueprint

2026-10-24, task #2051 (part of #2036). Implemented by `sonnet-1`. Static readings
of the owner-supplied engine image (`CS_ENGINE_IMAGE`) and of the installation's
own scripts and strings: nothing here is `verified_original`, the original
executable was not run, no disassembly or original screenshot is committed. No
independent review yet. A scripted smoke is never `human_play`.

## What shipped

* `crates/cs_app/src/ui/front_end/construction_stats.rs`: the level function,
  the tables it reads (numbers with their addresses) and the names of the rows
  and strings. `HubFeed::plane` takes a `PlaneRecord`; `HubPage` draws the four
  texts (`PX_T_TOPSPEED`, `_ARMOR`, `_AGILITY`, `_OFFENSE`), and the window puts
  the lit bars and the airframe's blueprint into the hub section.
* `ShellSection::held_panes` keeps the bar panes the shell used to drop, so the
  hub can put the lit ones back (`prepare_section`).
* The flow smoke feeds the hub a harness plane (`SMOKE_PLANE`, not an original
  plane) so the picture shows the statistics.

## Readings (engine image, all `static`)

Handler `0x409685` (`E 2015(25, mode, a, b, c, d, flag)`), level function
`0x40faf0(record, statistic 1..4, mode)`, gun sum `0x4055c0`; slot 25 is the
working plane record at `0x64cb78` (the table of 204-byte records starts at
`0x64b78c`). Mode `0x40` stores the four levels, mode `0x20` the four texts.

| statistic | case | level (0 to 4, 5 or more stored as 4) |
| --- | --- | --- |
| 1 speed | `0x40fb1c` | `trunc(SPEED[af] * ENGINE[engine] - 1.0) / 85`; engine 6 gives 0. `SPEED` is the first word of the 12-byte rows at `0x619da0`, `ENGINE` the six `f64` at `0x619e38` (0.9, 1.0, 1.1, 1.197, 1.33, 1.463). |
| 2 armor | `0x40fb95` | `(ARMOR[af] + rec[0x74] + rec[0x78] + rec[0x7c] + rec[0x80] - 1) / 73`; `ARMOR` is the word at `0x619bc0` of the 44-byte airframe rows from `0x619bb8`. |
| 3 agility | `0x40fc08` | `(AGILITY[af] - 1) / 4` toward zero; `AGILITY` is the word at `0x619bbc`. |
| 4 offense | `0x40fc66` | `((rec[0x34] + rec[0x38]) * 480 + guns) / 2060`, `guns` from `0x4055c0`: for each of the four slots (`rec + 0x88 + 4 * slot`) not holding kind 5, the word at `+8` or (when bit `slot` of the airframe row's word at `0x619bc8` is set) `+0xc` of the 28-byte gun row at `0x619e68`, times 2 when bit `slot` of the byte `rec + 0x84` is set. |

The divisors come from the compiler's magic constants (`0x60606061` is 85,
`0xe070381d` with its add is 73, `0x7f411e53` is 2060; checked by recomputing the
constants). The text of statistic `k` is string 1018, 1019, 1020 or 1132, a
space (`0x61f428`) and string `501 + level` (*Poor* to *Excellent*), after the
face string 93 (94 when the call's last argument is non-zero; the script passes 0).

The script (`PLANECONSTRUCTION.SCRIPT`, `case 12000`/`12002`) stores the texts in
the order speed, armor, agility, offense (`WNA, YNA, XNA, ZNA`) and lights bar
`r` of a group when `r <= level`: speed `PX_P_OS`, armor `PX_P_SI`, agility
`PX_P_AD`, offense `PX_P_OC` (the script swaps the second and third group the
same way as the texts, so each group sits under its own label). A level 0 still
lights one bar.

`E 2243` (`0x40d54d`, args slot, art out, flag): with the flag clear the art is
`Assets\Graphics\PX_%1d_Blueprint.tga` over the airframe `[0x64cba4]`
(eleven in the installation); with it set `PX_Icon_%1d_%2d_%3d.tga` over the
airframe, `[0x64cbb8]` (13 read as 11) and the slot. The script's `12003` shows
the blueprint on `PX_P_PLANE0` at `PLANEX, PLANEY` (16, 44) and hides planes 1
to 3 on the first pass.

## What stays unknown

* **What the record's fields are in the game's terms** (armor words' unit, the
  engine factor's meaning, the two hardpoint words). Affected: every level.
  The feed that maps a draft to a record: #2052.
* **The icons**: `PX_P_SMALLICONS` is shown only when the script's `CNA` is
  non-zero, which nothing in the script sets; the icon pictures (`E 2243` with
  the flag set, `E 2230`'s tint, `E 2212` the record copy) are not drawn.
* **The text face**: the stand-in face is used; the colour and box are the
  layout's.
* A real windowed run still feeds no record (the start flow keeps no per-pilot
  plane), so outside the smoke the hub shows no statistics until #2052.
* **The owner-command smoke does not pass on main either**: it ends on
  `ScrapBook_TOC` instead of `MainMenu` and expects two notices for panels that
  are now built (the same two failures on `origin/main` at 15b39eb23). Filed.

## Verification

`cargo test --locked -p cs_app -- accept_parity5_cabin_plane_construction_stats_
--include-ignored` (three synthetic tests and one retail test). Smoke command
as in the hub finding with `--capture-dir private/evidence/2051`: the hub
screenshot `flow-plane-construction.png` shows the blueprint of airframe 1, the
four lines (*Good*, *Average*, *Average*, *Fair*) and 4, 3, 3 and 2 bars lit
for the harness plane. The smoke's own verdict is `failed` for the two
pre-existing reasons above.
