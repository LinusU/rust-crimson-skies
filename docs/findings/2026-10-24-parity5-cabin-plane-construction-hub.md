# The cabin's Plane Construction hub (`PlaneConstruction`)

2026-10-24, task #2042 (P5-CONS-A, part of #2036). Implemented by `sonnet-1`.
Static readings of the installation's own data and of the owner-supplied engine
image (`CS_ENGINE_IMAGE`): nothing here is `verified_original`, the original
executable was not run, no disassembly or original screenshot is committed. No
independent review yet. A scripted smoke is never `human_play`.

## What shipped

The Plane Name dialog's OK with a name opens the hub instead of the marked
notice (`crates/cs_app/src/ui/front_end/construction_hub.rs`, drawn by
`construction_hub_view.rs`, wired in `window.rs` and `start_flow.rs`):

* `StartFlow::open_construction_hub` keeps the machine on `Screen::Construction`
  and makes `section()` answer `PlaneConstruction`; any move off the screen
  closes the hub. Cancel is the table's `Construction --Cancel--> Cabin` row.
* The page reads the section's rows: the `PLANE NAME:` caption and its edit box
  (placed as the script does, `SNA.WB = 302 - R`), the `T` captions, the six tab
  buttons' captions and four colours each, the cash text.
* The six tabs behave as the script's `mail(10000/10018, ..)`: one is held down
  (`MenuShell::set_latched`), the airframe tab on open. Their panels are other
  tasks, so a press puts up the marked notice (#2043 airframe and engine, #2044
  armor and paint, #2045 guns and hardpoints), and the hub's panel area carries a
  marked line. Opening the hub records the Airframe panel as seen without a
  notice over the screen.
* **Ready** (`ScriptToExe = Purchase`) and **Sell** (`hangar.script`) put up
  marked notices; Sell is inactive until the feed reports three saved planes.
* The texts follow a `HubFeed` (cash, plane cost, weight capacity, current weight,
  airframe name, unsaved flag, saved-plane count). `HubFeed::from_view` builds it
  from the production `ConstructionView`: cost meter's use, mass meter's limit and
  use, the view's dirty flag. A figure nobody supplied is drawn as nothing.
* Cancel with unsaved changes asks (string 1228 over the airframe's short name);
  Enter or Y leaves, Esc or N keeps building.
* **A defect the smoke found.** The tabs' and Ready's `ScriptToExe` name sections
  the layout nests (`[@AirFrame@]`, `[@Engine@]`, `[@Armor@]`, `[@Guns@]`,
  `[@HardPoints@]`, `[@Paint@]`, `[@Purchase@]`, indented lines 703 to 838 of
  `LAYOUT.CSV`), so the shell reported their press as a move there and the flow
  refused it ("the Guns screen is not part of the front-end state table"). The
  window now treats such a move from the hub as the hub's own press and steps the
  shell back. The synthetic layout carries those sections so a test would catch it.

## Readings (engine image, all `static`)

Dispatcher `0x4093a0`; numbers `0x834..0x96c` index the byte table `0x40f788` and
the jump table `0x40f5b4`; `0x7d0..0x7f6` index `0x40f518`.

| call | handler | reading |
| --- | --- | --- |
| `E 2213(name, text)` | `0x40add0` | formats string `0x40c` (1036, *PLANE COST: $%1!d!*) over the cost `[0x64cba0]` into the second argument; answers `cost <= [0x64b788]` (`setle` at `0x40ae23`): the cash. The script turns the line red on a zero answer. |
| `E 2214(a, b, c)` | `0x40ae33` | string `3000 + [0x64cba4]` through `0x405` (1029) into `a`; the airframe's capacity `[0x619bb8 + 44 * airframe]` through `0x406` (1030) into `b`; the weight through `0x407` (1031), or `0x408` (1032, *Pending*) when `[0x647ba4]` is set, into `c`. Answers 0 only when the weight `[0x64cbb4]` is above the capacity (`jle` at `0x40af17`) and `[0x647ba4]` is clear; the script turns the weight line red on 0. |
| `E 2215(1, name)` | `0x40ad9b` | (from #1958) copies the string to `0x64cb7c`: the plane's name. |
| `E 2221` | `0x40b6b3` | compares the working plane record `0x64cb78..` with the saved copy `0x6480fc..` (`0x64cbfc` vs `0x648150`, `0x64cba8` vs `0x6480fc`, … `0x64cbb8` vs `0x64810c`, then three arrays of three words at `0x64cbbc`, `0x64cbc8` and `0x64cbd4` against `0x648110`, `0x64811c` and `0x648128`) and answers non-zero on any difference (the mismatch jumps to `0x40a441`). The cancel button leaves at once on zero. |
| `E 2265(OR.TH)` | `0x40b3d4` | formats string `0x4cc` (1228, *Are you sure you want to cancel building this %1!s!?*) over the string `3020 + [0x64cba4]` (the airframe's short name) into the message box's text. `OR.UR = 0x4`; an answer `OR.VR == 3` leaves. |
| `E 2018(&n)` | `0x4097ea` | answers `0x40fd10()` minus 1 when `[0x64b340]` is 13 or 17; the script deactivates Sell when the answer is below 3. What the count is (the pilot's saved planes) is inferred. |
| `E 2015(25, 0x20/0x40, …)` | `0x409685` | fills the four statistics texts (0x20) and the bars' levels (0x40) from tables indexed by the airframe record; the tables are unread. |
| `E 2212`, `E 2230`, `E 2243`, `E 2010`, `E 2507` | `0x40aacc`, `0x40d473`, `0x40d54d`, `0x4094a7`, the `A` class | copy a plane record (`2212`), fetch a slot's colour (`2230`) and picture (`2243`), … : unread. |

Strings (installation's `langui.dll`): 203, 1027 to 1049 (the hub's captions:
1029 airframe, 1030 weight capacity, 1031 current weight, 1032 pending, 1035
*OFFENSE: %1!s!*, 1036 plane cost, 1037 to 1043 the seven step captions),
1225 to 1228 (*NO PAINT*, *INSUFFICIENT FUNDS*, *OVERWEIGHT*, the cancel prompt),
3000 to 3010 airframe names, 3020 to 3030 short names. The layout header
(`LAYOUT.CSV` line 6) names a button's colour columns `ColorDisabled`,
`ColorActive`, `ColorRollover`, `ColorDepressed`, which the tab captions use.

## What stays unknown

* **The statistics and the bar graphs.** `PX_T_TOPSPEED`, `_AGILITY`, `_ARMOR`,
  `_OFFENSE` and the 20 `PX_P_OS/AD/SI/OC` bar panes depend on `E 2015` tables.
  Nothing is drawn for the texts and the panes are not presented. Affected: the
  hub's statistics block. Resolving task: #2051.
* **The plane picture** (`PX_P_PLANE0..3`, `PX_P_SMALLICONS`, `E 2243`): the
  central blueprint is not drawn. Affected: the hub's centre. Resolving task: #2051.
* **Where the figures come from in a real run.** The start flow keeps no
  per-pilot plane, cash or `ConstructionScreen`, so the windowed run feeds an
  empty `HubFeed`: the cash, cost, airframe and weight lines are not drawn. The
  mapping from the draft's units to the engine's (*lbs.*, dollars) is unmeasured.
  Affected: every figure on the hub. Resolving task: #2052.
* **Ready's purchase panel** (`[@Purchase@]`, *7) CONSTRUCTION TOTALS*) is not
  one of #2043 to #2046. Resolving task: #2053.
* **Message box 4's look** (the `MessageBox` section's presenter is unbuilt) and
  **the cash row's face** (`callback($$NB$$, 91, …)`): stand-ins.
* **`callback($$A$$, 2507, 0/9)`**, **`callback($$NB$$, 4005, …)`**,
  **`mail(10021, SNA)`** and **`E 2014`**: unread. **The *gimme* cheat** is not
  modelled.
* **The edit box's width** uses a stand-in caption width (0.6 of the size per
  glyph), since the face is not in the installation.

## Verification

`cargo test --locked -p cs_app -- accept_parity5_cabin_plane_construction_`
(`--include-ignored` with `CS_GAME_DIR`; hub tests are
`accept_parity5_cabin_plane_construction_hub_*`: ten synthetic and one retail).
The owner-command front-end smoke
(`CS_FRONT_END_SMOKE_SCENE=flow CS_CAPABILITIES=retail,gpu,audio target/release/cs
--cs-path "$CS_GAME_DIR" --front-end --smoke-seconds 21 --capture-dir
private/evidence/2042`) exited 0 with `passed: true`; its visited sections end
`PlaneName → PlaneConstruction → PassengerCabin → MainMenu`, and it captured
`flow-plane-construction.png` (the hub open on the Airframe tab) and
`flow-plane-construction-guns.png` (the Guns tab held down under the marked
notice) among sixteen framebuffers, `final` included (git-ignored).
