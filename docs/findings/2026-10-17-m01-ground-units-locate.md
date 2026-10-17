# M01-GROUND-UNITS-LOCATE (#1549): where the bridge, gun trucks, fuel truck, camp and wreck are

Date: 2026-10-17. Task #1549. Capability used: `retail` (read-only). Method:
the production readers (`discover_container`, `read_gamez_nodes`) over the
mission reader archives and world containers, scratch probes only (nothing
committed, outputs under `private/`). The engine image was not needed: the
answer is in the retail data. Nothing here is `verified_original`.

## Answer in one paragraph

The units #1538 could not find **are not missing from the game; they belong to
`ZBD/C3/M01`, not to `ZBD/C1C/M01`.** `ZBD/C3/M01/zrdr.zbd` has the
`targets.zrd` that C1C/M01 lacks, and `ZBD/C3/gamez.zbd` places the bridge, the
three gun trucks, the bridge truck (with a `tank`), the camps and the wreck as
ordinary world-container nodes. C1C/M01's reader archive and `ZBD/C1C/gamez.zbd`
hold none of them (measured below). The project's M01 binding
(`missions/bindings/M01.json`: `world/c1c`, `script/c1c-m01-zrdr`) chose C1C by
directory name; the data says that directory is a different mission. That is an
owner-visible premise problem, filed as a follow-up task, **not** fixed here.

## Measurements

### C1C/M01 holds no such unit

* Its reader archive has 12 members (`aiv.zrd`, `egen.zrd`, `location.zrd`,
  `map.zrd`, `mis_anim.zrd`, `net.zrd`, `objectives.zrd`, `startanims.zrd`,
  `weather.zrd`, `zeppelins.zrd`, `placezeps.zrd`, `wv_tailhook.zrd`). A
  case-insensitive search of every member for `truck|bridge|camp|wreck|teresa`
  finds nothing.
* `ZBD/C1C/gamez.zbd` (5 644 nodes): no node named for a truck, bridge, camp,
  wreck or tent. Its scene content is the Workers Voyage / Pirate / Black Swan
  zeppelin families and effects.
* Its `objectives.zrd` sound groups are `snd_c2-NW-m1_*` and its targets are
  `piratezep`, `workersvoyagezep`, engines (`reng*`, `leng*`).

### C3/M01 holds all of them

`ZBD/C3/M01/zrdr.zbd` has 13 members, including `targets.zrd`, `dzones.zrd` and
`tex_drop.zrd`.

| Wanted | Node in `ZBD/C3/gamez.zbd` (index) | Local translation (m, source axes) | Named by |
| --- | --- | --- | --- |
| Bridge | `susp_bridge` (3021), parent world, 14 children: `rope1`, `rope2`, `part1`..`part5`, `dbase`, three trucks, `bridge_truck01` | identity at the bridge root; children carry the world positions | `susp_bridge.zrd` (cam_anim reference); objective 7 waits on animation `rope1burn` |
| Three gun trucks | `t_truck01` (3029), `t_truck02` (3040), `t_truck03` (3051), children of `susp_bridge` | (-5459.3, 118.6, -5339.2), (-5461.5, 118.6, -5375.9), (-5461.4, 118.6, -5391.8) | `targets.zrd` (`MSG_OBJ_BRITCONVOY`, `help_label MSG_OBJ_DESTROY`), `objectives.zrd` objective 5 `ADD_OBJECTIVE_TARGET`, objective 6 `WAKEUP_TURRETS t_truck**`, `aiv.zrd` |
| Fuel truck | `bridge_truck01` (3069), child of `susp_bridge`, children `shadow`, `chasis`, `tank`, `flashlight` | (-5458.1, 118.4, -5366.4) | `targets.zrd` (fourth `MSG_OBJ_BRITCONVOY`/`MSG_OBJ_DESTROY` row); animation `bridge_destroy01` in `ZBD/C3/zrdr.zbd::fueltruck.zrd` |
| Camp | `u_camp1`..`u_camp3` (under 2318), `u_camp5`..`u_camp8` (under `town_buildings`, 2165); each with 3 children | e.g. `u_camp5` (-12654.6, 189.4, -10390.4); `u_camp1` (-12695.8, 273.8, -9871.6) | animation `uppercamp_dest*` over `u_camp*` in `ZBD/C3/zrdr.zbd::ucamp_dest.zrd` |
| Wreck | `shipwreck` (3007), parent world, 10 children (`hull`, `hull1`..`hull3`, `masts`, `railings`, `stringy1`..`4`) | (-8891.4, 5.0, -4685.5) | `targets.zrd` (`MSG_OBJ_SITE3`, `MSG_OBJ_EXAMINE`), objective 12 `TRAVELERS player APPROACHING shipwreck`, animation `shipwreck_destroy` in `shipwreck.zrd` |
| Betty, Medusa Kestrels | `aiv.zrd` records `Betty` (`wingman_1`) and `medkestrel_1`..`_3` | (not measured here) | `aiv.zrd` of C3/M01 |

