# M01-TARGET-ROSTER (#1592): M01's bound ground targets in the F30 targeting store

Date: 2026-10-18. Follow-up to `M01-BRIDGE-CONVOY-SPAWN` (#1566,
`2026-10-18-m01-bridge-convoy-spawn.md`). Capabilities used: **`retail`**
(read-only `$CS_GAME_DIR`) and ordinary build/test. No original executable ran:
nothing here is `verified_original`. Test prefix: `accept_m01_target_roster_`.

## The answer

A world-container node *does* have a measured allegiance, though no record
states one. `targets.zrd` rows and world records carry no `team`, but the
loader's node resolver (`0x4bf080` → `0x4bef90`, measured by #1155 and #1177
for `zeppelins.zrd`) answers a team int for **any** node's subtree: a staged
marked-node lane (`unk040` bits 30..31), then an `ai.zrd` `TURRET` binding
(`TEAM`, default 2), then the children depth-first, else `0`. The int's
spelling is the loader's own: `0` neutral, `1` ally, `2` enemy.
`mission_world_actors::resolve_world_node_allegiance` runs that same walk over
named nodes, and `mission_targets::BoundTarget::allegiance` carries its answer.

Over the retail installation (`zbd/c3/m01`, group `zbd/c3`):

| node(s) | team | source |
| --- | --- | --- |
| `t_truck01`, `t_truck02`, `t_truck03` | 2 (`enemy`) | `ai.zrd` `TURRET` record 20 binds the node itself |
| `bridge_truck01` | 2 (`enemy`) | marked node `tank` in its subtree |
| `susp_bridge` | 2 (`enemy`) | the walk reaches `t_truck01`'s turret binding in its subtree |
| `u_camp1..3`, `unit01..17`, `g_tower1..7` | 2 (`enemy`) | marked `healthy` child |
| `shipwreck`, `t_chamber`, `grasshut2`, `u_camp5..8`, `unit31..42`, `hydrogentank1..4`, `trucker1..5`, `craterlake`, `leasure1..5`, `airdock2`, `pzhookpoint`, `dz4` | 0 (`neutral`) | nothing binds: the resolver's terminal zero |
| `rock_zeppelin` | open | the name joins 5 container nodes, so there is no single subtree |

## What changed

* `mission_world_actors::resolve_world_node_allegiance` (new, public).
* `BoundTarget::allegiance`; `bind_mission_targets` fills it.
* `MissionGroundTarget::{roster, allegiance_open}` decided at spawn;
  `roster_mission_ground_targets` (on the fixed tick beside the zeppelin
  roster) registers each rostered target as a `TargetClass::WorldObject`
  record under its faction, with the `objective` flag of its declaring row, and
  declares the player↔faction relation. Its own report is
  `MissionGroundTargetRosterReport`; the zeppelin report is unchanged.

## What is designed, and what stays open

* **Designed:** the *relation*. It reuses #1492's reading
  (`declared_player_allegiance`, claim
  `m01-play-parts-geometry.designed-player-allegiance`): the player's own team
  is friendly, another measured team is opposed. One deliberate deviation: a
  `neutral` node is rostered with **no** relation, because nothing measured
  says the player is opposed to a neutral node.
* **Unmeasured:** that the original *asks* the resolver about a ground target
  at all. The resolver is measured as the zeppelin loader's call; its use for
  `targets.zrd` nodes is inferred from it being the engine's one subtree
  allegiance walk, not observed. **Affected content:** whether the original's
  reticle names a convoy truck as an enemy. **Resolving task:** an
  original-run reference capture of the M01 reticle over a truck (owner).
* **`susp_bridge` reads `enemy` through its child truck's binding.** That is
  what the walk returns; whether the bridge is a hostile target is not
  claimed beyond that.
* **Open nodes are not rostered** and are named in
  `MissionGroundTargetRosterReport::unresolved`.
