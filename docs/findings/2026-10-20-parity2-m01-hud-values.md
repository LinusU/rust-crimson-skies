# PARITY2-M01-HUD-VALUES: the HUD's start values against the original

Date: 2026-10-20. Task: #1732. Capabilities: `retail`, `gpu`, `audio`. Nothing here is
`verified_original`; the owner's capture (`original.png`, +25 s) is outside this checkout and was
not readable, its numbers are taken from the task description.

## Rockets and guns: the original's numbers are `weapons.zrd` record fields

The original at first control reads ROCKETS `BOOM` **3** and GUNS `SLUG` **2000**. Ours read 6 and
750 because both were *designed* constants (`MISSION_ROCKET_CAPACITY = 6`; `MISSION_STARTING_ROUNDS
= 250` per gun, summed over the three guns by `WeaponGauge::selected_rounds`), not data.

Read from `ZBD/zrdr.zbd` / `weapons.zrd` through the production `decode_zrd`:

| record | `NAME` | field | value |
| --- | --- | --- | --- |
| `wep_50` (the first slot of the fresh-campaign Flight Check record, #1628) | `50slug` | `AMMO_LIMIT` / `CLUSTER_SIZE` | 2000 / 2000 |
| `wep_40`, `wep_30` (slots 2, 3) | `40slug`, `30slug` | `AMMO_LIMIT` | 2400, 2800 |
| `wep_06` (rocket kind 1, label `BOOM`) | `BOOM` | `CLUSTER_SIZE` (no `AMMO_LIMIT`) | 3 |

So the GUNS count is one gun record's `AMMO_LIMIT` (the current gun, `wep_50`), **not** a sum over
the three guns, and the ROCKETS count is the `BOOM` record's `CLUSTER_SIZE`. Meaning of the
`/ 2000` beside the count: the same `AMMO_LIMIT`.

What changed: `mission_session::weapon_counts` reads the two fields for M01's retail stage
(`MissionStage::weapon_counts`); the weapon session starts every gun with 2000 rounds and the
rocket stack at 3; the gauges (`cockpit_hud`, `hud_art`) show the first selected gun's rounds.

Inferred, not measured: that the HUD number *is* those fields (two equalities, no read of the HUD
code); that rocket kind 1 is `wep_06` (the player block `wep_05..=wep_15` has one record per kind,
`wep_24` is a second `BOOM` record with `CLUSTER_SIZE` 7 the loadout is not shown to use). Unknown:
whether the original gives each gun its own pool (2000/2400/2800 by record) or shares one, and
whether the four rocket slots each hold 3. Our session gives every gun 2000 (one `register` value)
and one rocket launcher a stack of 3.

## MPH

At the very first control frame (`frame_02410_first_control`, tick 4818) our MPH is 0 because the
launch cinematic holds the body at zero velocity until the handover restores it
(`launch_cinematic::hold_body`); six frames later (`frame_02416_cinematic_handover`) the dial reads
**128 mph** and 167 mph by `frame_02806_hud`. 128 mph is the designed 55 m/s spawn speed
(`player_spawn_speed_m_s`, status `designed`; the original's was never recovered), inside the
original's 100-200 mph reading. The one-frame 0 is a held-body artefact, not a wrong value; whether
the original's first control frame also shows a restored speed is unmeasured.

## ALT

ALT is `position.y` in feet: 250 m = 820 ft at first control, from the spawn height. The unit
(feet) and whether the original reads terrain- or sea-relative height are unmeasured.

## Not fixed: the art gauges' digits are blank

In the smoke frames the ROCKETS and GUNS art dials' dark digit boxes are empty, and the `BOOM <n>`
text of `cockpit_hud` is not visible at all (it sits under the damage dial's art), so the rocket
count is **not visible** in `frame_02410_first_control.png`; SLUG 2000 is. The same was true of
the frames before this change (`private/evidence/PARITY2-M01-SEA-SHADING`). Recorded as a
follow-up task.

Evidence: owner-command smoke, 25 checks, no failures; frames
`private/evidence/M01-OWNER-SMOKE-1732/frame_02410_first_control.png`,
`frame_02416_cinematic_handover.png`, `frame_02806_hud.png` (not committed).
