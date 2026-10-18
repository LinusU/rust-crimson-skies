# M01-OUTCOME-A-ATTACKER (#1680): what can engage the M01 player in the first ~70 s

Date: 2026-10-19. Task #1680, follow-up of #1653. Capabilities used: **`retail`**
(read-only `$CS_GAME_DIR`) through production readers. The engine image was not
needed beyond the directive semantics already measured in
`2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md` and
`...-directive-c-...`. No original executable ran; nothing here is
`verified_original`. Archive: **`ZBD/C3/M01`** (not C1C). Test prefix
`accept_m01_outcome_attacker_`; production reader
`cs_app::cinematics::launch_attackers`.

## Verdict

**Retail data names no hostile aircraft that engages an unthreatened M01 player
in its first ~70 s. The owner capture's red fighter and left-wing fire at +70 s
are therefore not explained by anything read here; the cause is recorded as
unknown (below), not guessed.** Every candidate the task lists was measured:

| Candidate | Record / archive | Activation source | Result |
| --- | --- | --- | --- |
| (a) cinematic pirate fighters | scene node `piratefighter`, placed by animation records `generic_intro`, `gi_scene1`, `gi_pfighter1`, `gi_pfighter2` in `ZBD/C3/cam_anim.zbd` | the cinematic's `CALL_ANIMATION` closure from `generic_intro` (`NEW_GAME_START`) | **Animation scenery, not an AI actor.** See §1. |
| (b) time-driven or launch-driven program sites | `ZBD/C3/M01/zrdr.zbd` `objectives.zrd` | exactly one: `OBJECTIVE1`, `BEGIN_DORMANT [2.0]`, self-wake at mission clock 2.0 s | It wakes **`WAKEUP_ZEP_TURRETS [piratezep]`** (the Pandora's own turrets) and a sound group. **No aircraft.** See §2. |
| medkestrel_1..3 | `ZBD/C3/M01/zrdr.zbd` `aiv.zrd`, team 2, asleep (field 21 = 1) | only `OBJECTIVE23` `WAKEUP_ENEMIES` | Reachable only through `12 → 27 → 21 → 22 → 23`, whose one awake-at-start root is the **shipwreck** approach (200 m), ~8 km from the start. See §3. |
| (c) a Kestrel wake route not yet reported | none found | — | Every `WAKEUP_*` site of the program was enumerated: aircraft are named at `OBJECTIVE23` and `OBJECTIVE39` only (§3). |

## 1. The cinematic's pirate fighters (candidate a)

* `piratefighter` is not a world node of `ZBD/C3/gamez.zbd` (5 408 nodes; no
  node named `*fighter*`, `*kestrel*`, `*medusa*`). It is the **`nodename` of
  the Devastator airframe family** in `ZBD/zrdr.zbd` `vehicle.zrd`
  (`devastator`, `rusdevastator`, `rdevastator`, `wingman`) and a node (index
  2324) of `ZBD/planes.zbd`, the shared airframe archive. The player's own
  model node is `player_pfighter` (planes.zbd node 44) in `gi_playerdrop`'s
  node table. So the cinematic's "pirate fighters" are the Devastator model that
  the player and Betty also fly.
* The closure from `generic_intro` through `CALL_ANIMATION` reaches 24 records
  (`speed_cue`, `apzep_engines_start` and its prop records, `letterbox`,
  `gi_scene1`, `gi_scene2`, `gi_pfighter1`, `wing_lights_blink`, `spinprops`,
  `gi_1stperson`, `gi_playerdrop`, `gi_pfighter2`); only `open_pzeplaunchdoors`
  lives elsewhere. The `piratefighter` node occurs in the node tables of
  exactly `generic_intro`, `gi_scene1`, `gi_pfighter1`, `gi_pfighter2`.
* The closure's events are `SOUND`, `LIGHT_STATE`, `OBJECT_ACTIVE_STATE`,
  `OBJECT_TRANSLATE_STATE`, `OBJECT_ROTATE_STATE`, `OBJECT_MOTION`,
  `OBJECT_MOTION_SI_SCRIPT`, `OBJECT_ADD_CHILD`, `OBJECT_DELETE_CHILD`,
  `CAMERA_STATE`, `CALL_SEQUENCE`, `CALL_ANIMATION`, `STOP_ANIMATION`, `LOOP`,
  `IF`/`ELSE`/`ELSEIF`/`ENDIF`, `FBFX_COLOR_FROM_TO`, `PUFFER_STATE` and nine
  `CALLBACK`s. None is a weapon, damage or AI statement. The one
  gameplay-marker class opcode is `CALLBACK`; **what the nine callbacks do is
  unmeasured** (`player_setup` fires `0x14 0x2 0xb 0xe 0x391`).
* No `aiv.zrd` record is a pirate fighter: the mission's aircraft records are
  `player`, `medkestrel_1`, `medkestrel_2`, `wingman_1`, `medkestrel_3`.
* `gi_pfighter1`/`gi_pfighter2` move the node with `OBJECT_MOTION_SI_SCRIPT`
  tracks (`gi_pfighter1`: `IndexWords[0]` = 7, `gi_pfighter2`: 13) and
  `gi_pfighter2`'s last event sets `piratefighter` inactive
  (`OBJECT_ACTIVE_STATE`, state 0). `generic_intro`'s `RESET_SEQUENCE` deletes
  `piratefighter` from `piratezep`.

**Not decided:** whether that final inactive event actually runs after the
motion ends (its stored `START_TIME` reads 0; the sequencing of events after a
blocking motion event is unmeasured), and when the cinematic ends (`intro-clock`,
`2026-10-18-m01-launch-end-pose.md`). If the node stayed visible, a Devastator
model could be in frame at +70 s; that would be scenery, not an attacker, and
does not explain fire on the player's wing.

## 2. Time-driven sites (candidate b)

The self-wake rule is
`BEGIN_DORMANT >= 0` (mission-clock seconds, directive-b finding). Exactly one
site is non-negative: **`OBJECTIVE1` at 2.0 s**. Its wake effects are
`WAKEUP_ZEP_TURRETS ['piratezep']` and `WAKEUP_SOUND_GROUP ['snd_HI1Start']`.

* The Pandora's turret entries become live at 2.0 s. Whom they engage is
  **unknown**: M01's `piratezep` record states no `team` key, and the allegiance
  it resolves through the turret table
  (`2026-10-09-m01-lc-zeppelin-allegiance.md`, measured on the other archive's
  records) was not re-measured for `ZBD/C3/M01`. Affected content: whether the
  Pandora's cannons (`cannon_fire_range` 500 m) can hit the launched player.
  Resolving task: re-measure the `ZBD/C3/M01` `piratezep` allegiance and the
  turret targeting law.
* Objectives with no `BEGIN_DORMANT` (awake from tick 0): 2, 7, 9, 10, 11, 12, 19.
  Their conditions at the stored start `(-1426, 150, -1813)`: every `TRAVELERS`
  target is farther than its radius (`t_chamber` 4.1 km vs 200 m / 1500 m,
  the bridge 5.4 km vs 1100 m, the point at (-6311, -27, -4611) 5.6 km vs 200 m, the shipwreck 8.0 km
  vs 200 m). The `ANIM_STATE` / `DANGER_ZONES_COMPLETED` conditions of 7 and 11
  need `rope1burn` running and `dzpath4` flown. None completes unaided.

## 3. The Kestrels (medkestrel_1..3)

* `aiv.zrd`: team 2, asleep, positions around `(-5600, 300, -8250)`, heading
  90; targets `player` (0.5) and `piratezep` (-1.0).
* `WAKEUP_ENEMIES` sites in the whole program: `OBJECTIVE23`
  (`medkestrel_1..3`) and `OBJECTIVE39` (`cargozep1`, a zeppelin).
* Transitive wake/nap predecessors of `OBJECTIVE23`: **12, 21, 22, 27.**
  `OBJECTIVE12` (awake from tick 0) completes when the player is within 200 m of
  `shipwreck`; it naps 27 (2 s), which naps 21 (3 s); 21 also needs `ANIM_STATE
  texdrop EXECUTED` and wakes 22; 22 naps 23 (10 s). So the chain starts at the
  **shipwreck** (`OBJECTIVE12`), not at `OBJECTIVE2` (`t_chamber`) as #1653's
  description said. Nothing else wakes them.
* After the wake, `OBJECTIVE28` `SET_AI_NET`s them onto net `M1Medusas`
  (`neindex.zrd` id 1, `ne000001.zrd`): a ten-waypoint loop at altitude 250
  within 600 m of the shipwreck, ~8 km from the start.
* A headless idle run therefore wakes no hostile aircraft, matching #1653.

## What stays unknown

| unknown | affects | resolver |
| --- | --- | --- |
| What produced the red fighter and left-wing fire in the owner capture at +70 s (the owner may have flown; unrecorded) | #1653's parity target | an owner run with a recorded input trace, or a static trace of the launch path after the cinematic |
| What the nine cinematic `CALLBACK`s do | whether a callback arms an attacker | static analysis of the callback table (`0x14 0x2 0xb 0xe 0x391` among them) |
| Whether `piratefighter` is deactivated at the end of `gi_pfighter2` and when the intro ends | scenery visible at +70 s | #1674 (`intro-clock`) |
| `piratezep` turret allegiance for `ZBD/C3/M01` and the turret fire law | whether the woken Pandora turrets shoot the player | a task re-measuring both for C3/M01 |
| The original's AI wake for sleeping aircraft beyond `WAKEUP_ENEMIES` (e.g. an engine-side proximity wake, `activation` range in `vehicle.zrd`) | whether sleeping Kestrels wake without the program | static analysis of `0x48c220` / the sleep routine `0x4b0f40` |

## No follow-up wiring task

No launch-driven or time-driven activation of an aircraft exists in the data, so
there is nothing for #1653 to depend on from this finding. #1653's description
should read OBJECTIVE12 (shipwreck) where it says OBJECTIVE2.

## Files

* `crates/cs_app/src/cinematics/launch_attackers.rs` (new): the survey and the
  objectives wake-graph reader.
* `crates/cs_app/src/cinematics/mod.rs`: module declaration (wiring).
* `crates/cs_app/tests/campaign/m01_outcome_attacker.rs`, `campaign/mod.rs`
  (module declaration): one CI test, four ignored retail tests.
