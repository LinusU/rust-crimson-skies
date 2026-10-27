# M02-T2: a retail world group holds several campaign missions

Task #449. A world id (`world/c1`) is a world-**group** identity, not a mission
identity.

## Measured (retail, read at run time)

`SourceContext::campaign()` derives one entry per `ZBD/<group>/<mission>`
directory. The shared groups, as `accept_m02_t2_retail_world_group_names_its_missions`
prints them:

| World id | Missions (`mission_id`) |
| --- | --- |
| `world/c1` | `ch2-m02`, `ch2-m04`, `ch2-m05` |
| `world/c2` | `ch3-m01`, `ch3-m02`, `ch3-m03`, `ch3-m05` |
| `world/c3` | `ch1-m01` .. `ch1-m05` |
| `world/c4` | `ch4-m01` .. `ch4-m05` |
| `world/c5` | `ch5-m01` .. `ch5-m04` |

`mission_id` and `program_id` are unique per mission inside every group.

## Lookup added

`cs_content::campaign_bindings::missions_in_world_group` (also
`SourceContext::missions_in_world_group`) answers "which retail missions live in
this world group", returning each mission with its `mission_id` and `program_id`.
More than one entry means the world id must not select mission content.

## Audit of places that key by world id

* `cs_app::mission_control::campaign_mission_ids` keys by the reader archive's
  logical container key, not by world id: correct.
* `catalog::baseline` world rows are one per group by design (F14-D.3); mission
  rows carry their own mission id: correct.
* F18 `WorldId`/`WorldInstance` identify a world group load; they are not used as
  a mission key in the code reviewed. Whether a mission's overlay (F18-C) is
  selected by `MissionOverlay` rather than by world id is not verified here.
  Unknown: affected content is every mission in the shared groups above; resolving
  work is whichever task first selects mission-local content from a `WorldId`.
