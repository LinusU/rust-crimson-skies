# M01-WINDOW-ACTORS (#1538): why no other actor was visible in the windowed M01

Date: 2026-10-17. Task #1538. Capabilities used: `retail`, `gpu`, `audio`
(owner's installation read-only, a real window on the implementer's machine;
no original run). Implementer: sonnet-2. Nothing here is `verified_original`.

## Why nothing was visible (measured by the census this task adds)

| Actor | Spawned? | Why it was invisible |
| --- | --- | --- |
| 9 `aiv.zrd` aircraft | yes (#1435) | The spawn made a physics body, a damage binding and a `Transform`, **no mesh**. |
| 3 `zeppelins.zrd` actors | yes (#772/#1435) | The binding entity carries no mesh, and the zeppelin's subtree is a node of the world container that is **not a world object**, so the world draw (#1433) never drew it. |
| Betty, Pandora, trucks, camp, wreck | see below | not records of the retail members |

Distance, fog and culling were not the cause: with the models attached, the
aircraft and a zeppelin are drawn at 1 km and 3.8 km in the same window.

## What this task changed

* `mission_session::actor_visual`: each scripted aircraft draws a `planes.zbd`
  model through the player airframe's production path. **Designed reading,
  claim `m01-window-actors.designed-model-from-kind-of-chain`:** the record's
  measured `kind_of` chain is searched from the record outward for the first
  element that is a `planes.zbd` root, or whose name is a display name of the
  original's airframe table (#715; row 5 `Devastator` -> model `piratefighter`).
  Result on M01: `devastator`, `rusdevastator`, `wingman` -> `piratefighter`,
  `bsfury` -> `fury`. A chain that names no root is refused by name, never
  given a substitute.
* `playtest_retail::build_world_node_visual`: draws a named world-container
  node's subtree (the area's own LOD/damage-alternative selection) relative to
  the node, so each zeppelin is parented under its binding entity.
* `mission_session::actor_census`: one terminal line per actor (name, model,
  position, distance from the player), the wingmen's distance each second for
  five seconds and every five seconds after, and a designed HUD aid: the
  nearest hostiles with distances plus a bracket over each hostile and wingman
  on screen. Hostile = measured `team` differs from the `player` record's (the
  AI split's reading), or a world actor whose roster is `Hostile`.

## Not done, with evidence

1. **Ground units and the bridge: no record exists.** M01's `zrdr.zbd` has no
   `targets.zrd` (see `2026-10-14-m01-play-actors-premise-and-census.md`);
   `aiv.zrd` names only aircraft. Nothing was spawned for them. Affected
   content: gun trucks, fuel truck, bridge, camp, wreck. Resolving task: a
   measurement of where the original places them (world-container geometry or
   engine code), not a guess.
2. **Betty is not beside the player at the start.** `aiv.zrd`'s `player`
   record stands at (-3694, 1318, -12482); `wingman_2` at (-3421, 1350,
   -13114) and `wingman_3` at (-4531, 1280, -13065), heading 180: 733 m and
   1037 m away, trailing, and the designed follow law does not close the gap
   in the first seconds (terminal: `mission wingmen t+1s..t+3s`). Spawning
   them beside the player would invent a pose. Which record is "Betty" is
   also unmeasured. Resolving task: the wingman formation law (`0x48c220`).
3. **The Pandora, Medusa Kestrels and Medusa zeppelin** have no distinct
   record; `piratezep`, `workersvoyagezep`, `blackswanzep` are the three
   zeppelin records and no label was invented.
4. Zeppelin parts drawn are the area's intact selection; turrets/engines
   alternatives and any animation of the parts are unmeasured here.

## Screenshots

Private evidence (not committed): `private/evidence/M01-WINDOW-ACTORS/`
`run5B` (an aircraft drawn ahead, hostile brackets), `run7C` (the
`piratezep` envelope ahead of the player). The capture script pitches the
player up for a second, so placements were set with `CS_MISSION_START_M` /
`CS_MISSION_START_HEADING_RAD` (heading `pi` flies +Z); those are test
placements, not the measured start.
