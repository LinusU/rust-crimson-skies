# #1435 M01-PLAY-ACTORS: the task's actor list does not match M01's retail members

Date: 2026-10-14. Task: #1435 (`M01-PLAY-ACTORS`). Capability used: `retail`
(read-only). Nothing was run from the original; nothing here is
`verified_original`.

## Measured: what M01's `ZBD/C1C/M01/zrdr.zbd` declares

Read through `discover_container` and `decode_zrd` (a scratch probe, not
committed). The reader has **11 members**: `aiv.zrd`, `egen.zrd`,
`location.zrd`, `map.zrd`, `mis_anim.zrd`, `net.zrd`, `objectives.zrd`,
`startanims.zrd`, `weather.zrd`, `zeppelins.zrd`, `placezeps.zrd` and
`wv_tailhook.zrd`.

* There is **no `targets.zrd`** member. The task text names it.
* `aiv.zrd` has 12 records of 81 fields each: `player`, `wingman_2`,
  `wingman_3`, `bsfury_1`, `bsfury_2`, `devastator_2`, `devastator_3`,
  `rusdevastator_1`..`rusdevastator_5`. All are aircraft records. There is
  no record named for a gun truck, fuel truck, Kestrel, camp or wreck.
* `zeppelins.zrd` already spawns three capital ships (`piratezep`,
  `workersvoyagezep`, `blackswanzep`) through `bind_mission_world_actors`
  (#772) and the mission host's `WorldActorSession`. No "Medusa cargo
  zeppelin" exists as a record.

## Why the task cannot be done as written

1. The task's list (Pandora, Betty Charles, bridge gun trucks, a fuel truck,
   three Medusa Kestrels, a Medusa cargo zeppelin) comes from a fan wiki
   summary. The retail members above do not corroborate it. Mapping
   `bsfury_*`, `devastator_*` or `rusdevastator_*` to "Kestrels", or any
   zeppelin to "Pandora", would be a guess (AGENTS.md rule 4).
2. The aircraft records cannot be spawned faithfully yet. #715 and
   `mission_start.rs` record that no `aiv.zrd` field names an airframe
   (`AIRFRAME_UNKNOWN_REASON`), and the other ~80 positional fields are
   unmeasured. A spawn needs an airframe, a pose and a faction.
3. The static ground scenery (trucks, camp, wreck) is world-container
   geometry (`ZBD/C1C/gamez.zbd`), already placed by the world residency.
   The `fuel_truck0*` selector matches no `ZBD/C1C` node (census in
   `2026-10-04-m01-lc-world-actors.md`), so a "fuel truck on a bridge" is
   not evidenced in M01's world either.
4. "Scripted actors spawn when the program wakes them": the measured wake
   path (`WAKEUP_ENEMIES`, `WAKEUP_GENERATOR`, see
   `2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives.md`)
   acts on named vehicles that are asleep. It needs the aircraft records'
   unmeasured fields to be driven.

## What is needed

* A measurement task for the `aiv.zrd` aircraft record layout (airframe,
  pose, faction, sleep state), with evidence from `$CS_ENGINE_IMAGE`.
* An owner decision on the task text: either re-scope the acceptance to M01's
  measured actor set (3 zeppelins, 10 AI aircraft records, the player and
  two wingmates), or provide evidence for the wiki's actors.

The follow-up tasks M01-PLAY-AI (#1436) and M01-PLAY-TRIGGERS (#1439) depend
on this one and carry the same premise.
