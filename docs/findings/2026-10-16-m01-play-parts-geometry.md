# M01-PLAY-PARTS-GEOMETRY (#1492): the swept-box geometry and the allegiance a fired round needs

Date: 2026-10-16. Task `M01-PLAY-PARTS-GEOMETRY` (Rally #1492), the follow-up
of `M01-PLAY-TRIGGERS-PARTS` (#1481) and `M01-PLAY-TRIGGERS-TURRET-CLASS`
(#1491). Capabilities used: `retail` (read-only `$CS_GAME_DIR`) plus
build/test. No `gpu`, no `audio`, no original run, no engine-image trace.
Implementer: bunny-alpha-2/bunny-alpha-2. Reviewer: the Rally review claim;
this report names only the implementer until that claim closes, and a review by
the same agent identity would not be independent evidence. Nothing here is
`verified_original`; a Rally merge awards `checked` only.

## The defect this slice repairs

#1481 made M01's 39 part-state chains **observable and addressable** and proved
the `INACTIVE` ladders advance when a part's damage node is destroyed *through
the damage authority*. Nothing in the composed run could destroy a part
**geometrically**, and its own residues said so:

* the parts carried no `PartSweptBox`, no `NodeVisualTransform` and no
  `LinearVelocity`, so the F27 gun step's `part_sweep_candidates` read had
  nothing to sweep — a round crossed no part box ever;
* M01's `zeppelins.zrd` records state no `team`, so nothing declared the
  zeppelins hostile: the F27 gun step's own `(shooter, target)` relation check
  answered `None`, the designed `FriendlyFireRule::HostileOnly` refuses an
  **undeclared** pair, and every shot at a zeppelin was refused by name.

## Measurement 1 — the allegiance is measured; the reading of it is designed

The record states no `team`, but the loader assigns an allegiance while the
record loads (#1155) and the engine's own marked-node/turret resolver decides
it (#1177). That resolved faction rides on the launched world-actor program:
`LoweredWorldActors::actors[i].faction`, a `ContentKind::Faction` id.

Measured over M01's stage (read through the production readers on this run):

| world actor | measured faction | team int (#1155's vocabulary) |
| --- | --- | --- |
| `workersvoyagezep` | `enemy` | 2 |
| `piratezep` | `ally` | 1 |
| a third bound world actor | `enemy` | 2 |

The player's own side is measured too: `aiv.zrd`'s `player` record carries
`team = 1` (#1453, the value #1436's role split compares against).

What is **designed** is the reading that turns those two measured teams into a
relation: `declared_player_allegiance` says a world actor on the player's own
team is the player's flight (`Friendly`) and any other measured team is opposed
(`Hostile`) — exactly the shape #1436's role split uses for aircraft, applied to
the world-actor program. It is labelled
`m01-play-parts-geometry.designed-player-allegiance`. A faction outside the
measured `neutral`/`ally`/`enemy` vocabulary, or a player team that was never
read, answers `None`: no side, and therefore no relation, is invented. Claim ids
for the measured half are the ones #1155/#1177 already filed; the reading is
this task's designed claim.

The declared relation is the pair the gun step reads, and it is declared **both
ways** (`player → faction` and `faction → player`) so the two directions cannot
disagree.

## Measurement 2 — the part box has no measured source, and says so

A part node (`reng1`, `utur1`, …) is an `object3d` record of the container
hierarchy, and the world import makes an object only for a record the **world
node owns directly** (`cs_content::world::import_world_container`), so a part
node gets no ECS object; `MemberResolver` reports existence only and carries no
position. No reader in this tree decodes a container node's authored transform
or stored box, so the part's box **centre and extents have no measured source**.

Per AGENTS.md rule 4 this slice does not invent one and does not present a guess
as a measurement:

* the **box** is the labelled designed `DESIGNED_PART_HALF_EXTENTS_M`
  (`[4.0, 4.0, 4.0]` m, one value for every part, so it is explicitly not a
  per-part tuning table) under the designed claim
  `m01-play-parts-geometry.designed-part-box`;
* the value a measurement would replace it with is carried **beside** it as an
  explicit `Resolved::Unknown` under
  `m01-play-parts-geometry.node-box-unmeasured`, naming the exact node whose
  record was not decoded;
* the **centre** is the one placement the composition measures: the owner
  world actor's session pose at bind time, which is the canonical pose the
  world-actor session reports;
* the **motion** is zero: M01's zeppelins declare `DeclaredMotion::Held` (no
  route, no keyframes, no velocity), so the part's own velocity is zero and
  `part_sweep_candidates` reconstructs the previous centre exactly as it does
  for a still target.

## What changed

`crates/cs_app/src/mission_session/parts.rs` (owner path):

* `DESIGNED_PART_HALF_EXTENTS_M`, `M01_PART_GEOMETRY_CLAIM`,
  `M01_PART_GEOMETRY_UNKNOWN_CLAIM`, `part_sweep_geometry` and the
  `MissionPartSweep` component, which records where the box came from and the
  unmeasured container box it stands in for;
* `spawn_part_state` now gives every part entity the whole geometric face: the
  designed `PartSweptBox`, the composed `NodeVisualTransform` at the owner's
  bind pose, and the zero `LinearVelocity` a `Held` actor has. The part-state
  chain entity is otherwise unchanged, so #1481's observer and ladders are
  untouched.

`crates/cs_app/src/mission_actors.rs` (owner path):

* `WorldActorRoster` (the actor's measured faction and kind, the program's
  objective flag, and the designed player relation) is decided in the spawn
  host and carried on `MissionZeppelinActor`;
* `measured_team`, `declared_player_allegiance`, `target_class_of`,
  `M01_ZEPPELIN_ALLEGIANCE_CLAIM`, the `MissionZeppelinAllegianceReport` and the
  per-tick `roster_mission_zeppelins` entry (in
  `MissionZeppelinAllegiancePlugin`), which registers each bound actor's
  `TargetRecord` with the session's targeting store and declares the pair's
  relation. It is idempotent, so it runs every fixed tick and a restart — which
  opens a fresh store and re-spawns the actors — re-rosters exactly the new
  generation. The player's measured team travels through the `HostView`, so the
  composed restart, which holds the stage out of the world, keeps it.

Wiring only: the plugin registration and the report's teardown removal in
`mission_session/compose.rs`, the `parts`/`MissionPartSweep` re-exports in
`mission_session/mod.rs`, and the new test module's `mod` lines.

## The acceptance suite

`crates/cs_app/tests/campaign/m01_play_parts_geometry.rs`, prefix
`accept_m01_play_parts_geometry_` — 5 tests (4 CI, 1 retail):

* `…_the_fire_key_destroys_a_bound_part_and_advances_its_ladder` (CI) — the
  composed synthetic mission binds one part with the whole geometric face and
  one hostile world actor, the player's **own fire key** delivers
  `FirePrimary`, the production gun step sweeps the part's box under the
  declared relation, the damage authority applies the hit and the `INACTIVE`
  ladder completes on the production observer's own row. Removing the geometry
  or the roster makes the sweep refuse and this leg fail;
* `…_a_world_actor_on_the_players_team_is_refused` (CI) — the same run with the
  actor's measured faction on the player's team: the relation reads `Friendly`,
  `HostileOnly` refuses every contact and the part never leaves play, so the
  allegiance is load-bearing rather than a blanket hostility grant;
* `…_an_unread_player_team_declares_no_side` (CI) — the fail-closed half: with
  no measured player team the actor is rostered but no relation is declared and
  the shot is refused;
* `…_the_allegiance_reading_is_the_measured_team_split` (CI) — the pure reading
  over the measured vocabulary;
* `…_m01_binds_the_measured_allegiance_and_a_labelled_box_on_every_part`
  (retail, `#[ignore = "requires CS_GAME_DIR"]`) — M01's composed run binds the
  39 parts, every one carrying the box at its owner's measured bind pose with
  the designed claim and the recorded unknown; the player's measured team is 1;
  `workersvoyagezep`'s measured faction is `enemy` and `piratezep`'s is `ally`;
  every rostered actor's relation is the designed reading; the targeting store
  holds each actor under its measured faction and the player's declared pair;
  and the production `part_sweep_candidates` read admits all 39 parts with a
  relation and refuses none.

```sh
cargo test --workspace --locked -- accept_m01_play_parts_geometry_ --include-ignored   # 5 tests, 1 retail
```

## Evidence

The acceptance run of record and the structural check:

```sh
set -o pipefail
cargo test --workspace --locked -- accept_m01_play_parts_geometry_ --include-ignored \
  2>&1 | tee private/evidence/M01-PLAY-PARTS-GEOMETRY/cargo-test.log        # exit 0

CS_EVIDENCE_DIR=private/evidence/M01-PLAY-PARTS-GEOMETRY \
CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_play_parts_geometry_ --include-ignored" \
CS_EVIDENCE_EXIT_CODE=0 CS_EVIDENCE_REVIEWER=<the reviewer's own Rally identity> \
  cargo test --locked -p cs_app --test campaign evidence_report_m01_play_parts_geometry -- --ignored

python3 tools/validate_evidence.py private/evidence/M01-PLAY-PARTS-GEOMETRY/acceptance.json \
  --artifact-root private/evidence/M01-PLAY-PARTS-GEOMETRY
```

`--require-pass` is deliberately **not** used: it refuses any report whose
`unknowns` array is non-empty, and this report carries four unresolved issues
that must stay in machine-readable evidence. Deleting them to turn the flag
green is exactly what the owner directive of 2026-10-05 forbids. The report's
`claim` is `implemented`, its capabilities are `[retail, synthetic]` — no `gpu`
capability is claimed — and the pass/fail facts are carried by
`command.exit_code` and the `tests` counts, which the plain validation does
check. The harness reads the reviewer identity from `CS_EVIDENCE_REVIEWER`, so
the reviewer's own regeneration names itself instead of a literal.

## Unknowns, affected content and resolving work

| Unknown | Affected content | Resolves in |
| --- | --- | --- |
| each part node's authored transform and stored box (no decoded source) | every geometrically destroyable part box of M01's two zeppelins, i.e. how close a shot must be | the container node decode (F18/F35), or a measured original hit / F27-E's measured damage table |
| the original's per-part integrity | the pool every part damage node carries (still the designed single unit #1481 bound) | #668 (the measured damage table), F27-E |
| which gameplay side `ally`/`enemy` place the player on, and whether a script flips a relation mid-mission | every world actor's relation to the player; in particular M01's `piratezep`, whose measured `ally` faction makes the player's guns refuse it under this design | an original run (#358), or the F30/F34 faction-consumption stages; a scripted relation change is F39 |
| the in-play bit's producers (`+0x24` bit 4) | what the original does when a part is hit, before `gwNodeSetActive` clears the node | #1360's open item; an original run (#358) |

## Status and limits

* `checked` only when merged: no `verified_original` and no `release_approved`
  is claimed anywhere, and no original executable ran.
* The part box is **designed**: every part gets the same 4 m half extents and
  the geometry that would replace them is recorded as an open unknown. The
  destruction a leg proves is reachable *given* that box; nothing here claims
  the original's parts had that box, or that a shot at them hit at that
  distance.
* The relation is designed from measured inputs. It is **not** a statement that
  the original's combat sides are the loader's team ints compared with the
  player record's team — that reading stays unmeasured, and it is named in the
  unknowns above and in the evidence report.
* The geometry is at the owner's bind pose and does not move: M01's zeppelins
  are `Held`. A world actor with a route, keyframes or a carried socket would
  need the part pose to follow it, which this slice does not do (no such actor
  is bound today).
* No original assets, extracted scripts or engine bytes are committed here. The
  evidence log holds only test names, counts and paths.
