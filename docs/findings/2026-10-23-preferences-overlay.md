# Preferences overlay in the mission window (#1922, PARITY3-PAUSE-MAP-SCREEN.02-E.01)

Date: 2026-10-23. Capability used: `retail` (read-only `$CS_GAME_DIR`). Nothing
here is `verified_original`; the original executable was not run.

`crates/cs_app/src/mission_session/preferences_overlay.rs` draws the
`[@Preferences@]` section of `ASSETS/LAYOUT.CSV` over the composed mission
window as Bevy UI nodes, through the same `MenuShell` the standalone shell
uses (so positions, strip frames and per-button effects are the shell's
reading). It exposes `OpenPreferencesOverlay` / `ClosePreferencesOverlay`
messages, a `PreferencesOverlay` record (rows drawn, rows skipped, other row
kinds, presses) and `PreferencesPressed` messages.

## What the data states, and what the overlay does with it

- The section has 2 `P` rows (`PF_BACKGROUND` 110,215; `PF_LOGO` 134,13), 6 `B`
  rows (four option buttons, `PF_B_MAINMENU` 462,504 and `PF_B_RETURNTOGAME`
  455,509), plus `M` and `T` rows (counted, not drawn).
- `PREFERENCES.SCRIPT` picks the bottom slot by the in-game flag `$$KW$$`:
  `pf_b_returntogame` when set, `pf_b_mainmenu` otherwise; the logo and movie
  rows are created only when set. The Esc screen's Preferences is the in-game
  case (`2026-10-22-parity5-start-flow-transitions.md`), so the overlay hides
  `PF_B_MAINMENU` (not drawn, not hit) and draws `PF_B_RETURNTOGAME`.
  `read_preferences_overlay` refuses a script whose text does not state both
  the slot selection and the `callback($$A$$, 24)` arm under `$$KW$$`.
- **Return to Game** has the shell's `Unhandled(NoHandler)` reading (the arm is
  keyed by the slot's object, not the button id). The script's arm for the slot
  under `$$KW$$` is `callback($$A$$, 24)`; native `A 24` is the **statically**
  read "resume from the in-game Preferences" of the transitions finding
  (`static`, not run). The press closes the overlay and is recorded as
  `ReturnToGame`; the overlay does not resume the session or touch the clock.
- Game Options / Audio / Video / Controls start layout scripts
  (`GameOptions`, `Audio`, `Video`, `ControlsPrefs`) the overlay does not
  present: recorded as `Navigate(section)`, the page stays.

## Unknown

- **The backdrop.** The `M` row is a movie that is not read; nothing is drawn
  behind the panes (the pause map stays visible below). No backdrop is invented.
- The `T` (text) rows are counted and not drawn; their strings are not read.
- Draw order beyond declaration order, focus/tab order, the original's enabled
  states and `A 24`'s run-time effect (see the shell's module docs).
- Pointer input only; no keyboard or Esc handling.
- While both screens are up the pause map's own buttons still see clicks; the
  wiring that opens the overlay from the pause map's Preferences button and
  gates that is #1923 (.02-E.02).

## Not yet shown on a framebuffer

Nothing opens the overlay from `cs --mission` yet (that is #1923), so the
owner-command smoke cannot capture it; the acceptance test drives the composed
retail mission headlessly. The framebuffer screenshot AGENTS rule 6 asks for
belongs to #1923 or the owner-command smoke after it.
