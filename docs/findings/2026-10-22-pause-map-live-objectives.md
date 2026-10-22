# PARITY3-PAUSE-MAP-SCREEN.02-B: live pins, objectives note and player icon (#1874)

Date: 2026-10-22. Implementer: `claude-2`; no independent review yet. Provenance `static`;
nothing here is `verified_original`, and no original run was observed. The owner's Esc
screenshot is not in the task history or the checkout, so nothing below is fitted to it.

Code: `crates/cs_app/src/mission_session/pause_map.rs` (`PauseMapLive`, `read_pause_map_live`,
`pin_flag_bitmap`, `world_to_chart_pixel`), `crates/cs_app/src/ui/hud/objectives.rs`
(`HudObjective::revealed`, `ObjectiveHud::row_text`). Prefix `accept_parity3_pause_map_02b_`.

## What the data states

* `ESC_SCRIPT` of `loading_c61` holds `Objective OBJn index i` for n = 1..4, i = 0..3, next to
  `Pict OBJPINn`. So pin `OBJPINn` and note line `OBJn` both name objective index `n - 1`.
* M01's `IDENTITY` rows with a message element are `PRIMARY 1`..`PRIMARY 4` (objectives O9, O11,
  O15, O18); `SECONDARY 11` has no message. The reading: index `i` is the `i`-th such row in
  (role, ordinal) order, so index 0..3 are `PRIMARY 1`..`4`.
* `OBJECTIVESLIST CHECKMARK` is a record whose `BITMAP` is `obj_check1` (25x24); `LIST` has
  `POSITION [580 50]`, `WORDWRAP [190 255]`, `SPACING [5]`.

## Readings (unverified, each one named function or constant)

1. **"?" to number: `pin_flag_bitmap`.** A pin keeps the bitmap its script line names while its
   objective is not revealed, and shows `pin<ordinal>` (`pin6_1` for 6, since `pin6` is the "?"
   flag; ordinals above 8 keep the script bitmap) once it is. *Revealed* = the engine model
   marks it present and either completed or no longer `Dormant`, or it is the objective
   `ObjectiveHud::displayed` draws while nothing is live (the same fallback the HUD line uses,
   #1539). Consequence: M01's `OBJPIN4` is authored as the "4" flag, so it never shows "?";
   whether the original shows "?" there until objective 4 wakes is unknown. Alternatives not
   taken: "?" until completed, "?" until woken without the HUD fallback.
2. **Note list: `PauseObjectiveRow::rows_of`.** The revealed rows only (no hidden objective),
   text from the installation's `strings.dll` through the HUD's resolver (the message key when it
   does not resolve), completed rows carry `obj_check1`. The check mark is drawn immediately left
   of the list column (25 px, at `x = 555`), on the entry's first line: its placement rule is
   native and unread.
3. **Player icon: `world_to_chart_pixel`.** Candidate: the `WORLD` rectangle maps onto the chart's
   *clip* rectangle, `x` left to right, `-z` top to bottom (the second `WORLD` component
   descends, so it is `-z`). Evidence: only Lighthouse (-7898, -5765) -> (405, 248) against the pin
   at (396, 254), which the data finding already called one match; the smoke shows the icon on
   the coast at the player's start, which is plausible, not a measurement. The icon is
   `singledev` centred on the point, rotated clockwise by the nose's angle from `-z`
   (`player_fix`, `BODY_FORWARD`); that the art points up at rotation 0, the rotation sense and
   the update rate are unknown. **Needs the owner's screenshot (one known player position and
   heading) to fit or refute**; until then this is a candidate, not a claim.

## Still unknown / stand-ins

`MYZEP` / `PANDORA` icons, the note's real face (`Andy Bold` is not installed; Bevy's default
face stands in at 14 px), the original's stacking of the check mark, and whether the original
lists unrevealed objectives. Resolving: the owner's capture of the Esc screen at known objective
states.

## Evidence

* Headless: `cargo test --locked -p cs_app --test cs_app_tests_c accept_parity3_pause_map_ --include-ignored`
  -> 15 passed (5 of them `_02b_`, one needs `CS_GAME_DIR`).
* Owner-command smoke: `cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 60 --capture-dir
  private/evidence/PAUSE-MAP-02B` -> 58 checks, failures: none;
  `frame_03048_pause_map.png` (private, not committed) shows pin 1 as the "1" flag, pins 2 and 3
  "?", pin 4 "4", the note listing "1) Find the main treasure site." and the green aircraft icon
  on the chart.
