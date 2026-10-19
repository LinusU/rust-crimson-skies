# M01-LC-FLIGHTCHECK: the Flight Check with the measured loadout (#1661)

Integration of #1669..#1672. `FlightCheckView::from_retail` now fills both slots'
gun and rocket lists from the committed fresh-campaign rows
(`data/owner-static/engine-facts.toml`, M01-LC-FC-B; `static`, not
`verified_original`) and the installation's string table:

* gun line: item-number string (id 519, the `%1!d!)` insertion) + caliber label
  (`3320 + gun`) + ` ` + ammunition label (`3365 + kind`); rocket line: number +
  label (`3395 + rocket`). Leading font tags (`[COUR9]`) are stripped.
* The space between the number and the label is unmeasured (designed).
* A list stays `unknown` when a string row or the committed facts do not read.

Owner-command smoke on this commit (release build, retail, GPU, audio):
`cs --cs-path "$CS_GAME_DIR" --mission M01 --smoke-seconds 45 --capture-dir private/evidence/M01-LC-FLIGHTCHECK`
exit 0, 19 checks, 0 failed, including the four `flight_check_*` checks.
`frame_fc00020_flight_check.png` shows the title, the four objectives from
`MSG_BRF_HAM1_OBJn`, the pilot's plane (Devastator) with six gun and four
rocket lines, the wingman block, and Return to Briefing / Fly Mission.
Screenshots stay in `private/`.

## Still unknown

* The wingman's plane (drawn `unknown`; no source binds it).
* Whether anything rewrites the live plane records before M01's Flight Check
  (FC-B finding); the lists are the fresh-profile defaults.
* The `Change Ammo` / `Change Plane` labels are not visible on the pill art, and
  the Return to Briefing button has no briefing to return to yet (#1642).
