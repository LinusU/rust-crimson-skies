# FE-MENU-2: the main menu's flag-movie backdrop, logo/frame panes and six buttons

Date: 2026-10-23. Task: #1834 / FE-MENU-2 "Front-end presenter 2/4: main menu
backdrop (CrimFlag.MPG loop), logo/frame composition and six working buttons"
(`specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`, sections
`### F45-B` and `### F45-D`). Shared contract: `docs/contracts/CLI-EVIDENCE.md`.
Required capabilities: `retail` and `gpu` (both present:
`CS_CAPABILITIES=retail,gpu,audio`). Task test prefix: `accept_fe_menu_2_`.
Evidence: `docs/findings/evidence/FE-MENU-2.json`, artifacts in
`private/evidence/FE-MENU-2/` (git-ignored; screenshots and original pixels
never enter Git).

Implemented by `swe2-max-1` (session of 2026-10-23, continuing checkpointed
WIP from the same agent). This file is the implementer's record; it is not a
review and awards no `verified_original`/`release_approved`. `retail` below
means read access to the owner's installation, never a run of the original
executable: no agent ran `crimson.exe`, and no capture here is a screenshot
of the original renderer.

## What the original data says (measured, not guessed)

From the installation's own `ASSETS/LAYOUT.CSV` `[MainMenu]` section:

- `MM_LOGO=P,MM_Logo.png,134,13,0,1,0,1,0` and
  `BFRAME=P,MainMenuButtonsStructure.png,245,256,0,1,0,1,0` — the logo and the
  button-frame panes, presented in declaration order.
- `MOVIE=M,CrimFlag.MPG,0,0,0,250,250,0,0` — the backdrop movie. The row's
  `ScaleX`/`ScaleY` are the *drawn* size in percent, not pixels: the video
  decodes at 320x240 @30.00 fps and is drawn at 250%x250%, i.e. the whole
  authored 800x600 surface. `# Loops=0` is the file's continuous-loop record.
- Six `B` rows (`MM_B_CAMPAIGN`, `MM_B_INSTANTACTION`, `MM_B_MULTIPLAYER`,
  `MM_B_PREFERENCES`, `MM_B_CREDITS`, `MM_B_QUIT`) — each writes `ResID=0`,
  i.e. no label record: the button text is baked into the strip art.
  `ShellButton.res_id` keeps the column verbatim (`None` for empty/`0`), so a
  later label task reads the row as written instead of re-deriving it.

The `M`-row header in the file omits the file column the data rows write
first — the same header staleness F45-B.1 recorded on the button header. The
section record `front-end-mainmenu.json` in the evidence directory is the
shell's own dump of all of the above.

## What this stage built

- `shell.rs`: `M` rows present as `ShellMovie` (file, authored origin, z,
  percent scale, loop count, region); the drawn rect waits for the decoded
  extent (`ShellMovie::rect_at`) because the row carries scale, not pixels.
- `movie.rs`: `MovieBackdrop` decodes through the production `MovieSource`,
  presents each frame at its own timestamp, loops continuously on
  `loops == 0`, stops after a finite count holding the last delivered frame,
  records decode/reopen failures through `MediaFailure` and never fabricates
  pixels. A loop restarts the cycle where the ended stream's last frame
  finishes its nominal display time (last pts relative to the cycle base,
  plus one frame interval from the measured rate) — the original's exact
  restart timing is unread; this is a stated design choice. `movie_media`
  keys the media `ContentId` by the catalog's `install_file_key`, the same
  key the install-file inventory row carries.
- `window.rs`: the windowed presenter spawns each section's movies below its
  panes/buttons; the smoke's scripted `HoverButton` matches row ids
  case-insensitively (layout ids are stored upper-cased; the script names
  them as the file writes them — a case-sensitive match silently wrote no
  `CursorMoved` and the run never reached Preferences or Quit).
- `start_flow.rs` (WIP commit): the six presses route through the F45 start
  flow on the real input path — Campaign→ProfileSelect,
  InstantAction→IaSelect, Preferences→Settings, Credits→Credits,
  Quit→application exit. Multiplayer's `activate(@MultiPlayerMain@)` names a
  section the layout does not hold, so the press stands for the table's
  `OpenLobby` → `Screen::Lobby`, refused `NotYetPresented` while the lobby
  presenter is unbuilt — and in the shell-only smoke it is the run's one
  recorded unhandled press.

## The windowed run and the owner's reference frame

`cs --cs-path $CS_GAME_DIR --front-end --smoke-seconds 14 --capture-dir
private/evidence/FE-MENU-2` on this machine's GPU (Apple M3 Pro, Metal):

- 840 frames presented, 302 decoded movie frames reached the backdrop's
  texture (per `report.json`, `passed:true`, `quit_seen:true`,
  `unhandled:1`).
- The pointer walk visited MainMenu → Campaign → MainMenu → InstantAction →
  MainMenu → Preferences → MainMenu → Credits → MainMenu, hovered every
  `MM_B_*` button and the other sections' exit rows, and Enter on Quit
  delivered `AppExit`.

Frame comparison (both git-ignored; paths are the citation):

- Ours: `private/evidence/FE-MENU-2/mainmenu.png`
- Owner's original capture: `private/ref-drydock/menu/original.png`

Side by side the composition agrees: the Zeppelin-marked logo top-left at
its authored offset, the bevelled button frame center-left holding six
labelled strips, and the waving Crimson flag filling the background behind
both. The flag's pose differs — they are different decoded frames of the
same `CrimFlag.MPG`, an expected difference for a looped movie, not a
composition mismatch. This is a visual correspondence between our renderer
and an owner-supplied screenshot of the original; it is not
`verified_original` evidence and no claim about the original executable's
internal behavior is made.

## Recorded unknowns (not fabricated)

- The original layout's focus/tab order is unread: Up/Down walks declaration
  order, and the cursor art is the first named pointer member that decodes
  (`PASSIVEPOINTERZ.PNG`), never claimed as the original's choice.
- `# Loops=0` reads as continuous looping and the restart lands on the
  stream's own period; the original's restart timing is unread.
- Which string table answers a nonzero `ResID`/`IDS_*` and how a label
  composes over the strip art are unread; the six MainMenu buttons write
  `ResID=0` regardless.
- The movie's own audio is FE-MENU-4's work; the movie z-order beyond
  declaration order is unverified.

## Checks (this branch, this machine)

- `cargo fmt --all -- --check` — clean.
- `cargo clippy --workspace --all-targets --all-features --locked --
  -D warnings` — clean.
- `cargo test --locked -p cs_app` — all green.
- `cargo test --locked -p cs_app --test cs_app_tests_c -- accept_fe_menu_2_
  --include-ignored` (with `CS_GAME_DIR`, `CS_CAPABILITIES=retail,gpu`) —
  8 passed, 0 failed.
- `cargo test --locked -p cs_app --test cs_app_tests_c -- accept_fe_menu_1_
  --include-ignored` — 9 passed, 0 failed (regression over the shared
  shell/window code).
- Evidence harness `evidence_report_fe_menu_2` — wrote
  `private/evidence/FE-MENU-2/acceptance.json`; validated by
  `tools/validate_evidence.py --artifact-root private/evidence/FE-MENU-2
  --require-pass` (`structurally_valid: true`, 14 artifacts).
