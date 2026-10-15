# #1435 M01-PLAY-ACTORS: the spawned actor set and its designed values

Task: Rally #1435 (`M01-PLAY-ACTORS`), 2026-10-15.
Implementer: swe2-max-1. Branch: `rally/1435-spawn-m01-s-actors-in-the-mission-pandor`.
Evidence: `docs/findings/evidence/M01-PLAY-ACTORS.json`
(`retail` capability, `implemented` claim — nothing here is
`verified_original`).

This note is the list the owner asked for: every value the spawn path uses
that is *designed* rather than measured, and what would replace it. The
premise census is `2026-10-14-m01-play-actors-premise-and-census.md`; the
`aiv.zrd` field measurements are #1453's (`2026-10-15-m01-aiv-layout.md`).

## The measured set

M01's `zrdr.zbd` `aiv.zrd` member carries 12 aircraft records. The
composition (`cs_app::mission_actors`) spawns the nine the original's spawn
loop keeps:

- Skipped exactly as measured: `player` dispatches to the engine-state path
  (the composition's own `MissionPlayerBody`), and `bsfury_2` /
  `rusdevastator_3` whose `enabled` field is `0` (`0x474d10`).
- Spawned: `wingman_2`, `wingman_3`, `bsfury_1`, `devastator_2`,
  `devastator_3`, `rusdevastator_1`, `rusdevastator_2`, `rusdevastator_4`,
  `rusdevastator_5` — each at its stored pose (#436 position scale,
  `stored_heading_radians` + `object3d_orientation` heading compose, the
  same conversion the player's spawn uses).
- `bsfury_1`'s measured `deactivated` field is non-zero, so the entity's
  binding carries `asleep` (#681's sleep path); the wake is the mission
  program's `WAKEUP_ENEMIES`, which the program drives when it runs.
- `zeppelins.zrd`'s three world actors (`piratezep`, `workersvoyagezep`,
  `blackswanzep`) are the `WorldActorSession`'s launched actors (#772); the
  composition binds each an ECS entity (`WorldActorBinding` +
  `MissionZeppelinActor`) at the session's canonical pose — a declared actor
  the session holds no pose for is a recorded refusal, never a fallback.
  Trucks, camp and wreck stay world-container geometry (the census found no
  records for them); no "Medusa cargo zeppelin" or "Pandora" label is
  invented.

## The designed values and what replaces them

1. **Airframe/model label** — the entity carries the record's measured
   `SpawnDispatch::VehicleRecord` name-truncated prefix verbatim
   (`devastator`, `rusdevastator`, `bsfury`, `wingman`). #1453 measured that
   the original resolves the prefix against `vehicle.zrd`; *which* record
   each prefix binds and what model it draws is unmeasured, so no airframe
   or model is claimed. Replaced by a measured `vehicle.zrd`
   record-to-model step (the follow-up #1453's note anticipates).
2. **Damage graph** — each actor registers a designed single-node graph
   (`airframe`, internal structure, lethal) whose integrity is
   `Resolved::Unknown` under claim `m01-actors.damage-pool`: no M01 damage
   table is measured (#668), so a routed hit blocks by name instead of
   draining an invented pool. Replaced by M01's measured damage tables.
3. **Kinematic hold** — scripted aircraft are `BodyMode::Kinematic` bodies
   held at their spawn pose: they collide and can be damaged, but nothing
   flies them. Replaced by #1436's AI/flight behaviour (and by the wake
   directives for the sleeper).
4. **Collider extents** — the playtest's declared development box
   (`AIRCRAFT_HALF_EXTENTS_M`); no scripted-aircraft collider extent is
   measured. Replaced by measured extents.
5. **Mass** — `mass_kg: 0.0`: a kinematic body integrates nothing; a later
   dynamic release must declare the measured mass rather than inherit a
   collider-density default.
6. **Damage producer serial** — `MISSION_DAMAGE_PRODUCER = 0`: the session
   has no measured producer table, so the spawn path owns this one.
7. **`enabled` unmeasured** — a record whose `enabled` field is absent still
   spawns (the measured skip is `== 0`, not "missing"). All of M01's records
   carry the field, so no M01 record takes this path.
8. **`starts_asleep` unmeasured** — treated as awake
   (`unwrap_or(false)`); all of M01's records carry the field.

Every other value on the spawned entities is `ObservedTool`-provenanced
retail data: the record verbatim, its pose, heading, team, group, enabled
flag, primary target, title and sleep state.
