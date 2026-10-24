# The Plane Construction hub's Airframe and Engine panels

2026-10-24, task #2043 (P5-CONS-B, part of #2036). Implemented by `sonnet-2`.
Static readings of the installation's own data and of the owner-supplied engine
image (`CS_ENGINE_IMAGE`): nothing here is `verified_original`, the original
executable was not run, no disassembly or original screenshot is committed. No
independent review yet. A scripted run is never `human_play`.

## What shipped

* `crates/cs_app/src/ui/front_end/construction_panels.rs`: `ChoicePanel`, the
  model of one panel (`ChoiceKind::Airframe`, `ChoiceKind::Engine`), read from
  the `[@AirFrame@]` and `[@Engine@]` sections: the title text (`IDS_AF_TITLE`,
  `IDS_EN_TITLE`), the name line the script fills at run time, the description
  box's rect and the `D` row (422, 116, 330 wide, 15 per row, 11 and 7
  `TotalDisplayed`). `apply_choice` puts a chosen entry into the production
  `ConstructionScreen` (`set_airframe` / `set_engine`), which refuses an id of
  the wrong kind and leaves the draft alone.
* `HubPage` owns the two panels when the layout supplies their rows
  (`panel_built`); a layout without them keeps the marked notice for that tab.
  `press_at` / `hover_at` / `close_list` drive the list; a choice is the
  `HubOutcome::Choose` the window records; `set_choices` / `follow_choice` take
  the entries and the draft's held id.
* The window routes a pointer press to the open panel's drop-down before the
  shell, Esc shuts an open list, a built tab no longer puts up the notice, and
  the hub opening on the Airframe tab no longer counts it missing.
  `ShellResource::set_hub_choices` hands a run's entries in.
  `construction_hub_view.rs` draws the title, the chosen name, the box and the
  open list (stand-in ground, frame and highlight; the control's art is not
  drawn).
* While the airframe list is open with the pointer on an entry that is not the
  choice, the hub's weight line shows string 1032 (*Pending*).

## Readings (engine image, all `static`)

Dispatcher `0x4093a0`; the byte table `0x40f788` and jump table `0x40f5b4`
give each number's handler (read with the image's PE headers).

| call | handler | reading |
| --- | --- | --- |
| `E 2216(index, commit)` | `0x40d28d` | `[0x64cba4] = f(index)` (`call 0x410120`), and the *Pending* flag `[0x647ba4] = (commit == 0)` (`sete`/`mov` at `0x40d29e..0x40d2a9`). The script's `10015` (chosen) sends `commit = 1`, its `10013` (rolled over) `0`, so the hub's *Pending* weight is the roll-over preview. |
| `E 2217` | `0x40d272` | answers `0x410170([0x64cba4])`: the airframe's list index. |
| `E 2218` | `0x40bce1` | with a null target answers 4, or 7 when `[0x64b678] > 7` or any of `[0x647b5c]`, `[0x647b68]`, `[0x647b6c]` is non-zero; the engine script names it as its drop-down's callback (`HP.TJ = 2218`). Read as the engine list's length: an inference. |
| `E 2219(get/set, ..)` | `0x40bd97` | the engine slot `[0x64cba8]` (the field `E 2221` compares with the saved plane). Index limit 3, or 6 on the same condition as `E 2218`; an index equal to the limit is stored as 6 (`0x40be05`). |
| `E 2221`, `E 2265` | as the hub finding | the airframe script asks `E 2221` before it switches airframes. |

Script flow read from `ASSETS/SCRIPTS/AIRFRAME.SCRIPT` (1813 bytes) and
`ENGINE.SCRIPT` (911 bytes) of `GOSDATA/ASSETS/crimson.rof` (read through
`cs-inspect rof --member`, not committed): both start with
`callback($$A$$, 2507, 2|3)`, mail the hub `10017` on creation and `12001`
after a change; the airframe also mails `12002`/`12003` on a roll-over refresh.
Message box 8 (`E 2222`) asks whether to switch airframes when the plane
differs from the saved one: `@globals@OR.VR` 3 or 4 switches, 2 keeps the old
airframe.

## What stays unknown

* **The descriptions** (`E 2223` airframe, `E 2224` engine) and the **engine
  names** (`E 2220(2, ..)`): their string source is unread, so the scrolling
  box is not drawn and the engine entries' names come from the caller.
  Affected: the description box of both panels. Resolving task: #2051 (with the
  statistics tables).
* **The campaign gate on the lists** (the four globals above) and **which
  content id is airframe or engine `n`**: the page takes entries from the
  caller. Affected: which entries a real run offers. Resolving task: #2052.
* **Message box 8** (the switch-airframe prompt, `E 2222`): not built, so a
  choice applies at once. Affected: choosing a second airframe over a changed
  plane. Resolving task: none yet.
* **The five-step statistics bars** (`PX_P_OS/AD/SI/OC` via `E 2015`) the choice
  refreshes: tables unread. Resolving task: #2051.
* **The drop-down's keys and art** (scroll bar, arrows, highlight) and
  **`callback($$A$$, 2507, 2|3)`**: unread. The list opens by pointer only.
* A real windowed run feeds no entries (the start flow keeps no per-pilot
  plane): the panels show their title and an empty box until #2052.

## Verification

`cargo test --locked -p cs_app -- accept_parity5_cabin_plane_construction_airframe_engine_
--include-ignored`: six synthetic tests (rows, unbuilt tabs, list/roll-over/
choice, the *Pending* weight, the choice into the production draft, the
window's pointer route) and one retail test over the installation's rows and
strings.
