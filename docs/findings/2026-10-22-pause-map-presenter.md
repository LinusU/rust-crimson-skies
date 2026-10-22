# PARITY3-PAUSE-MAP-SCREEN.02-A: the Esc pause screen presenter (#1873)

Date: 2026-10-22. Implementer: `claude-2`; no independent review yet. Provenance `static`;
nothing here is `verified_original`.

`crates/cs_app/src/mission_session/pause_map.rs` reads `ZBD/zrdr.zbd#escape.zrd` for the running
mission (key: the briefing script's `brief_*` key read as `loading_*`; M01 = `loading_c61`) and
draws, while the player's Esc pause holds the session: the `loadframe` background, the chart at
`POSITION` clipped to `CLIP`, the memento photo and its shadow, the parchment note with the
`MSG_BRF_DLG_OBJECTIVES` title, one flag pin per `OBJPINn` at the authored pixels, the four
`escape_button1/2/3` buttons with `MSG_BTN_*` labels, and the `daglove`/`dafinger` cursor.
Resume resumes the session; Restart, Preferences and Quit emit a `PauseMapPress` message and do
nothing else (tasks #1875, #1877, #1876). The old text panel of `original_keys.rs` is removed;
`F1` help is unchanged.

## Stand-ins and unknowns

* **Fonts.** `Andy Bold` and `Trebuchet MS Bold` are not installed; the title and labels use
  Bevy's default face at 16 / 14 logical px. Colours are `fonts.zrd`'s (title 16,16,16; labels
  normal 10,10,10, rollover 105,100,85, activate 180,175,151). Italic and weight are not applied.
* **Pins** are all drawn as the record's bitmap (three "?" and one "4" for M01); the "?" to number
  rule stays unknown (#1874).
* **Not drawn yet:** the objective list text and check marks, the player icon (`singledev`) and the
  `WORLD` to pixel mapping, `MYZEP`/`PANDORA` icons (#1874).
* **Draw order** between the photo, note and pins is a reading (`ToBack OBJECTIVESLIST`, shadow
  under photo); no data states the full stacking.
* **Cursor:** `daglove` with its top-left corner at the pointer (`CENTER [0]`), `dafinger` over a
  button; the system pointer is hidden while the screen is up. Which cursor art is the "brass
  arrow" is not decided (see the data finding).
* **Mesh HUD** (dials, compass) is drawn by its own camera over the UI, so the screen hides it
  while open.
* The pre-mission screens (briefing, Flight Check) never show the map; Esc there still pauses the
  session as before.

## Evidence

Owner-command smoke (`docs/PLAYTEST.md`): `cs --cs-path <retail> --mission M01 --smoke-seconds 60
--capture-dir private/evidence/PAUSE-MAP-02A` -> 58 checks, failures: none; the new phase presses
Esc, photographs `frame_03048_pause_map.png`, and presses Esc to resume.
