# M01 convoy truck destruction (#2017): objective response, restart, box overlap

Date: 2026-10-24. Task: #2017, follow-up to #1998 (`BUG-M01-CONVOY-GUN-HITS`).
Capability used: `retail` (read-only `$CS_GAME_DIR`). Nothing here ran the original
executable; nothing is `verified_original`. Tests: `accept_m01_convoy_destruction_`
in `crates/cs_app/tests/campaign/m01_convoy_destruction.rs` (all `#[ignore]`, need
`CS_GAME_DIR`).

## 1. What the objective does when a truck dies

The lowered M01 program (39 objective blocks) reads four operand members:
`piratezep`, `player`, `shipwreck`, `t_chamber` (`WorldOperands::of`). **No block
counts the trucks.** `t_truck01..03` and `bridge_truck01` are objective *targets*:
OBJECTIVE1 sets them (`SetTargetFlag objective=true`, `ADD_OBJECTIVE_TARGET`),
OBJECTIVE7 clears them when the `rope1burn` animation runs. The convoy block
completes on that animation, not on a kill count, and `WAKEUP_TURRETS t_truck**`
(OBJECTIVE6) wakes the trucks' turrets.

So the only consumer of a truck's destruction in the original's data is the member
observation the `INACTIVE` ladders read. The test pins that: every truck's row is
`held_in_play` before the shot, `no_longer_in_play` after the damage authority
destroys it, and the other trucks' rows do not move. It also asserts the program
reads no truck chain, so if a later decode finds a consumer the test fails and this
finding must be rewritten. Whether the original ends or changes the objective HUD
line when the last truck dies is **unknown**; no data seen here says so.

## 2. Restart / generation

After shooting down `t_truck01` and `t_truck02`, with a gun burst in flight at
`t_truck03`, a composed restart (`MissionHostRestartRequest`) gives: new actors on a
new session for all four trucks, none destroyed, full designed integrity, a fresh
`PartSweptBox` with the designed half extents, the old actors unknown to the new
resolver, no damage to any truck over 180 idle frames, and a truck destructible
again.

## 3. Zeppelin-container ground targets and overlaps

M01 binds one of the four zeppelin-container names the task listed:
`rock_zeppelin` (container origin, target point `[0, 4.5, 62.9]`). `workersvoyagezep`,
`blackswanzep` and `hooks` are not in M01's bound-target list (`pzhookpoint` is a
separate node). Its designed box overlaps no other swept box (the two zeppelin
actors sit ~1.4 km and ~7 km away).

The only overlap of the four trucks and `rock_zeppelin` with any other box is
`susp_bridge`'s designed box against `t_truck03`'s: about 1.1 m deep in z. Gun hits
are one per distinct actor (`Ballistics::sweep`), so a round through that sliver can
damage both the truck and the bridge. The test bounds the depth below 1.5 m.
The trucker rows in the world (`trucker1..4`, 4-11 m apart) have overlapping
5 m-half-extent boxes too; they are not M01 convoy trucks and are untouched here.

## 4. Measured extents: evidence for the follow-up

The container node field `unk164` (min, max corners) looks like the node's local
bounding box, not proven:

| node | `unk164` min | `unk164` max |
| --- | --- | --- |
| `t_truck01..03` | `[-1.48, -0.17, -2.79]` | `[1.48, 4.45, 4.41]` |
| `bridge_truck01` | `[-1.92, 0, -13.6]` | `[1.92, 5.87, 3.97]` |
| `susp_bridge` | `[-5468.6, 72.1, -5683.0]` | `[-5451.4, 169.5, -5119.9]` |

The truck box is a plausible truck (3 m wide, 7 m long); `susp_bridge` is already in
world coordinates and matches its target point. Neither the field's meaning nor its
frame is decoded, so `GROUND_TARGET_BOX_CLAIM`'s designed `[5, 3, 5]` stays. Item 5
of #2017 remains open.

## Not done here

The owner-command smoke with framebuffer screenshots of a destroyed truck needs a
new smoke mode beside `--smoke-trucks`; it is a separate slice.
