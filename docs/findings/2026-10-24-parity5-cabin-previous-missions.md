# The cabin's Previous Missions screen (`ScrapBook_TOC`)

2026-10-24, task #1956, follow-up of #1841. Implemented by `sonnet-1`. Static
readings of the installation's own data and of the owner-supplied engine
image (`CS_ENGINE_IMAGE`): nothing here is `verified_original`, no original
run was made, and no disassembly or screenshot of original content is
committed. Numbers and string ids are facts; handler addresses are the
citations.

## What shipped

The cabin's `PC_B_PREVIOUS` button (layout script `ScrapBook_TOC`) opens the
real screen instead of the marked "not implemented" notice
(`crates/cs_app/src/ui/front_end/previous_missions.rs`, drawn by
`previous_missions_view.rs`, wired in `window.rs`):

* `start_flow::presentation(Screen::Scrapbook)` is `Shell`; the `Cabin →
  Scrapbook` and `Scrapbook → Cabin` (Back) rows of the F45 table carry the
  moves, `SBTOC_B_RETURN`'s `ScriptToExe PassengerCabin` is the way back.
* `NOT_IMPLEMENTED` lost its Previous Missions entry; two remain (Change
  Memento, Plane Construction).
* `FlightCheckStrings::resolve_id_tagged` answers a string by number, which
  the list's native call addresses by.

## Readings the page is built on

Layout (`[@ScrapBook_TOC@]`): the list `SBTOC_L_TOCList` at 420,140, 325
wide, rows 80 high, 4 shown; the `T` rows `SBTOC_T_CHARACTER` (no resource
id) and `SBTOC_T_MISSIONS` (`IDS_SBTOC_PREVIOUS`); buttons `SBTOC_B_VIEW`,
`SBTOC_B_REPLAY`, `SBTOC_B_RETURN`, `SBTOC_B_CURRENT`. The icon strip
`fc_planeicons.png` is 80x960: 12 frames of 80x80 stacked vertically.

`SCRAPBOOK_TOC.SCRIPT` class `NTA` (the list): per row one
`callback($$E$$, 2409, row, icon, line1, line2, line3)`; three black text
lines 10, 30, 50 px down the row at icon width + 20; selected row
`0x80f2e7b7` fill + `0xffdd9017` frame; hovered row `0xffdd9017` frame over a
`0x40f2e7b7` fill; a press selects (`UT` = row index).

Engine image, dispatcher `0x4093a0` (`E` group, ids 2100..):

| call | handler | reading |
| --- | --- | --- |
| `E 2409` | `0x40a4b8` | row `-1`: count = (`[0x647b80]` ? 24 : completed `[0x64b678]`) + 1. Row 0: icon 11, lines `string 1217`, `string 1218`, third line formatted from `string 511`. Row n>=1 (record stride 168 at `0x64cc28`): icon = record dword, line 1 `string 3479 + n`, line 2 `string 1219 + A7(n)`, line 3 the record's stored text; when n > completed the icon is `rand() % 11` |
| `E 2405` | `0x40a633` | arg 0: the selected mission `[0x64b340]`; arg 1 stores the given one |
| `E 2411` | `0x40a408` | replayable iff either of two words of mission n's record (`0x64cbac`, `0x64cc00`, stride 168) is non-zero; mission 0 never |
| `A 7` | `0x419ce0` | maps a mission-table lookup (`0x463bb0`) to 0..5 through a five-entry jump table |

Buttons: Replay runs `E 2104(UT)` then, unless the mission is the one just
played, `A 31` + `A 6` (the briefing); View Selected and the Current Mission
tab run `E 2405(1, …)` and open `SCRAPBOOK.SCRIPT`.

## What stays unknown (and what the page does about it)

* **The per-pilot mission records** (icon, the two replayable words, stored
  text) live in an engine table this project does not write; the start flow
  keeps no per-pilot progress. The page takes a declared `MissionProgress`;
  the start flow passes `MissionProgress::fresh()`, so a pilot sees the header
  row only and Replay stays off. Affected: list rows past the header, Replay.
  Resolving task: per-pilot progress persistence (not yet filed).
* **Line 2** (`A 7`'s lookup table `0x463bb0` and the domain 0..5), the
  header's third line (the format at `0x61f300` and `string 511`) and
  `SBTOC_T_CHARACTER` (`callback($$NB$$, 90)` + `E 2101`, undecoded): drawn
  as nothing.
* **What mail `10018` / `10000` does to the Replay button** is unread: the
  page ignores a Replay press while `E 2411` is false and does not draw the
  button disabled.
* **Where the original puts the buttons' captions** and the faces
  (`TREB9B`, `AB14I`): unread; captions are centred in the engine's default
  face.
* **The `ispy` debug code** (`gui_char`) is not modelled.
* **The Scrapbook journal** (`SCRAPBOOK.SCRIPT`) that View Selected and the
  Current Mission tab open is not built: those buttons open the marked
  notice (`StartFlow::show_not_implemented`). A Replay of a replayable mission
  is refused by name on stderr: it needs the progress store above and the
  briefing hand-over for a chosen mission.

## Verification

`cargo test --locked -p cs_app` (all five suites) passes. Task prefix
`accept_parity5_cabin_previous_missions_`: five synthetic tests (CI) and one
retail test (`CS_GAME_DIR`, run locally). The windowed flow smoke
(`accept_parity5_cabin_intro_06_…`, `CS_CAPABILITIES=retail,gpu,audio`) walks
main menu → Campaign → cabin → Previous Missions → Return to Cabin → main
menu and captured `flow-previous-missions.png`: the retail background, the
three captions, "Previous Missions" and the header row (two lines and the
card-stack icon).
