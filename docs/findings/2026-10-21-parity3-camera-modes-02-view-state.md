# PARITY3-CAMERA-MODES.02 (#1824): view-mode state, external cameras and zoom

Date: 2026-10-21. Parent #1743. Predecessor: `2026-10-21-parity3-camera-modes-01-measured.md`.
Code: `crates/cs_app/src/mission_session/view_cycle.rs` (state),
`external_view.rs` (placement, zoom), `original_keys.rs` (key minting). Tests:
`crates/cs_app/tests/accept_parity3_camera_modes_02.rs`, prefix
`accept_parity3_camera_modes_02_`. Nothing here is `verified_original`.

## Measured and used (static, from #1823)

* Commands 50/51/52/53 (F9–F12) select modes 1/2/3/4; command 54 (**F7**)
  selects mode **9**, the held-point camera. It is a separate
  `MissionViewMode::HeldPoint`, not the ordinary chase (mode 0); F7 therefore
  no longer does nothing (a #1809 test that tapped F7 as an "other key" now
  taps only `A`).
* F8 from a mode outside {0, 6, 7} returns to the last cycle mode entered
  (`CycleMemory`, initially 6).
* Keep table (`MissionViewMode::keeps_state_into`); entering mode 0 clears the
  zoom.
* Zoom: held numpad `+`/`-` (commands 67/68, polled, so read from the held
  keys through the same table) move a target by 2/s inside `[0, 1]`; the
  smoothed value follows at rate 1.5 as `exp(-rate*dt)` (the exp reading is
  the finding's, not traced); mode 0 only, `smoothed * 10` extra distance.

## Designed (labelled in code)

The distances are in `camparam.zrd`, which nothing in the repository reads yet,
so the placements use the capture-fitted chase distance: Down above and astern,
Back astern at 15.5/13 of that (ratio of the measured default `back_dist_min`
to `dist`), Left/Right abeam, HeldPoint frozen where the camera was at entry,
all looking at the aircraft. The zoom distance unit is read as metres.
Chase-view zoom scales the offset of the rig's frame from the aircraft.

## Unknown / not done

* Loading `camparam.zrd` (per-airframe distances) — follow-up task.
* Mode 1/3/4/9 placement maths, mode 3/4 side flip, mode 9 timer and fly-by
  parameters; the guard `[[0x71c298]+0x91d]` that makes the keys inert.
* The unit of the zoom command-68 bit 4 modifier; look keys (#1825).
* No owner-command smoke was run; this slice claims no M01 visual acceptance.
