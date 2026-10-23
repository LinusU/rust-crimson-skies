# FE-MENU-3: the Campaign pilot-selection page

Date: 2026-10-23. Task: #1835 / FE-MENU-3 "Front-end presenter 3/4: Campaign
pilot-selection screen (edit box, pilot list, Continue/Delete/Cancel)"
(`specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`). Shared
contract: `docs/contracts/CLI-EVIDENCE.md`. Required capabilities: `retail` and
`gpu` (`CS_CAPABILITIES=retail,gpu,audio`). Test prefix: `accept_fe_menu_3_`.
Evidence: `docs/findings/evidence/FE-MENU-3.json`, artifacts in
`private/evidence/FE-MENU-3/` (git-ignored; screenshots never enter Git).

Implemented by `claude-1`. This is the implementer's record, not a review; it
awards no `verified_original`/`release_approved`. `retail` means read access to
the installation: no agent ran `crimson.exe`, and no capture here is the
original renderer's.

## What the original data says (static readings)

`ASSETS/LAYOUT.CSV` `[@Campaign@]`:

- pane `CM_BACKGROUND` (`CM_BackGround.png` at 193,251); buttons `CM_B_START`
  (`GN_B_Continue.png`, 474,293), `CM_B_DELETEPLAYER` (193,547), `CM_B_CANCEL`
  (444,547) — all drawn by the existing shell.
- `CM_E_NAME=E,FONT_CM_EDIT,243,297,0,221,22,16,0xFFFF0000,,0xFFEF0010`. The
  header writes `FontID,X,Y,Z,Width,Height,MaxChars,HelpID,TabOrder,TextColor,
  FrameColor,CursorColor`; the data row omits `HelpID`/`TabOrder` (the same
  header staleness the `M` and `B` headers have), so the columns read are: font,
  X, Y, Z, 221x22, 16 characters, text `0xFFFF0000` (red), frame empty (the
  header: "colors=0 means use default" — nothing drawn), caret `0xFFEF0010`.
- `CM_L_PLAYERS=L,CM_B_ScrollBar.png,CM_B_ScrollUp.png,CM_B_ScrollDown.png,245,
  356,0,305,20,7`. The header writes a `ScriptPointer` column the row omits; the
  columns read are slider, up, down, X, Y, Z, width 305, row height 20, 7 rows.
- `CAMPAIGN.SCRIPT` (read through `FrontEndScreens::script_text`): Enter and
  Continue both run message 11004 (empty name → message box 200; look the name
  up; create or select it; start the passenger cabin); a list-row press copies
  the row's name into the edit box; the list draws black ground, the selected
  row filled `0xFF800000`, the hovered row framed `0xFFFF0000`, white text; the
  pilot limit is `24 > IB`; Delete asks message box 201 first.

## What this stage built

- `pilot_page.rs`: `PilotPage` reads the two rows (never a guessed geometry —
  a missing row or unreadable column is a named `PilotPageError`) and owns typing
  (printable characters up to `MaxChars`), list selection by arrows and by pixel
  (`row_at`), Continue (create or choose), Delete and the 24-pilot limit, over
  the existing `ProfileSession` (`create`, `select`, `delete`).
- `pilot_view.rs`: draws the black list ground, selected/hover rows, row text,
  the typed name with a caret and the page's notice over the section.
- `window.rs`: `KeyboardInput` text, arrows, Enter, Esc and list presses reach
  the page while `Campaign` is shown; `CM_B_START`/`CM_B_DELETEPLAYER` (the
  shell's recorded unhandled presses) run Continue/Delete; Continue takes the
  F45 machine through `NewProfile`/`ContinueProfile` then `ConfirmProfile`
  (`StartFlow::choose_pilot`) to the cabin and records `OpenProfile`. The smoke
  script gains a typed-name and Down-selection step on its Campaign visit.
- Persistence: this project's own profile files (`ProfileSession`). A person's
  population is `$CS_PROFILE_DIR`, else `~/.crimson-skies`; a smoke run uses the
  synthetic population under its capture directory (`profiles/`).

## Unknown, and not claimed

- **The original save layout.** The original stores pilots through engine
  callbacks 2099/2100/2101/2106 whose file format is unread. Recorded unknown;
  the affected content is the original pilot files (no import).
- **`FONT_CM_EDIT`** is a GDI font id; its face and height are not in the
  installation (`2026-10-01-f51-fontcell-bitmap-font-mapping-not-in-installation.md`).
  Text draws in the engine's default face, a stand-in.
- **The scroll bar.** Slider/up/down placement and frame layout are unread. A
  first attempt stretching the art beside the list drew a flat block in the
  windowed capture, so the art is not drawn; the list scrolls by arrows.
- **Delete confirmation.** The original's yes/no `MessageBox` presenter is not
  built; the first press arms the delete and names the pilot, a second confirms,
  Esc/typing/selection cancel it. A design choice.
- **Esc.** `2026-10-22-parity5-start-flow-transitions.md` found no Esc handler on
  this page; the task asks for Esc = Cancel, implemented as a design addition
  (`StartFlow::escape` now steps back from `ProfileSelect` too; an armed delete
  consumes the Esc first).
- The script's `crashcheat!` name, `mail(10021, …)` and `$$A$$` sound callbacks.

## Reproduce

```sh
mkdir -p private/evidence/FE-MENU-3
CS_EVIDENCE_DIR=$PWD/private/evidence/FE-MENU-3 \
CS_FE_MENU_SMOKE_OUT=$PWD/private/evidence/FE-MENU-3 \
  cargo test --locked -p cs_app --test cs_app_tests_c -- accept_fe_menu_3_ --include-ignored
```