Translations are the node's own stored `translation`, read with the production
reader. The ancestors' composed transforms were **not** applied (the bridge
root stores an identity transform and a parent of index 0, the world), so the
bridge-truck figures are world positions only as far as that holds; the camp
rows sit under `town_buildings`/another parent whose transform was not
composed. Treat every camp figure as local until a task composes the chain.

### What makes them damageable targets (structural reading, not proven)

* Each gun truck node has a `healthy` child (with `turret` -> `gun` ->
  `firepoint`, and the `stude_truck` body mesh) and a `destroyed` child (three
  parts); the node's `zone_id` is 1, as for the bridge. This is the same
  healthy/destroyed layout the zeppelin parts use.
* The animation programs carry `HEALTH` (`fueltruck.zrd`, `ucamp_dest.zrd`),
  `PROXIMITY_DAMAGE` (camp) and a `DAMAGE_SEQUENCE` with `ANIM_HEALTH`
  branches; `*_destroy*` animations swap `healthy`/`destroyed` active states and
  call effects (`large_fireball AT_NODE bridge_truck01`).
* The mission program wakes the turrets (`WAKEUP_TURRETS t_truck**`, objective
  6) and lists the nodes as objective targets.

How `HEALTH` values, damage rules and the turret fire law are resolved is
**unmeasured here** (it is F20/F34 animation-program semantics, see
`2026-10-04-m01-lc-world-actors.md`'s unmeasured families).

### The wiki list and the installation agree

The game's own in-game news text (`GOSDATA/ASSETS/BINARIES/langui.dll`, string
ids not recorded here) describes a convoy of three British trucks hit while
crossing a suspension bridge. That is the `t_truck01`..`03` / `susp_bridge`
set above. It is consistent with the fan-wiki list quoted by #1435 and with the
walkthrough cited in `2026-10-17-m01-window-pose-record.md`.

### The chapter directory is not the chapter number

Sound-group prefixes in each archive's `objectives.zrd` carry their own chapter
and area code:

| Directory | Prefix | Directory | Prefix |
| --- | --- | --- | --- |
| `C3/M01..M04` | `snd_c1-HA-m1..m4` | `C1C/M01`, `C1/M02` | `snd_c2-NW-m1`, `snd_c2-NW-m2` |
| `C2/M01..M03`, `C2B/M04` | `snd_c3-HW-m*` | `C4/M01..M04` | `snd_c4-RM-m*` |
| `C5/M01` | `snd_c5-MN-m1` | | |

So `C3` holds the sound groups of chapter **1** (`c1-HA`) and `C1C` those of
chapter **2** (`c2-NW`). `2026-09-29-m01-a-source-binding.md` bound M01 as
"the only chapter-1 group holding `M01`: `ZBD/C1C/M01/`" by reading the
directory name as the chapter. The prefix, the in-game news text and the
walkthrough ("The Lost Treasure": a chamber, a grass hut, the convoy on the
bridge, the wreck) all point at `ZBD/C3/M01` as the first mission.
Caveat: the prefix is a naming convention, not a decoded campaign table; the
campaign order is read by the title-block join, which gives a position and not
a directory. No original run was made.

## Unknown, with evidence

1. **Which archive is the mission titled "The Lost Treasure".** Strong
   evidence says `ZBD/C3/M01`; the binding says `ZBD/C1C/M01`. The decision
   changes `missions/bindings/M01.json`, every M01 task that read C1C/M01 and
   `missions/M01.md`'s owner-protected text, so it is the owner's.
2. **"Teresa".** No node, record or string of `ZBD/C3`, `C3/M01` or
   `C1C` is named `teresa` (case-insensitive, ASCII and UTF-16 `langui.dll`).
   The only wreck in C3/M01 is `shipwreck` (an objective site to examine,
   with a `shipwreck_destroy` animation). Which name the walkthrough's "wreck
   Teresa" corresponds to is not measured.
3. Composed world positions of the camps and the bridge children (see above),
   the health values and the damage law of the trucks/camps, the turret fire
   law, and whether `fuel_truck01` (node 2466 in `ZBD/C3/gamez.zbd`, a separate
   fuel truck near the lower camp, ~(-12320, 8, -10456)) is part of M01 or only
   of the shared world (`ZBD/C3/M02` also references `fuel_truck0*`).

## Follow-ups

Filed through Rally: (a) the owner decision / re-binding on the M01 archive
(blocks everything below if the answer is C3); (b) a spawn task for the C3/M01
bridge convoy, if (a) says C3: world nodes already exist in the world
residency, so the work is the mission-program wiring, targets and turret wake,
not placement.
