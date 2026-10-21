# PARITY5-FRONTEND-BRIEFING-MAP: the briefing map screen (#1750)

Date: 2026-10-21. Implementer: `claude-2/claude-1` (Rally); no independent
review yet. Capabilities: `retail`, `gpu`, `audio` (read access to the
installation; nothing here is `verified_original`, no original executable was
run). The owner's capture of the original briefing (800x600) was compared by
eye with our frames; those images stay in `private/`.

## What the data states (measured)

* `ZBD/zrdr.zbd#Briefing.zrd` holds the whole screen. `BRIEFINGDIALOG` has the
  objectives panel (`parchment` at (0,295), title string `MSG_BRF_DLG_OBJECTIVES`
  at (35,315), list at (35,335), wrap 185x240, spacing 5) and the three buttons
  (`brief_button1`, 196x32, at (197|397|597, 560), labels
  `MSG_BTN_REPLAY_BRIEFING` / `MSG_BTN_RETURN_TO_CABIN` /
  `MSG_BTN_GO_TO_FLIGHT_CHECK` at offset (98|100, 4), three font names each).
* 24 `brief_c<NN>` scripts. The script of a mission is the one whose
  `PlaySound` names `briefing_c<chapter>m<mission>` (campaign position from
  `SourceContext`): M01 (`zbd/c3/m01`, chapter 1 mission 1) is `brief_c61`, map
  `HA-m1MAP`, sound `briefing_c1m1`.
* `sounds.zrd` maps the group `brief_c61` to `c1-HA-m1_briefing.wav`
  (`ZBD/soundsh.zbd`, MS-ADPCM mono 44.1 kHz, 98.7 s). Its `cue ` chunk holds
  eight points (ids 1..8) at 14.40, 22.30, 36.30, 43.50, 51.40, 65.30, 71.30
  and 94.50 s; `soundsl.zbd` holds the same cue times at 11.025 kHz.
* The command set of all 24 scripts is twelve commands: `PlaySound`,
  `WaitForMarker`, `Wait`, `Pict`, `Fade`, `Spin`, `Move`, `On`, `Off`/`OFF`,
  `ToBack`, `Objective`, `Line`. Every bitmap of M01's script is in
  `ZBD/rimage.zbd` (case-insensitive names).

## Code

`cs_content::briefing` (decoder, wave cue reader), `cs_app::ui::front_end::briefing`
(timeline, scene, playback), `cs_app::mission_session::briefing_screen`
(retail reader, voice over the audible device, window systems, smoke probe),
`FlightCheckPhase` (stage: Briefing, Flight Check, Cabin, Released),
`smoke/briefing.rs` (eight new owner-smoke checks). `cs --mission M01` now
opens on the briefing.

## Recorded unknowns (drawn as stated, listed in `BriefingScene::unknowns`)

* Whether `Fade`/`Spin`/`Move` run beside the script or hold it (drawn: beside).
* What `Spin` is relative to (drawn: from `startrevs` to `revs`, clockwise
  positive; the owner's capture shows the treasure map tilted about +18 deg,
  which fits `revs` 0.05 as the rest angle).
* Which cue is marker n: the cue ids are not in time order (id 6 is first).
  Drawn: marker n = nth earliest cue. Evidence: only this reading plays the
  script to the sound's end (marker 7 at 94.5 s + the 4.5 s final move = 99 s).
* Fonts (`ObjList`, `BtnLabel*`) and the cursor bitmaps are unread: default
  font, designed colours (white; red under the pointer, read off the capture).
* The briefing's state when Return to Briefing comes back from the Flight Check
  (drawn: restarts). The F41 bus of the briefing voice (device applies none).
* Return to Cabin: the button is wired to the machine's `Back` row, but the
  window has no cabin hub (#1749); without `with_cabin_hub()` it reports
  "unavailable" (a notice), never a silent no-op. The hub task flips it.

## Evidence

Owner-command smoke on the task commit (release build, retail, GPU, audio):
`cs --cs-path "$CS_GAME_DIR" --mission M01 --smoke-seconds 45 --capture-dir
private/evidence/PARITY5-BRIEFING` exit 0, 36 checks, 0 failed, including
`briefing_screen_drawn`, `briefing_elements_match_script`, `briefing_animates`,
`briefing_buttons_drawn`, `briefing_voice_plays` (210k sample frames pulled by
the output stream), `briefing_replay_restarts`, `briefing_return_trip` and
`briefing_go_to_flight_check`. Frames `frame_br00..09_*.png` are the briefing at
2 s, 3 s after each marker and just past the end. Against the owner's capture the
map, printer's panel, treasure map (position, tilt), buttons and their order
match; label text is the engine's default font, not the original's.
