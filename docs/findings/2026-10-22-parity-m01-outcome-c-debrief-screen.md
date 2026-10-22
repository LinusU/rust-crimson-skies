# PARITY-M01-OUTCOME-C: the journal debrief screen after a failed run (#1655)

Date: 2026-10-22. Implementer: `claude-2` (Rally); no independent review yet.
Capabilities: `retail`, `gpu`, `audio`. Nothing here is `verified_original`; no
original executable was run. The owner's frames (Mission Failed, run time 1:37
and 1:11) stay in `private/`.

## What the installation states (measured)

* `GOSDATA/ASSETS/crimson.rof#ASSETS/LAYOUT.CSV` has the whole screen as the
  `[@ScrapBook@]` section: panes `SB_BACKGROUND` (`SB_BackGround.jpg`, 800x600)
  and `SB_STATCARD` (`SB_P_Card.png` at 403,297, two frames), buttons
  `SB_B_REPLAY` (594,505), `SB_B_TOC` (375,560), `SB_B_RETURNPC` (593,561),
  `SB_B_BEST` (432,283), `SB_B_MOST` (594,283), `SB_B_NEXT`/`SB_B_PREV`
  (708|0,465) and `SB_B_CURRENT` (558,7); text rows for the title line
  (58,53), the stat titles at x 417 (y 339, 369, 412, 388, 436, 460, 484) and
  the values at x 642.
* The captions are rows of `langui.dll` named through `RESOURCE.H`:
  `IDS_SB_FAILURE` = `Mission Failed` (`IDS_SB_SUCCESS` = `Mission Completed`),
  `IDS_SB_RESULTS_TITLE`, `IDS_SB_TIME_TITLE`, `IDS_SB_HITS_TITLE` (`Gun Hit
  Ratio`), `IDS_SB_CASH_TITLE`, `IDS_SB_PLANES_TITLE`, `IDS_SB_B_BEST`,
  `IDS_SB_B_RECENT`, `IDS_SBTOC_B_REPLAY`; the formats `IDS_SB_NAMEANDAREA`
  (`%1!s! - %2!s!`), `IDS_SB_PERCENTAGE_VAL` (`%1!d!%%`), `IDS_SB_CASH_VAL`
  (`$%1!d!`), `IDS_SB_PLANES_VAL`, `IDS_SB_TIME_VAL` (`%1!02d!:%2!02d!`).
* The title line is the player name beside the mission's **short** name:
  campaign-length title block 3480..=3503 (`The Lost Treasure` for M01), not
  the region-prefixed long block the Flight Check shows. The name is
  `IDS_DEFAULTPLAYERNAME` (500); its row decodes as `]Nathan Zachary` (a font
  tag whose opening is cut), and the stray `]` is trimmed.

## Code

`cs_app::mission_session::debrief_screen` (message formats, `DebriefScene`,
`DebriefScreen` with its button notices, `DebriefSource` read at launch,
`DebriefScreenPlugin`); `host::mission_host_tick` shows the screen instead of
writing the `AppExit` when the settled outcome is a failure *and* a source is
installed (a windowed run); a headless run keeps its exit. While the screen is
up the virtual clock is held and the mesh HUD and text HUD are switched off.
`--smoke-failure` (`smoke/debrief.rs`) is the owner-command smoke mode.
Wiring/support edits outside the new files: `flight_check.rs` (three row
helpers and `DESIGNED_TEXT_HEIGHT` made `pub(super)`, `read_names`),
`hud_art.rs` and `cockpit_hud.rs` (stand down under the screen), `compose.rs`
(the source is read at launch; `MissionCompositionError::Debrief`), `cli.rs` /
`mission_launch.rs` (the flag), `smoke.rs`.

## Recorded unknowns (listed in `DebriefScene::unknowns`, printed at run time)

* The **Mission Succeeded** variant is not photographed: the screen is built
  for a failure only; a settled success keeps its exit.
* The **Best to Date / Most Recent** tab states (which is selected, the strip
  frame of the selected tab, what each shows), the page arrows and View All
  Missions: drawn at their places with the art's first frame; a press reports a
  notice. Which frame of `SB_P_Card.png` is used when: frame 0 matches the
  owner's frame.
* **Run Time** is drawn `m:ss` as in the owner's frame; the string's own format
  would draw `01:37`.
* **Gun Hit Ratio** is truncated to a whole percent (the original's rounding is
  unmeasured); no shots is `0%`, as in the frames.
* **Cash Earned** is drawn `unknown` (no cash rule is measured); the frames show
  `$0` for failed runs.
* **Rockets Expended** (`SB_T_ROCKETSTITLE`) is not drawn in the frames; why is
  unknown.
* **Kill markers** (`SB_KILL0..10`, the strip with counts) and `SB_P_GRIME` are
  not drawn; **Current Mission** (`SB_B_CURRENT`) is not in the frames.
* Fonts are the engine's default; the label colours are black on the art.
* Replay Mission (#1656) and Return to Cabin (#1749) report that they are
  unavailable.
* Follow-up needed from the owner: original frames of the Mission Succeeded
  debrief and of the Best to Date / Most Recent tab states.

## Evidence

`cargo test --locked -p cs_app --test cs_app_tests_a accept_parity_m01_outcome_c_
-- --include-ignored`: 5 tests (message formats; the headless crash flow shows
the screen and writes no exit; no source keeps the exit; button notices; the
retail screen states the owner's frame values and positions).

Owner-command smoke `cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45
--smoke-failure --capture-dir private/evidence/PARITY-M01-OUTCOME-C` (release
build): exit 0, 6 checks, 0 failed; the lethal hit at frame 2471, the screen up
at 2472 with no `AppExit`; 744 per mille of the 2560x1440 frame off black.
Frames `frame_db02492_debrief.png` and `frame_db02500_debrief_bare.png` (private).
Against the owner's frame the journal, title line, stat card, the two tabs, the
four stat rows, Replay Mission, the page arrows and the two brass buttons match
in place; the text uses the engine's default font.
