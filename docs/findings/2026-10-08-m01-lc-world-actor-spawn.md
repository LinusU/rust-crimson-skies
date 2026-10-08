# #772: the `world_actors` launch surface gets its measured spawn adapter —
# and stays `unsupported` on the fields no measurement has reached

Date: 2026-10-08. Task #772 `M01-LC-WORLD-ACTOR-SPAWN` ("Give a placed world
actor a measured spawn and route so the `world_actors` launch surface is
owned"), the second of the three follow-ups #359 (`VS-M01-RUNTIME`) filed
when it re-measured M01's launch closure on 2026-10-08. Feature sheet:
`specs/F34-*.md` (the declared world-actor schema/runtime stages) and
`docs/contracts/SCRIPT-MISSION.md`. Capabilities used: **`retail`**
(read-only access to `$CS_GAME_DIR`) and ordinary build/test. `gpu` and
`audio` were available and **not used**: nothing is rendered or played and
no original run happened, so nothing here is `verified_original`.

## The verdict first

**The measured spawn now flows through the production path, and the surface
honestly stays `unsupported`.** `crates/cs_app/src/mission_world_actors.rs`
(new) decodes a scope's `zeppelins.zrd` carrier (#574's measured grammar),
joins each record's `node` name to the world container's canonical scene
graph — the same conversion `world_geometry` runs — declares a
`DeclaredWorldActorProgram` with the record's stored `position` bound as
`position_m` metres (`ObservedTool`, claim
`f34-world.zeppelin-spawn-pose`), and hands the program to
`lower_world_actors` and `WorldActorSession::launch`. For M01 all three
records decode and all three join (`piratezep`, `workersvoyagezep`,
`blackswanzep`), and the production lowering then refuses on the first
unmeasured field, `orientation`, with the open-field list naming the rest:
`faction` and `orientation` per actor. The launch verdict is read off that
report rather than restated, so `world_actors` names exactly what is open —
nothing is invented to reach `Satisfied`.

## What was added

- `crates/cs_app/src/mission_world_actors.rs` (new): the spawn adapter.
  `bind_mission_world_actors(install_root, found, mission_dir, group_dir,
  mission_subject, session_ticks_per_second)` returns `MissionWorldActors`
  — carrier read state, scene join state, one `SpawnedZeppelinActor` row
  per decoded record (stored pose verbatim, `node` join outcome, declared
  `ProgramActor` when the join is unambiguous), the assembled program, the
  full open-field list, and the lowering/launch refusals verbatim. A record
  whose `node` names zero or several scene nodes is reported, never
  defaulted onto an invented identity.
- `crates/cs_app/src/mission_launch.rs` (ported from #359's blocked branch
  and extended): `measure_actor_readers` now derives `world_actors` from
  `bind_mission_world_actors` — `Satisfied` only when a session launches,
  `Unsupported` naming the adapter's rows, open fields and refusals
  otherwise, `Unknown` when the carrier cannot be read.
- `DeclaredWorldActorKind::Airship` and `WorldActorKind::Airship`
  (`crates/cs_content/src/world_actors.rs`,
  `crates/cs_sim/src/world_actors/runtime.rs`, the lowering match in
  `crates/cs_app/src/world_actors.rs`): the kind domain had no arm a
  zeppelin record could honestly take — rail, road, water and kinematic all
  assert something about the actor's drive the carrier does not state.
- `UnmeasuredFieldFamily` reasons for `PlacementRecords` and `StoredUnit`
  in `crates/cs_app/src/animation/programs.rs` re-worded: #574 decoded the
  placement encoding and #436/#677 measured the stored unit, so both
  reasons now say the grammar/unit are measured and name what is actually
  still open (field meanings, not encodings).
