# M01's designed pursuit-and-fire law (M01-PLAY-AI, #1436)

Task: **#1436 M01-PLAY-AI** — "Give M01's aircraft and ground units a basic AI:
Kestrels attack, Betty follows, trucks fire". This finding records exactly
what the slice measured, what it designed and what it left unknown, so a
reviewer can tell the three apart without re-deriving them.

Nothing here is `verified_original`. The flight law the aircraft fly is
`OWNER-STATIC-2026-10-08` static analysis of the owner's decrypted image
(#796); the control law on top of it is project design.

## 1. Measured, and used verbatim

| Input | Where it comes from |
| --- | --- |
| The twelve `aiv.zrd` records, their `team`, `group`, `enabled`, `primary_target`, `title` and `deactivated` fields, and their stored poses | #1453's record layout (`docs/findings/2026-10-15-m01-aiv-layout.md`), #436's position unit, #681's sleep field |
| The `vehicle.zrd` record each name prefix dispatches to (`bsfury`, `devastator`, `rusdevastator`, `wingman`) | #1453's name-prefix dispatch; the import itself is `cs_content::original_airframe` (#796) |
| Every per-airframe flight parameter the law consumes (`roll_torque`, `pitch_torque`, `rudder_torque`, `rec_moments_inertia`, `fd_speed`, `veh_weight`, `ref_area`, engine factor, …) and the globals (`gravity = 20`, `liftAOAs`, authority limits, fades) | the retail import (#796), provenance `OWNER-STATIC-2026-10-08` |
| The law's own two AI branches (lift target always along the nose, 4.47 m/s forward-speed floor) and the fact that fuel is burned only for a player seat | `cs_sim::flight::original` (#796) |
| The player's team (`1`) — the value the role split compares against — and the `player` record's own dispatch | `aiv.zrd` via #1453 |

Imported chains (measured through the production reader, asserted by
`accept_m01_play_ai_the_stage_imports_an_airframe_for_every_scripted_record`):

| prefix | `kind_of` chain | weight | `fd_speed` | engine |
| --- | --- | --- | --- | --- |
| `bsfury` | `basic_airplane → fury → bsfury` | 1500 | 126 | Fury Lvl-2 (0.59) |
| `devastator` | `basic_airplane → devastator` | 2850 | 113 | Devastator Lvl-2 (0.65) |
| `rusdevastator` | `basic_airplane → devastator → rusdevastator` | 2850 | 113 | Devastator Lvl-2 (0.65) |
| `wingman` | `basic_airplane → devastator → wingman` | 2850 | 113 | Devastator Lvl-2 (0.65) |

## 2. Designed, with the value and why

Everything in this section is project design. The original's AI control law
sits at `0x48c220` and was never recovered (`docs/findings/
2026-10-08-flight-original-fixed-wing-law.md`, open unknown), so there is no
original number to be faithful to; each value is stated so it can be replaced
when one is measured.

### The steering half (`crates/cs_app/src/mission_ai.rs`)

The law aims the nose at a point, banks to build turn rate and adds rudder
for the first degrees of alignment — one `FlightInput` per fixed tick,
written to the same command boundary the player's seat uses.

| Constant | Value | Why this number |
| --- | --- | --- |
| `PURSUIT_MAX_BANK_RAD` | 60° | Bounds how hard an attacker commits to a turn so it cannot roll inverted while closing. |
| `PURSUIT_BANK_GAIN` | 1.6 / rad | A 60° bank error saturates the roll axis, so the turn is committed immediately and eased as the bank arrives. |
| `PURSUIT_MAX_PITCH_RAD` | 35° | Bounds a dive or climb so a target below the nose cannot pull the aircraft through a loop to reach it. |
| `PURSUIT_PITCH_GAIN` | 1.8 / rad | A ~32° elevation error saturates the pitch axis. |
| `PURSUIT_YAW_GAIN` | 0.5 / rad | Rudder lines the nose up in the first degrees; the bank does the fast turning. |

### Throttle and station

| Constant | Value | Why this number |
| --- | --- | --- |
| `PURSUIT_THROTTLE` | 1.0 | Full power for a pursuer. The original's AI throttle policy is part of `0x48c220` and unrecovered. |
| Start throttle | 0.75 (`CRUISE_THROTTLE`) | The session's own cruise, the value the player's record starts at — not an AI measurement. |
| `HOLD_COURSE_LOOKAHEAD_M` | 1200 m | A point far enough ahead that the steering half reads "keep the heading and altitude" without amplifying pose noise. |
| `STATION_OFFSET_M` | 45 m outboard, 18 m up, 75 m astern (~89 m from the leader) | A designed wingman position behind the leader's own body frame (`+Z` is astern). Chosen as a readable station, not a formation measurement. |
| `WINGMAN_THROTTLE_GAIN` | 0.003 / m | A 100 m station error adds 0.30 throttle; large enough to close a 270 m spawn gap inside the acceptance window, small enough not to oscillate at the station. |
| `WINGMAN_MIN_THROTTLE` | 0.2 | A station error behind the wingman can never leave it at idle and falling. |

The wingman's base throttle is its leader's speed as a fraction of its own
`fd_speed`, never below the session cruise: M01's leader (`devastator_2`) and
wingman (`wingman_2`) import the same airframe chain, so equal throttle at a
zero station error holds the station by construction.

### The role split

`behaviour_for` reads only measured fields, but its **reading** is designed:

* `starts_asleep` non-zero → no law is attached at all (the record keeps the
  kinematic held body #1435 gave it) — measured sleep, measured hold.
* `team == player team` (both measured) → the player's flight: a record with
  a stored `primary_target` is a wingman **on that leader**, a record without
  one holds its course at the session cruise.
* any other measured team → opposed: it engages its stored `primary_target`
  by name, or, when it stores none (M01's four `rusdevastator_*` records),
  **the player body** — designed, and the behaviour the task asks for.
* a record whose `team` was never read states no side: it holds its course.
  No hostility is invented for missing data.

What a team number means engine-wide, and what the original did with a
stored primary target, remain unmeasured
(`docs/findings/2026-10-15-m01-aiv-layout.md`, unknowns 4 and 5).

### Fire, arming and the roster (all designed)

The gun rig itself is #1437's designed M01 loadout (three `designed .50
caliber` guns on the mission's designed hardpoints, 4-tick bank cadence,
640 m/s muzzle). This task adds the AI side of it:

| Constant | Value | Why this number |
| --- | --- | --- |
| `FIRE_RANGE_M` | 600 m | The designed round flies 640 m/s for 90 ticks (~480 m), so a shot taken inside this window can still reach its target when it expires. The measured run ends the four attackers 334–428 m from the player, inside it. |
| `FIRE_CONE_RAD` | ~12° | A nose within 12° of the target's direction counts as aimed: tight enough that a pass is not a shot, loose enough that a closing attacker fires before the geometry is perfect. |
| `FIRE_INTERVAL_TICKS` | 8 | How often the AI *asks* to fire (15 Hz). The bank's own cadence still decides whether a round leaves the barrel — this is only the ask. |
| `AI_STARTING_ROUNDS` | 500 | The player's designed loadout starts with 250 (#1437); an attacker that never reloads starts with more. Neither number is a measurement. |
| `MISSION_AI_FIRE_PRODUCER` | `205` | The intent serial the AI's shots carry; 201–204 are the mission's router, ordnance, damage and cockpit producers (#1437). |
| Faction rostering | hostile for opposed records, the player's own flight otherwise | Read from the same designed team split as the behaviour, so friendly fire stays inside #1437's declared rules (`HostileOnly`) and an attacker's rounds resolve against the player and Betty. |
| Mounts | #1437's designed hardpoints (`gun_mount_1..3`, 0/−0.4/−4.0 m, ±3.2/−0.2/−0.4 m) | The AI's rig hangs on exactly the player's gun mounts, so no second set of muzzle positions is invented. |

A tick's weapon inputs are consumed exactly once, so an attacker never
publishes its own record: `append_ai_fire_orders` joins its intents to the
mission's own publication, under the AI's producer serial.

### Body and seat

* A flown scripted aircraft is a **dynamic** body whose declared mass is the
  imported weight over the law's force scale (`W / 9.82`), the mapping
  `docs/contracts/FLIGHT-PHYSICS.md` states for the player's body, with
  `NoAutoMass` bound so the collider can never replace it.
* Its initial velocity is **zero** — the spawn contract #1435 already had.
  The original's AI spawn speed is unrecovered (#796 lists the *player's*
  spawn speed as unknown; no AI figure was recovered either), so nothing is
  invented: the law's own 4.47 m/s AI floor gets it rolling.
* The record rides `PlaytestOriginalFlight { is_player: false, fuel: 0.0 }`.
  `is_player` selects the law's own AI branch (measured behaviour);
  `fuel: 0.0` is what `OriginalState::fuel` documents for AI records, whose
  chains state no fuel load as an explicit unknown. `engine()` therefore
  reads a non-player seat as running at its own throttle, because only a
  player seat can run dry.

### Acceptance bounds (also designed)

| Constant | Value | Why |
| --- | --- | --- |
| `RUN_SECONDS` | 45 | The four attackers spawn ~3.0 km from the player, both sides fly ~113 m/s-class airframes and the attackers start pointed within ~15° of the player, so the window allows for the turn-around and the stand-up acceleration from rest with margin. |
| `ENGAGEMENT_RANGE_M` | 1000 m | The window the pursuit law must reach; the *fire* window (`FIRE_RANGE_M`, 600 m) sits inside it, so an aircraft that reaches the engagement window can already shoot. |
| `MINIMUM_CLOSE_M` | 1500 m | The run must show real closure, not a token drift. |
| `WINGMAN_DISTANCE_M` | 250 m | Tighter than the ~270 m `wingman_2` spawns from `devastator_2`, looser than the 89 m station, so doing nothing fails and a converged or converging station passes. |

## 3. Unknown, and left unknown

* **The original AI control law, `0x48c220`.** Everything in §2 is designed
  against it. Gates: any fidelity claim about how the original's AI flies.
* **The ±5 % AI parameter roll** (`0x4771c6`, #805): not applied, so every AI
  airframe flies the exact retail numbers.
* **`bsfury_1`'s wake.** `WAKEUP_ENEMIES` names it, but the directive has no
  consumer in the runtime yet, so the record stays held at its measured pose
  and never flies. A wake consumer is follow-up work.
* **The gun trucks, camp and wreck.** They are world-container scenery with
  no `aiv.zrd` record (`docs/findings/2026-10-14-m01-play-actors-premise-and-census.md`),
  so "trucks warn, then fire" has no actor to attach a law to. Ground-actor
  measurement is F34-D.
* **The cargo zeppelin's route.** M01's `zeppelins.zrd` records declare
  `DeclaredMotion::Held` and state no route polyline, keyframe schedule or
  velocity (`docs/findings/2026-10-14-m01-play-actors-premise-and-census.md`
  and the F772 finding), so "flies its route" has no measured route to fly.
  Route measurement is F31-D/F35-D.
* **The rear gunner** the task names for the attackers: no mount for it is
  measured (F27), so this slice's fire is forward-facing only.

## 4. Where the tests live

* `crates/cs_app/src/mission_ai.rs` — `accept_m01_play_ai_*` unit tests over
  the pure law (role split, steering signs and clamps, station point,
  wingman throttle, fire window and cone). CI runs them: they need no
  installation.
* `crates/cs_app/tests/campaign/m01_play_ai.rs` — the retail acceptance
  tests, `#[ignore = "requires CS_GAME_DIR"]`, driven with
  `--include-ignored` locally and by the reviewer.
* `crates/cs_app/tests/evidence_report_m01_play_ai.rs` — the evidence
  harness for `docs/contracts/CLI-EVIDENCE.md`; not prefixed.

The two integration seams this slice makes in #1437's landed weapon module
are `attach_gun_rig` (the player's rig without the rocket launcher, hung on
an arbitrary actor entity) and the three-line hook in
`publish_mission_weapon_input` that calls `append_ai_fire_orders`. Both
reuse #1437's designed loadout, mounts and rules verbatim; neither changes
what the player's seat does.
