# M01-BRIDGE-CONVOY-SPAWN (#1566): M01's bridge convoy, camps and wreck as damageable targets

Date: 2026-10-18. Task: #1566 `M01-BRIDGE-CONVOY-SPAWN` — "Wire ZBD/C3/M01's
bridge convoy, camps and wreck as damageable targets". Feature sheet:
`missions/M01.md` (`M01-B`), shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: **`retail`** (read-only `$CS_GAME_DIR`) plus ordinary
build/test. `gpu`/`audio` were available and **not** used: nothing is drawn or
played by this slice. No original executable ran, so nothing here is
`verified_original`.

## Why this task exists

`docs/findings/2026-10-17-m01-ground-units-locate.md` (#1549) measured where
M01 "The Lost Treasure" keeps its convoy, camps and wreck: ordinary records of
`ZBD/C3/gamez.zbd`, listed by the mission's own `targets.zrd`, and given a
`healthy`/`destroyed` layout and a destruction chain by the world group's
`ZBD/C3/zrdr.zbd` animation members. Nothing bound them. The world import makes
an object only for a record the **world node owns directly**, so a bridge child
(`t_truck01`, `bridge_truck01`) and a town child (`u_camp1`) had no entity at
all, and no consumer read the mission's `targets.zrd` or the authored damage
coupling. `objectives.zrd` names them (`t_truck01`..`03`, `bridge_truck01`,
`shipwreck`, `WAKEUP_TURRETS t_truck**`); the mission program's own directive
lowering owns the wake, and this task owns the binding.

## What changed

* **New: `crates/cs_app/src/mission_targets.rs`** — the binding. It reads two
  production sources and joins them to the world container:

  * `read_mission_targets` — the mission's `targets.zrd`, in the original's
    mission → group → shared mount order, through the measured pair walk
    (`cs_content::scenario_control`'s shape). Each row keeps `description`,
    the `nodes` chains, `help_label`, `category_label` and the two markers
    (`objective`, `other_target`). A refusal is never folded into an empty
    table.
  * `read_damage_couplings` — every `.zrd` member of the group's reader
    archive that reads as an `ANIMATION_DEFINITIONS` record through
    `crate::animation::programs::read_animation_definition_member`, keeping
    each definition that states a damage-coupling field (`HEALTH`,
    `DAMAGE_SEQUENCE`, `PROXIMITY_DAMAGE`, `COPY_NODE_DATA`). A member whose
    bytes spell a coupling field but which the production reader refuses is a
    refusal, never a silent skip.
  * `bind_mission_targets` — resolves every declared chain and every damage
    selector against the container's stored names through the production
    `WorldNodeNames` rule, reads the node's **composed world position** from
    the production scene graph (`SceneNode::world_transform`, `root ∘ … ∘
    local`), and reads the node's authored `healthy`/`destroyed` children off
    the container hierarchy.

* **`mission_session::compose::stage_for`** reads the binding once (after the
  turret table) and carries it on the host seed;
  **`mission_session::host::MissionHostSeed`** gains `targets`;
  **`mission_actors`** gains `MissionGroundTarget`, `BoundGroundTarget` and
  `MissionGroundTargetReport`, and `spawn_mission_actors_under` registers one
  damageable actor per bound target (one lethal `InternalStructure` node named
  for the container node) beside the aircraft and zeppelins. `despawn_mission_actors`
  despawns them and removes the report, so a restart reports its own generation.
  The synthetic stage carries the empty binding.

## The measured data

Read through the production readers over the owner's installation (counts and
names only; no original byte is reproduced):

* **`ZBD/C3/M01/zrdr.zbd::targets.zrd`** — 11 records. The four
  `MSG_OBJ_BRITCONVOY` rows name `t_truck01`, `t_truck02`, `t_truck03` and
  `bridge_truck01` (`help_label MSG_OBJ_DESTROY`); `MSG_OBJ_SITE1/2/3` name
  `t_chamber`, `grasshut2` and `shipwreck` (`objective`, `MSG_OBJ_EXAMINE`);
  the rest are `rock_zeppelin` under `piratezep` (`other_target`,
  `MSG_OBJ_ZEPPELIN`), `pzhookpoint` (`MSG_OBJ_DOCK`), `dz4`
  (`MSG_OBJ_FLYTHROUGH`) and `hydrogentank1` (`MSG_OBJ_DESTROY`).
* **`ZBD/C3/zrdr.zbd`** — 11 damage members: `airdock.zrd`, `boats_still.zrd`,
  `craterlake.zrd`, `fueltruck.zrd`, `grasshuts.zrd`, `hydrogen.zrd`,
  `susp_bridge.zrd`, `tower_dest.zrd`, `trucks_dest.zrd`, `ucamp_dest.zrd`,
  `unit_dest.zrd`. Their selectors resolve to 58 container records, and with
  the 11 declared rows (three of them shared) the binding is **69 targets**.
* **The task's three named members bind exactly as the finding predicted:**
  `bridge_truck01` (`fueltruck.zrd`, `HEALTH`), `u_camp1..3`/`u_camp5..8`
  (`ucamp_dest.zrd`, `HEALTH` + `DAMAGE_SEQUENCE` + `PROXIMITY_DAMAGE`, each
  with a `healthy`/`destroyed` child pair) and `susp_bridge` (`susp_bridge.zrd`,
  `HEALTH`). The three gun trucks carry their authored `healthy`/`destroyed`
  layout and are declared targets; their destruction is driven by
  `fueltruck.zrd`'s `chainreaction` calling `destroy_ttruck01..03`, which the
  group archive does not define.
* **Composed positions** come from the scene graph's own `world_transform`, so
  a bridge child (`world1.susp_bridge.t_truck01`) and a town child
  (`world1.town_2.u_camp1`) resolve through their whole ancestor chain. The
  bridge children's ancestors are identity, so their composed positions equal
  the stored translations the finding already recorded.

## What is consumed, and the fail-closed rule

Every bound target becomes a **live damageable actor**: one lethal
`DamageNodeKind::InternalStructure` node whose key is the container node's own
name, registered on the composition's `SessionDamageResolver`, with a matching
`MissionGroundTarget` entity at the composed position. The authored
`healthy`/`destroyed` layout and the coupling **field names** ride on the
component, read and never interpreted. The integrity pool is the labelled
designed single unit `DESIGNED_TARGET_INTEGRITY` (claim
`m01-bridge-convoy-spawn.designed-target-integrity`), the same shape
`mission_session::parts` uses for a zeppelin part: a node whose pool is
`Resolved::Unknown` blocks every hit, so the destruction path could never take
a target out of play at all.

## Unknowns and limitations (recorded, not guessed)

* **The coupling's meaning is unmeasured.** `HEALTH`, `DAMAGE_SEQUENCE` and
  `PROXIMITY_DAMAGE` state an authored destruction chain whose thresholds and
  sequence roles are F29's measurement
  (`f20-anim.damage-coupling-unmeasured`). The task's warning that the health
  **values** stay unknown is honoured literally: this binding reads the field
  **names** and never a value, and no authored number becomes a pool.
  **Affected content:** the durability and destruction law of every bound
  target. **Resolving task:** the F29 damage-model measurement; none filed
  here.
* **The targets are damageable but not in the F30 targeting store.** The
  mission's `targets.zrd` states no `team` and the world records declare none,
  so no player↔target relation is invented and no roster record is written.
  **Affected content:** whether the player's reticle/selection can name a
  convoy truck. **Resolving task:** filed as a follow-up (a measured faction
  for a world-container target).
* **`hydrogen.zrd`'s `NAME1` path binds its terminal only.** The production
  node-name table has no hierarchy, so the path's steps are not resolved: the
  binding reads the path's terminal `hydrogentank*` and binds
  `hydrogentank1..4`, never `cargozep1`/`underneath` — a node the definition
  merely names as an ancestor is not turned into a target.
* **The world group's damage set is the whole group's, not the mission's
  alone.** `ZBD/C3/zrdr.zbd` is the world group's reader archive, so its
  damageable records (boats, towers, units, airdock, crater lake) bind for
  every C3 mission, exactly as the original loads the group for M01. The
  mission's **declared** targets are marked on the binding, so a consumer can
  tell the mission's own list from the world's damage set.
* **`grasshuts.zrd`'s `grasshut#` selector matches nothing.** `#` after a
  name is not the measured `*` wildcard, so the selector is a literal the
  container does not carry and the family is a measured absence (not bound).
  The reader reports it; no grass hut is guessed into the target set.
* No image bytes, disassembly or decompiled code are committed; the names
  quoted are stored display names and message keys.

## Test inventory

| test | covers | fails when |
| --- | --- | --- |
| `accept_m01_bridge_convoy_spawn_declared_targets_resolve` (retail, `#[ignore]`) | the 11 declared rows, the four convoy rows in order, the wreck, every row bound, and the composed positions of the four convoy trucks and the wreck at their measured metres under `ObservedTool` provenance | the reader stops consuming `targets.zrd`, a row is dropped, or a composed position stops coming from the scene graph's `world_transform` |
| `accept_m01_bridge_convoy_spawn_damage_coupling_is_read` (retail, `#[ignore]`) | the 11 damage members, `bridge_truck01`'s `HEALTH` from `fueltruck.zrd`, the camps' three coupling fields and `healthy`/`destroyed` pair from `ucamp_dest.zrd`, `susp_bridge`'s `HEALTH` from `susp_bridge.zrd`, the gun trucks' destroy layout, and no unmeasured selector | a coupling field or member stops being read, the layout stops being classified, or a selector silently disappears |
| `accept_m01_bridge_convoy_spawn_composed_targets_are_damageable` (retail, `#[ignore]`) | a headless M01 whose `MissionGroundTargetReport` registers 13 named targets under the composition's session, each read back from `SessionDamageResolver` as a lethal `InternalStructure` node, and `u_camp1`'s entity carrying the authored layout | the composition stops spawning a target, stops registering its damage node, or spawns it under another class |
| `accept_m01_bridge_convoy_spawn_rows_and_damage_node_are_production` (CI, no installation) | the `targets.zrd` pair shape read from a hand-authored `ZrdValue`, and a bound target's damage node being the lethal designed-pool `InternalStructure` node under `Designed` provenance | the row reader or the damage-node builder stops consuming production code |

The retail legs are `#[ignore = "requires CS_GAME_DIR"]` so CI (which has no
installation) skips them; they are run locally with `--include-ignored`, and
they fail loudly when `CS_GAME_DIR` is unset.

## Commands run

On this branch's head, after the rebase onto `origin/main`:

```sh
cargo fmt --all -- --check                                            # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # exit 0
cargo test --locked -p cs_app --test cs_app_tests_a -- accept_m01_bridge_convoy_spawn_ --include-ignored
#   exit 0: 4 tests discovered, executed and passed (1 synthetic, 3 retail)
cargo test --locked -p cs_app --test cs_app_tests_a -- \
  campaign::m01_play_actors campaign::vs_m01_rt_host_restart \
  campaign::m01_play_triggers_observe --include-ignored                 # exit 0, 16 passed
cargo test --locked -p cs_app --test cs_app_tests_a -- campaign::vs_m01_rt_window   # exit 0, 2 passed
cargo test --locked -p cs_app --test cs_app_tests_d -- accept_f34_c_world_actors    # exit 0, 20 passed
```

`cargo test --workspace --locked` is not run locally (AGENTS: CI runs the full
suite on the pushed commit); the crate this branch changes was run with `-p`,
and the retail suites that read the installation with `--include-ignored`.

## Sources used

* The production readers over the installation: `cs_assets::install::discover`,
  `cs_formats::script_raw::discovery::discover_container`,
  `cs_formats::gamez::read_gamez_nodes`, `cs_content::world::world_scene_graph_from_gamez`,
  `crate::animation::programs::{read_animation_definition_member, WorldNodeNames}`.
* `docs/findings/2026-10-17-m01-ground-units-locate.md` (#1549: where the units
  are), `docs/findings/2026-10-18-m01-c3-world-residuals.md` (#1578: the
  grouping records the bridge and the wreck are named by),
  `docs/findings/2026-10-16-m01-play-triggers-turret-class.md` and
  `2026-10-16-m01-play-triggers-turret-wire.md` (#1491/#1499: the damage-node
  classification and its wiring this slice follows).
* `crates/cs_app/src/mission_session/parts.rs` (the designed-pool precedent),
  `crates/cs_app/src/mission_world_actors.rs` (the scope-read pattern),
  `crates/cs_app/src/animation/programs.rs` (the animation-definition reader
  and the damage-coupling family).

## Evidence

No evidence report is produced: this task needs no capability beyond
`retail` and ordinary build/test, and its acceptance runs are the four
`accept_m01_bridge_convoy_spawn_` tests above. The `retail` capability is
exercised by the three `#[ignore]` legs; CI runs the synthetic leg.
