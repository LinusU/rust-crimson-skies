# Plane Construction screen: integration (#2036)

2026-10-24, task #2036 (integration step after #1958, #2042-#2046, #2051-#2053,
#2063). Run by `sonnet-1`. Nothing here is `verified_original`; the original
executable was not run and the smoke is scripted, not `human_play`.

State on main: `CONSTRUCTION_NOT_BUILT` is gone; the Plane Name dialog's OK opens
the hub fed from the pilot's plane; Ready opens `[@Purchase@]`, Sell opens the
hangar, the six tabs have their panels, and `PN_B_DEFAULT` picks the starting
loadout (see the hangar finding). This task adds the end-to-end test
`accept_parity5_cabin_plane_construction_screen_*` (Plane Name OK, fed hub,
Ready, Sell, Cancel, no marked notice on the path).

Still marked, each with its own task or finding: the hangar's Export and
too-few boxes (`EXPORT_NOT_BUILT`, `TOO_FEW_NOT_BUILT`; `A 22` unread).
`READY_NOT_BUILT` and `SELL_NOT_BUILT` remain only for a layout lacking the
`Purchase`/`Hangar` rows, which the retail installation does not.

Smoke: `CS_FRONT_END_SMOKE_SCENE=flow cs --cs-path <retail> --front-end
--smoke-seconds 30 --capture-dir private/evidence/2036` exited 0, `passed:
true`, 19 framebuffers, path `Campaign → PassengerCabin → PlaneName →
PlaneConstruction → PassengerCabin`, 0 unhandled focus moves.
`flow-plane-construction-purchase.png` shows the hub with the *CONSTRUCTION
TOTALS* panel open.

Observation, not fixed here: in that frame the purchase panel's airframe row
reads *Focke-Wulf Fw 193 Hellhound* while the hub's line reads *Devastator*; the
smoke scene's purchase seeding is not the pilot's working plane. Worth a
follow-up before the purchase panel is claimed to match the draft.
