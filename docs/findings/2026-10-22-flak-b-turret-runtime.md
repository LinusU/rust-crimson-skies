# FLAK-B-TURRET-RUNTIME (#1820): integration of the turret runtime in M01

Date: 2026-10-22. Capabilities used: `retail`. Nothing here is `verified_original`.

FLAK-B1 (live byte, wake ledger), B2 (update law) and B3 (fire into the F29
damage authority) are merged; this task adds the integration test
`accept_flak_b_retail_objective6_wakes_every_truck_and_each_one_shoots_the_player`
(`crates/cs_app/tests/accept_flak_b_turret_runtime.rs`). On retail M01:

* `t_truck01..03` start asleep and wake together from OBJECTIVE6's
  `WAKEUP_TURRETS ['t_truck**']`; nothing fires before they wake.
* With the player 100 m east and 40 m above `t_truck02`, each of the three
  trucks fires `wep_29` and the player's hull pool falls through the damage
  authority. `aagun**` produce no shots (they stay asleep in M01).

## Observation

A player directly above the trucks is not engaged by the middle truck: the
needed pitch exceeds the 60 degree limit (angle clamped at 1.047 rad), so the
15 degree alignment never holds. That is the measured update law working, not a
fault; the test therefore places the player off to the side.

## Still open (unchanged from FLAK-B3)

Zeppelin turret mount positions (`WAKEUP_ZEP_TURRETS` wakes entries but no
`piratezep` gun is built), and the smoke that reaches OBJECTIVE6 (FLAK-C5,
#1887). The M01 owner-command smoke is therefore not claimed here.
