# ZBD/C3/M01 win-route census (#2086): the blocks that win "The Lost Treasure"

Date: 2026-10-24. Task: #2086. Capability: `retail` (read-only `$CS_GAME_DIR`). Nothing
here ran the original executable; nothing is `verified_original`. Tests:
`accept_m01_c3_win_route_census_` in `crates/cs_app/tests/campaign/m01_c3_win_route_census.rs`
(both `#[ignore = "requires CS_GAME_DIR"]`).

Why: `m01_play_triggers_route.rs` measured `ZBD/C1C/M01` (work order M06 "The Red Menace").
The owner's M01 is `ZBD/C3/M01` (`2026-10-17-m01-archive-decision.md`). Its program has no
`workersvoyagezep` and no engine part chains. This census is read through the production
`plan_mission_launch` / `stage_for` closure for `label("M01")`, "The Lost Treasure".

Numbering: `OBJECTIVE<n>` is `objectives[n - 1]` in the lowered program. Wake, nap and
kill lists spell the `OBJECTIVE<n>` number itself (the executor decodes `n - 1`,
`cs_sim::objectives::directives`). "Awake" means no `BEGIN_DORMANT`, or one with seconds
>= 0 (a self-wake at that mission-clock second). `BEGIN_DORMANT -1` is "woken only by
another block". A block whose condition is only its own awake gate completes the tick it is
awake. Wake effects such as `WAKE_ANIM` and `WAKEUP_ENEMIES` are listed with the block
that spells them; per `2026-10-06-m01-lc-directive-c-...` `WAKE_ANIM` starts "on wake".

## 1. The 39 blocks

| n | starts | condition (beyond awake) | effects / links |
| --- | --- | --- | --- |
| 1 | self-wake 2.0 s | none | `WAKEUP_ZEP_TURRETS piratezep`, sound `snd_HI1Start` |
| 2 | awake | TRAVELERS player -> `t_chamber` <= 200 m | clears `t_chamber` target; wakes 3 |
| 3 | dormant | danger zone `dzpath1` | wakes 4 |
| 4 | dormant | none | music |
| 5 | dormant | TRAVELERS player -> (-5458, 120, -5390) <= 1100 m | sets targets `t_truck01..03`, `bridge_truck01`; wakes 6, 35; naps 33 (1 s); kills 36 |
| 6 | dormant | none | `WAKEUP_TURRETS t_truck**` |
| 7 | awake | ANIM `rope1burn` RUNNING | clears the four truck targets; naps 8 (3 s); kills 18, 5, 6 |
| 8 | dormant | none | wakes 31 |
| 9 | awake | TRAVELERS -> (-6311, -27, -4611) <= 200 m | clears `grasshut2` target; wakes 30, 36; naps 37 (1 s); kills 19, 35 |
| 10 | awake | none | no effects |
| 11 | awake | danger zone `dzpath4` | sound |
| **12** | awake | TRAVELERS -> `shipwreck` <= 200 m | PRIMARY 1; clears `shipwreck` target; **naps 27 (2 s)**; kills 19, 35 |
| **13** | dormant, gate 26 | DEDG group 1 <= 0, and 26 awake | PRIMARY 3; sets `pzhookpoint` target; **naps 14 (5 s)**; wakes 17, 20 |
| **14** | dormant | ANIM `hooked_to_klondike` EXECUTED | `WAKE_ANIM pzhomebase`; PRIMARY 4; **wakes 15** |
| **15** | dormant | none | **`INSTANTWIN` (`Finish(Succeeded)`)** |
| 16 | dormant | none | music `missionsuccess` |
| 17 | dormant | none | music `primaryobj` |
| 18 | dormant | none | `WAKEUP_SOUND_GROUP Zachary_33` |
| 19 | awake | TRAVELERS -> `t_chamber` <= 1500 m | wakes 5 |
| 20 | dormant | TRAVELERS -> `piratezep` <= 500 m | wakes 16 |
| **21** | dormant | ANIM `texdrop` EXECUTED | `WAKE_ANIM enable_dropoff`; PRIMARY 2; clears shipwreck/t_chamber/grasshut2/truck targets; **wakes 22**; kills 2, 3, 5, 6, 7, 8, 9, 18 |
| **22** | dormant | none | `WAKE_ANIM disable_dropoff`; **naps 23 (10 s)** |
| **23** | dormant | none | `WAKEUP_ENEMIES medkestrel_1..3`; **wakes 29, 28, 25, 13**; naps 39 (2 s) |
| 24 | dormant | none | naps 38 (31 s) |
| **25** | dormant | ANIM `hydrogen_fire` RUNNING | clears `hydrogentank1` target; **wakes 26**; kills 24, 38 |
| **26** | dormant | ANIM `volcano1` EXECUTED | none (its awake state is what 13 reads) |
| **27** | dormant | none | `shipwreck` help label + target flag; **naps 21 (3 s)** |
| 28 | dormant | none | `SET_AI_NET medkestrel_1..4 -> M1Medusas` |
| 29 | dormant | none | music `prebattle` |
| 30 | dormant | ANIM `volcano1` EXECUTED | none |
| 31 | dormant | 30 awake | SECONDARY 12; wakes 32 |
| 32 | dormant | none | music |
| 33 | dormant | none | sound; wakes 34; kills 35 |
| 34 | dormant | none | sound |
| 35 | dormant | none | sound `snd_HI1Brits`; kills 33 |
| 36 | dormant | none | sound; wakes 5; naps 18 (10 s); kills 37 |
| 37 | dormant | none | sound `snd_HI1Camp`; wakes 5; naps 18 (10 s); kills 36 |
| 38 | dormant | none | sets `hydrogentank1` target |
| 39 | dormant | none | `WAKEUP_ENEMIES cargozep1`, `WAKE_ANIM fadein_cg1zep`, `WAKEUP_ZEP_TURRETS cargozep1`; naps 24 (13 s) |

