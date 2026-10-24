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
`PartSweptBox` with the node's stored half extents, the old actors unknown to the new
resolver, no damage to any truck over 180 idle frames, and a truck destructible
again.

## 3. Zeppelin-container ground targets and overlaps

M01 binds one of the four zeppelin-container names the task listed:
`rock_zeppelin` (container origin, target point `[0, 4.5, 62.9]`). `workersvoyagezep`,
`blackswanzep` and `hooks` are not in M01's bound-target list (`pzhookpoint` is a
separate node). With the node boxes of #2048 its box (centre at the target point) overlaps the
`pzhookpoint` part box by about 20 m.

The four convoy trucks each overlap `susp_bridge`'s box (8.6-9.5 m deep at the least
axis) because the trucks sit on the bridge and its stored box is 17 x 97 x 563 m. Gun
hits are one per distinct actor (`Ballistics::sweep`), so a round through the
overlap can damage both the truck and the bridge. The test pins exactly these five
pairs (four bridge pairs, `rock_zeppelin` x `pzhookpoint`), so a changed box shows
up. Whether the engine resolves only the first actor hit is unproven here.
The trucker rows in the world (`trucker1..4`, 4-11 m apart) have overlapping
boxes too; they are not M01 convoy trucks and are untouched here.

## 4. Measured extents

Done by #2048 (`docs/findings/2026-10-24-node-box-unk164.md`): the ground target boxes
take the node's stored box; the designed `[5, 3, 5]` only backs a node that stores none.

## Owner smoke

Item 3 was done by #2047: `docs/findings/2026-10-24-convoy-kill-owner-command-smoke.md`.