- The launch substrate this surface reports through —
  `crates/cs_app/src/mission_launch.rs`, `crates/cs_app/src/cli.rs`'s
  `MissionRequest`, `main.rs` dispatch, `tests/campaign/vs_m01_runtime.rs`
  — was written under #359 on its blocked branch and never merged; this
  task's acceptance criterion names it, so it is ported here and adapted to
  current main (`player_configuration` is now `Satisfied` through #715's
  `MissionStartConfiguration`, which the ported `measure_player` already
  reads — the retail closure test's expected gap set shrank accordingly).

## The measured claim, and exactly where it stops

- **Spawn position: measured, `ObservedTool`, claim
  `f34-world.zeppelin-spawn-pose`.** The owner-supplied decrypted image
  (sha256 `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`,
  image base `0x400000`, #770's conventions) reads the record's named
  `position` (three floats) into the spawned object's `obj+0x20`..`obj+0x28`
  at `0x4bda64`..`0x4bdac1`. The stored unit is the metre with the identity
  axis map (#436's owner note, #677's census), so `position_m` binds the
  stored triple widened to `f64`, unscaled. Corroboration, not the claim:
  M01's three records sit inside `c1c`'s measured node extent and
  `piratezep` lands ~517 m from the measured player start — encounter
  placement. `placezeps.zrd`'s startup placements state *different*
  positions for the same nodes (`piratezep` z = −5888 vs −11985); which
  mechanism the original applies last is unmeasured, and the adapter claims
  the carrier's value, not a blend.
- **Spawn attitude: unmeasured, `Resolved::Unknown`, claim
  `f34-world.zeppelin-attitude-unmeasured`.** `yaw`/`pitch` are degrees →
  radians into `obj+0x2c`/`obj+0x30` (measured at `0x4bda64`), but the
  composition of those slots into a world orientation was never traced for
  the zeppelin object family — the `M = Ry·Rx·Rz` compose #770 measured is
  `Object3d::SetRotation`'s `class+0x1c` layout, a different object. The
  gamez world records store identity rotations for these nodes,
  `zeppelins.zrd` states `yaw 220` for `workersvoyagezep`, and
  `placezeps.zrd` states `180` — three sources, no measured winner, so no
  quaternion is guessed.
- **Faction: unmeasured, `Resolved::Unknown`, claim
  `f34-world.zeppelin-faction-unmeasured`.** `team` spellings `ally`/`enemy`
  are #574's measured vocabulary; nothing maps a spelling to a faction
  `ContentId` — no faction catalog joins it and `net.zrd`, the plausible
  carrier of a net→faction table, has no decoder.
- **Motion: `Held` by shape, not by claim.** The carrier states a spawn
  pose and tuning floats (`max_speed`, `max_accel`, `accel_pitch`,
  `accel_yaw`, `max_rate_*`, `min_pitch`, `max_pitch`) — never a route
  polyline, keyframe schedule, velocity or carrier. `Held` is the only
  `DeclaredMotion` variant the record's own fields can fill; whether the
  original then drives the actor (`targets`, `deactivated`, the undecoded
  mission program) is outside this program's authority and named in
  `MOTION_RESIDUE`. The tuning floats are deliberately **not** read as a
  route: a speed limit is not a path.
- **Session cadence: designed, claim `f34-world.session-rate-designed`.**
  64 Hz is the host's stepping rate (the same designed value
  `MissionAnimationPlayer` is handed), not an original measurement.
- **Sockets, pickups, support edges, gate transitions:** the carrier has no
  fields for them, so the program declares none rather than inventing
  attachments.
- **`placezeps.zrd` stays refused** under
  `f20-anim.placement-member-fields-undecoded` (#718's spawn-path refusal),
  and the verdict says so with the scope's placement count — the two
  carriers disagree on values and are not interchangeable.

## What M01's run actually reports

`plan_mission_launch` over the retail installation: the carrier decodes 3
records; 3 declare actors (`piratezep`, `workersvoyagezep`,
`blackswanzep`); the lowering refuses `orientation` under
`f34-world.zeppelin-attitude-unmeasured`; the open list names
`faction`+`orientation` for `actor(0..2)`; the surface is `unsupported:
world-actor spawn semantics`. `blackswanzep` carries `deactivated=1` and
sits ~4.7 km outside the partition extent — the record is still declared
(its field's meaning is `KeyMeaning::Unknown`, so no behavior is inferred
from it) and reported with its stored value.

## Residues and the tasks that can lift them

- `f34-world.zeppelin-attitude-unmeasured` — trace the zeppelin object's
  write of `obj+0x2c`/`obj+0x30` into its world matrix (a `#358`-class
  image read, or an original run).
- `f34-world.zeppelin-faction-unmeasured` — decode `net.zrd` or the
  campaign faction catalog, whichever the records join.
- The `placezeps.zrd`/`zeppelins.zrd` disagreement — which carrier's pose
  the original's spawn lands on, and what `deactivated`/`targets` gate.
- Route/motion — no carrier measured states one; if the original drives
  these actors it does so through the undecoded mission program, not this
  member.