Awake from tick 0: 2, 7, 9, 10, 11, 12, 19 (and 1 at 2.0 s). Bold rows are the win route.
The program has exactly one `Finish`: block 15, and only block 14 wakes it.

## 2. The shortest route

Shortest wake/nap path (`shortest_chain` in the test): **12 -> 27 -> 21 -> 22 -> 23 -> 13 -> 14 -> 15**,
plus the branch 23 -> 25 -> 26 that block 13's gate needs. The steps for a scripted flight:

1. **Fly within 200 m of the `shipwreck` node** (block 12; examine site #3). 2 s later block 27
   marks it, 3 s after that block 21 is awake and its wake effect `enable_dropoff` runs.
2. **Land at one of the six `do_approach1..6` drop-off nodes.** `enable_dropoff` sets
   `land_on` ACTIVE on `do_approach1..6`; the landing rows are `angle 45`, `speed [50, 320]`.
   Each `do_approachN` runs `do_direction` and `CALL_ANIMATION texdrop`. `texdrop` EXECUTED
   completes block 21, which wakes 22; 10 s later block 23 runs.
3. **Block 23 wakes the Kestrels** `medkestrel_1..3` (group 1: the composed observation reports
   three) and wakes blocks 13 and 25.
4. **Destroy all of group 1** (block 13 `DEDG 1 <= 0`).
5. **Make `hydrogen_fire` run** (block 25; it belongs to the `cg1zep` gasbag records in
   `mis_anim.zbd`; the cargo zeppelin `cargozep1` is woken by block 39, 2 s after block 23, and
   block 38 marks `hydrogentank1` 46 s after block 23). Block 25 wakes 26, which block 13 requires
   awake. The 13 conjunction holds once both 4 and 5 are done, in either order.
6. Block 13 completes, 5 s later block 14 is awake and `WAKE_ANIM pzhomebase` arms the
   `piratezep` landing nodes; **land at `pz_manual_land`** (`angle 35`, `speed [50, 220]`) or the
   `auto` row `pz_auto_land`. `hooked_to_klondike` EXECUTED completes block 14 and wakes 15:
   `INSTANTWIN`.

Not on the route: the t_chamber site (2), the bridge convoy (5, 6, 7, trucks `t_truck01..03`,
`bridge_truck01`), the (-6311, -27, -4611) site (9), the danger zones, the `piratezep`
proximity (20). Block 21 kills 2, 3, 5, 6, 7, 8, 9 and 18 when it completes. No required block
is killed by another required block (asserted). Nothing in the program counts trucks.

Minimum mission time on the route after block 12 completes: 2 + 3 + landing + 10 + 5 + landing.

## 3. Production support in the windowed mission

| Step | Support | Evidence |
| --- | --- | --- |
| Proximity (12) | `Travelers` evaluator over the observation's member rows; examine objective HUD (#1754) | `2026-10-21-m01-examine-objective.md`; not re-driven here |
| `enable_dropoff` / `pzhomebase` `WAKE_ANIM` | stage-6b consumer exists | `2026-10-15-m01-play-triggers-anims-arm.md`; whether C3's names resolve is not driven here |
| **Landing at `do_approach1..6`** | **Missing.** `zbd/c3` landing table reads (11 rows, no refusals), but the six `do_approach` nodes compose **no position** (anchor `treasure_approach` is not a bound world actor), so the consumer finds no node and cannot land. Pinned by the second test; it must flip when fixed | test `..._the_composed_mission_carries_the_route_inputs` |
| `texdrop` content (`do_direction` rotate, callback 951 body detach) | Unknown. Callback 951 is a body release (`2026-10-19-m01-launch-body-pose.md`); what the player body does in the windowed mission is unmeasured | same |
| `WAKEUP_ENEMIES` Kestrels (23) | wake consumer exists (`m01_play_ai_wake`); asleep at tick 0 | composition output |
| Kill group 1 | gun hits and group counting exist generally; Kestrel kills on this route not driven here | `m01_hostile_engage`, DEDG tests |
| **`hydrogen_fire` RUNNING (25)** | **Unknown and unsupported.** No `CALL_ANIMATION`/`WAKE_ANIM` in the control program names it; the record sits in `mis_anim.zbd` among `cg1zep*_gasbag*`/`large_fireball` entries; what starts it (gasbag or tank damage?) is unmeasured | idle run: never starts |
| Docking (14) | landing rows `pz_manual_land` / `pz_auto_land` read; both compose a position (same `piratezep` offset); rules are the designed landing rules | `2026-10-16-m01-play-triggers-anims-land.md` |
| `volcano1` EXECUTED (26, 30) | not needed to complete 13 (only awake-ness of 26); start of `volcano1` unmeasured | -- |

An idle 80 s run changes none of `texdrop`, `hydrogen_fire`, `hooked_to_klondike`,
`rope1burn`, `volcano1`.

## 4. Unknowns that block a `--smoke-success` route

| Unknown | Affects | Resolver |
| --- | --- | --- |
| Position of `do_approach1..6` (anchor `treasure_approach`) | step 2, whole route | a task composing the node (static scene graph), under #2081 |
| What starts `hydrogen_fire` | step 5, the win | a static record/callback trace of `mis_anim.zbd` `cg1zep*` records |
| What `texdrop` does to the player body (callback 951) | step 2 aftermath | owner capture or static trace |
| Whether the original waits for group 1 to be awake first (kills of sleeping aircraft) | step 4 | none needed for the route as scripted |

## 5. Retargeting notes (#2081 / #2082 / #2083)

Posted on the tasks: the C1C blocks (3, 6, 8, 9 engine parts, wv_/hookup beats) do not exist
here; the beats become shipwreck proximity (12) -> drop-off landing (texdrop) -> Medusa wake
and kills (23, 13) -> hydrogen tank (25) -> docking (14, 15). The old test file
`m01_play_triggers_route.rs` is relabelled in its header comment as a C1C/M06 measurement.
