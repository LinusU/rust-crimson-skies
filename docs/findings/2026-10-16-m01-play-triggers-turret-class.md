# M01-PLAY-TRIGGERS-TURRET-CLASS (#1491): what M01's `workersvoyagezep` turret nodes are

Date: 2026-10-16. Task `M01-PLAY-TRIGGERS-TURRET-CLASS` (Rally #1491), follow-up
of `M01-PLAY-TRIGGERS-PARTS` (#1481). Capability used: `retail` (read-only
`$CS_GAME_DIR`) plus build/test. **`CS_ENGINE_IMAGE` was not set**, so nothing was
traced in the engine image. Implementer: claude-2/claude-1. No reviewer has run
yet and no original executable ran; nothing here is `verified_original`.

## The question

Nine of the 27 `workersvoyagezep` part-state chains the `INACTIVE` ladders
`OBJECTIVE7`/`OBJECTIVE28` count name nodes `zeppelins.zrd` does not declare:
`utur1`…`utur4`, `ctur1`…`ctur4`, `maingun9`. #1481 gave each an
`InternalStructure` node with no `disables` edge because a class read from the
name would have been a guess.

## Measurement 1 — a record table does declare them

The shared `ai.zrd` (`zbd/zrdr.zbd`, the member #1155 already reads for
allegiance) holds a `TURRET` list of 42 records. Records 0–15 are standalone
turrets (`CREATE_STANDALONE`, `HEALTHY_NODE`); the 26 others state `NODES`
entries, each a `[mount, part]` pattern chain (`*` = exactly one digit, the rule
#1155 measured at `0x4abea0`). Three of them name `workersvoyagezep`:

| Record | `NODES` entry | `TITLE` | `WEAPON` | `HEALTH` | matches |
| --- | --- | --- | --- | --- | --- |
| 27 | `[workersvoyagezep, maingun*]` | `MSG_TUR_MAIN` | `wep_27` | **absent** | `maingun9` |
| 28 | `[workersvoyagezep, ctur*]` | `MSG_TUR_BELLY` | `wep_29` | `10.0` | `ctur1`…`ctur4` |
| 31 | `[workersvoyagezep, utur*]` | `MSG_TUR_TOP` | `wep_29` | `10.0` | `utur1`…`utur4` |

All three carry `PARTS = [turret, gun, firepoint]` (each resolves under the node
in the C1C hierarchy, as does `healthy`), an `AMMO`, a `FIRE_RATE`, a
`DETECTION_RANGE`, `PITCH`/`YAW` limits and attack/bored timers. That is a
**weapon mount**: the nine nodes are turrets the AI drives, not structure.
None of the 30 engine chains matches any `NODES` entry of the table.

Claim ids (provenance `static`, the decoded `ai.zrd` member read through
`cs_formats::script_raw::discovery` + `cs_content::stunts::decode_zrd`):
`m01-turret-class.node-declared-by-ai-turret-table` (the nine rows above),
`m01-turret-class.node-is-weapon-mount` (`WEAPON` + `PARTS`),
`m01-turret-class.health-stated` (28 and 31 only).

## Measurement 2 — which system their destruction disables: not a per-actor edge

`SystemKind::Weapon` in `cs_sim::damage` is **one aggregate per actor** and reads
`Disabled` as soon as any declaring node is destroyed. The nine mounts each carry
their own `WEAPON`, ammunition and attack timers, so they fire independently;
declaring `disables = Weapon` on each would silence the whole zeppelin on the
first loss. That edge is therefore not bound. What the original's own rule is —
whether a destroyed mount stops firing only itself, and whether it clears the
in-play bit — needs the engine image (`gwNodeSetActive`, `0x4cca30`; producers
untraced, `cs_script::ir::IN_PLAY_BIT_WRITERS_UNTRACED`).

## What changed

`crates/cs_app/src/mission_session/parts.rs`: `read_turret_records`,
`turret_record_for` (earliest record wins), `pattern_matches`, `turret_part_node`
(`DamageNodeKind::WeaponMount`, designed pool, no `disables`) and
`classed_part_node`. Wiring-only: re-exports in `mission_session/mod.rs`.

Acceptance, prefix `accept_m01_play_triggers_turret_class_`: 2 retail legs
(the nine rows above; exactly nine of the 39 planned chains classify as mounts
and the 30 engine chains do not, with `healthy` and the `PARTS` nodes resolving
under each) and 2 CI legs (a synthetic table; the pattern rule).

## Unknowns, affected content and resolving work

| Unknown | Affected content | Resolves in |
| --- | --- | --- |
| whether destroying a mount clears the in-play bit, and whether a turret's `healthy` state is counted like an engine's | `OBJECTIVE7`/`OBJECTIVE28` (block indexes 6, 27) | #1360's open item, an engine-image trace (`CS_ENGINE_IMAGE`), or an original run (#358) |
| the unit of `HEALTH` (`10.0`; absent on `maingun`) | how many hits a mount survives; the nodes keep the designed unit pool | #668 / F27-E |
| the classification is **not yet consumed**: `mission_actors.rs` still calls `designed_part_node` and the stage carries no `ai.zrd` table | every F29 consumer of the nine nodes | the runtime wiring follow-up filed by this task |
