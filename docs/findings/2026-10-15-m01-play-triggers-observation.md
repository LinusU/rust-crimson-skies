# M01-PLAY-TRIGGERS-OBSERVE: the world observation comes from the live composition

Date: 2026-10-15. Task: `M01-PLAY-TRIGGERS-OBSERVE` (Rally #1480), the first
slice of `M01-PLAY-TRIGGERS` (#1439). Capabilities used: `retail` (read-only
`$CS_GAME_DIR`) plus build/test. No `gpu`, no `audio`, no original run.
Implementer: bunny-alpha-1. Nothing here is `verified_original`; a Rally merge
awards `checked` only.

## What this slice is, and what it is not

`MissionWorldObservation` — the port `MissionHost::step` folds in stage 4 —
has existed since VS-M01-RT-MISSION-HOST (#1217), and the host already merged
it with the record player's animation rows. **No stage ever wrote one**, so
every real run folded an empty world half and
`MissionHostRefusal::WorldObservation` stood over a fact table that held only
animation rows: M01's single `TRAVELERS` block could never hold because the
`["player"]` row nobody produced is exactly the row it reads.

This task installs the writer:

* `crates/cs_app/src/mission_session/observation.rs` — the production system
  [`mission_world_observation`], plus `subject_node_name` (the measured
  subject → node-name read) and the `PLAYER_MEMBER` spelling;
* wiring only: the module + re-exports in `mission_session/mod.rs`, and the
  registration inside `mission_session/host.rs::install_mission_host`, which
  places it in `FixedPostUpdate` after `PhysicsSystems::StepSimulation` and
  **before** `mission_host_tick` — the writer and its consumer are one pair.

Nothing else changed: no condition, no lifecycle model, no record reader.

## What the observer writes, and from where

| Row | Source | Fail-closed rule |
| --- | --- | --- |
| `["player"]` | the `MissionPlayerBody` entity's avian `Position` | exactly one body **and** a pose, else no row |
| `["<node>"]` per bound world actor | the `WorldActorBinding` entity's `Transform` | the node name is read back out of the measured `scene_node/…world1.<name>` subject only when `escape_scene_node_name` maps the key's last component onto itself; a name this reader cannot spell back verbatim yields no row |
| one row per group id | live `ScriptedAircraftActor` entities, keyed by the groups `MissionActorSpawnReport` × `MissionAircraftRoster` show the composition actually spawned | a group nobody spawned gets **no row**, never `0` |

Measured on the composed retail run (the acceptance log and
`world-observation.json` carry these numbers):

* the composed headless run folds **4 member rows** — `["player"]`,
  `["piratezep"]`, `["workersvoyagezep"]`, `["blackswanzep"]` — each at the
  live pose of the entity it came from;
* the group rows are `{1: 4, 3: 1, 5: 4}` — the four `rusdevastator_*`, the
  one `bsfury_1` (the measured sleeper) and the four group-5 aircraft
  (`devastator_2/3`, `wingman_2/3`); because `WorldFactTable::facts` carries
  only what `WorldOperands::of` spells, the fold M01's program actually reads
  is `{1: 4, 3: 1}` — group 5 is observed but never asked about.

## Which of M01's 42 member chains this observer can see

`WorldOperands::of(m01)` spells **42 distinct chains**
(`docs/findings/2026-10-08-m01-lc-world-facts.md`):

* **3 observable** — `["player"]` (the body pose; it resolves against no node,
  so before this task it was always recorded `Missing`), `["piratezep"]` and
  `["workersvoyagezep"]` (the two bound world actors, from their binding
  entities' live `Transform`). The observer also writes `["blackswanzep"]`,
  which no M01 operand spells, so the fold drops it again.
* **39 not observable** — the `workersvoyagezep` (27) and `piratezep` (12)
  part-state chains. `import_world_container` only makes an object for an
  `object3d` node the world node owns directly, so no entity exists to
  observe: these rows are **not written**, and every `INACTIVE` ladder that
  counts them stays `false` exactly as before. **M01-PLAY-TRIGGERS-PARTS
  (#1481)** owns making them observable.

That split is the whole point of the fail-closed rule: this task's observer
makes `OBJECTIVE3` fireable and leaves every other world-shaped block's
answer untouched.

## The trigger, measured

`OBJECTIVE3` is zero-based block index **2** and its condition is

```
All([ObjectiveAwake{2},
     Travelers{subject:["player"], anchor:Object(["workersvoyagezep"]),
               radius:700.0, approaching:true}])
```

Its engine declaration (read through
`mission_session::declared_engine_objectives` in this run, not written down):

| Record spelling (1-based) | Engine index | Effect on completion |
| --- | --- | --- |
| `NAP_OBJECTIVE 4`, 20 s | 3 | `OBJECTIVE4` → `Napping`, re-wakes on the engine timer |
| `WAKE_OBJECTIVE_WHEN_I_COMPLETE 27` | 26 | `Awake` |
| `WAKE_OBJECTIVE_WHEN_I_COMPLETE 51` | 50 | `Awake` |
| `KILL_OBJECTIVE_WHEN_I_COMPLETE 41` | 40 | not present (no row) |
| `KILL_OBJECTIVE_WHEN_I_COMPLETE 48` | 47 | not present (no row) |

All five targets load `Dormant`, so every one of them is observable as a
transition in the composed run; the tick report's own `woken` section later
carries the `3` the 20 s nap expiry wakes.

Block index 2 loads `Awake`, so the record's own `ObjectiveAwake{2}` gate is
satisfied from the first tick — the only thing that ever gated the trigger was
the world observation.

## DEDG groups: who exists, who does not, and why

The eight `DEDG` sites map to groups like this (measured by walking the
lowered program in this run):

| Block index | `OBJECTIVE` | Group | `remaining` |
| --- | --- | --- | --- |
| 1 | 2 | 1 | 0 |
| 15 | 16 | 1 | 0 |
| 16 | 17 | 2 | 0 |
| 19 | 20 | 3 | 0 |
| 20 | 21 | 4 | 0 |
| 41 | 42 | 2 | 0 (plus `ObjectiveAwake{53}`) |
| 42 | 43 | 2 | 0 (plus `ObjectiveAwake{53}`) |
| 43 | 44 | 4 | 1 |

`aiv.zrd`'s twelve records carry these groups: 1 = `{rusdevastator_1, 2, 4,
5}`, 2 = `{rusdevastator_3}`, 3 = `{bsfury_1}`, 4 = `{bsfury_2}`, 5 =
`{player, devastator_2, devastator_3, wingman_2, wingman_3}`.

**Groups 2 and 4 are empty for a measured reason, not an accident:** their
only records are exactly the two the original's own spawn loop skips —
`enabled == 0` at `0x474d10` drops `rusdevastator_3` and `bsfury_2`
(#1453). The composition therefore spawns nothing of them, the observer has
nothing to count, and per the fail-closed rule **no row is written for 2 or
4** — `EnemyGroupDepletion` over them keeps answering `false`, which the
acceptance test pins on the composed run.

Two consequences worth naming:

1. Whether the *original* answered `0` (no living member) or "unknown" for a
   group whose records never spawned is **unmeasured** here. This
   reimplementation's evaluator answers `false` for a group the facts do not
   carry ("an unrecorded group is unknown, not an empty one",
   `cs_script::runtime::MissionState::holds`), so blocks 16/41/42 and 20/43
   cannot complete until something observes those groups. That is recorded as
   a follow-up on **#1483** rather than a count nobody observed.
2. The observer counts **composition-spawned scripted aircraft entities**.
   The `player` record (group 5) is not one of them — the composition's
   player body carries no group — so group 5 is observed as 4 rather than the
   record's 5. No M01 `DEDG` reads group 5; general group membership is
   #1483's to measure.

## `zeppelins.zrd` carries no group field — measured, not assumed

* `aiv.zrd`'s AI group is **field 4 of an 81-field positional record**,
  parsed at `0x437726` into the document at `+0x38` and stored at
  `vehicle+0x388` by `0x47c694` (#1453) — the field `DEDG` counts.
* A `zeppelins.zrd` record is a **keyed** list under a different grammar
  (#574): its 26 keys are the exhaustive `ZeppelinKey::ALL` vocabulary, and
  `read_zeppelins_member` refuses any key outside it
  (`ZeppelinsError::UnknownKey`). M01's three records decode through that
  reader in production, so no `group` key can be present — and indeed the
  census over every carrying member (recorded in `world-observation.json`)
  shows M01's rows stating only measured keys.

So there is **no zeppelin group to report**: the observer's group rows come
from `aiv.zrd` records alone, and DEDG groups 2 and 4 stay unobserved. The
third task of #1439's split, **M01-PLAY-TRIGGERS-DEPLETION (#1483)**, owns
whatever content or measurement those groups need.

## The windowed criterion is **not** met here

#1439's acceptance carries into this slice the sentence "fires … in the
windowed mission as well as headlessly — produce the windowed evidence". It
cannot be met from this task's owner paths, and the report does not claim it:

* the mission starts **5 311 m** from `workersvoyagezep` against a **700 m**
  radius; a straight flight from the authored heading passes no closer than
  ~1 400 m, so the trigger cannot fire by flying the authored course;
* `cs --mission` accepts **no** input replay, no start-pose override and no
  camera flag — the CLI parser knows only `--cs-path`, `--mission`,
  `--playtest`, `--synthetic`, `--headless`, `--ticks`, `--seed`, `--trace`
  and `--capture-dir`, and `--input-replay`/`--cam` exist only in
  `docs/contracts/CLI-EVIDENCE.md` as interfaces that are **not implemented
  yet**;
* the windowed capture path (`CS_MISSION_CAPTURE=1`) drives a *fixed* input
  script — one second of pitch-up — and ends the run at frame 180 (+ a
  screenshot grace), i.e. about three seconds of flight.

A windowed run that could place or fly the player is a **product capability
its own task must add** (a `--mission --input-replay`, a start-pose override,
or an evidence script long enough to fly the approach); every candidate lives
in `cli.rs`/`mission_launch.rs`/`mission_session/evidence.rs`, which are
outside #1480's owner paths. Filed with `create_tasks` as
**`M01-PLAY-TRIGGERS-WINDOWED`**, which also carries #1439's windowed
criterion so it survives this slice. Until it exists, the headless legs and
the windowed leg of #1439 stay **unmet**, and no `gpu` capability is claimed
in this task's evidence report.

## How to re-derive every number here

```sh
cargo test --workspace --locked -- accept_m01_play_triggers_observe_ --include-ignored   # 5 tests (2 synthetic, 3 retail)
```

* `..._the_synthetic_run_observes_the_players_live_pose` and
  `..._the_synthetic_trigger_fires_only_when_observed` run in CI (no
  installation): the first pins the row, its ordering against the composed
  entry, the empty group map and the "report taken away" leg; the second
  binds a world actor on the synthetic stage and fires the `TRAVELERS` arm
  only when the observed player row is inside the radius.
* `..._the_trigger_fires_from_the_players_real_position` is the retail leg:
  the observed rows, the parked player, another actor inside the radius, the
  stripped report, then the body flown along a ten-waypoint route until the
  *observed* distance drops under 700 m — plus the five effect transitions
  and the nap's re-wake in a later tick's `woken`.
* `..._group_counts_come_from_the_live_entities` despawns group 1's four
  aircraft and expects `0` **and** `OBJECTIVE2`'s completion, while groups 2
  and 4 stay absent from both the observation and the fold.
* `..._zeppelin_rows_carry_no_group_field` runs the production carrier census
  over the installation.

The evidence report is produced by
`crates/cs_app/tests/evidence_report_m01_play_triggers_observe.rs`,
structurally validated with `python3 tools/validate_evidence.py …
--artifact-root …` (exit 0), and its copy is committed as
`docs/findings/evidence/M01-PLAY-TRIGGERS-OBSERVE.json`. The `--require-pass`
form of that check — the one `docs/contracts/CLI-EVIDENCE.md` names, and the
one step 3 of the harness's module doc used to show — does **not** pass on
this report, for the reason spelled out under Evidence below; the harness's
doc now says so instead of prescribing a command that exits 3, and no unknown
was removed to turn it green.

## Evidence

The acceptance run of record (its recorded log is the first artifact):

```sh
set -o pipefail
cargo test --workspace --locked -- accept_m01_play_triggers_observe_ --include-ignored \
  2>&1 | tee private/evidence/M01-PLAY-TRIGGERS-OBSERVE/cargo-test.log
```

then the harness, on the same clean commit, and the structural check:

```sh
CS_EVIDENCE_DIR=private/evidence/M01-PLAY-TRIGGERS-OBSERVE \
CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_play_triggers_observe_ --include-ignored" \
CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
  cargo test --locked -p cs_app --test evidence_report_m01_play_triggers_observe -- --ignored
python3 tools/validate_evidence.py private/evidence/M01-PLAY-TRIGGERS-OBSERVE/acceptance.json \
  --artifact-root private/evidence/M01-PLAY-TRIGGERS-OBSERVE
```

`--require-pass` is deliberately **not** used: it refuses any report whose
`unknowns` array is non-empty (`tools/validate_evidence.py`, line 60), and
this report carries four unresolved issues that must stay in machine-readable
evidence — the three sibling gaps (#1481, #1482, #1483) and this slice's
unmet windowed criterion (#1486). Deleting them to turn the flag green is
exactly what the owner directive forbids ("never remove an unresolved issue
from machine-readable evidence merely to pass a validator"). The report's
`claim` is `implemented`, its capabilities are `[retail, synthetic]` — no
`gpu` capability is claimed — and the pass/fail facts are carried by
`command.exit_code` and the `tests` counts, which the plain validation does
check.

## Reviewer's regeneration (2026-10-15)

The reviewer of #1480 re-ran every check and regenerated this report on the
reviewed commit `46993f25` (tree `f0d1fda4…`), as
`docs/contracts/CLI-EVIDENCE.md` asks of a reviewer: `cargo fmt --check` 0,
`cargo clippy --workspace --all-targets --all-features --locked -- -D
warnings` 0, `cargo test --workspace --locked` 0 (587 `test result: ok`,
none failed), and the acceptance suite 5/5 with `--include-ignored` (exit
0). Structural validation exits 0 with all four unknowns intact. Every
measured value is unchanged from the implementer's run — 42 spelled chains,
4 observed, groups `[1, 3, 5]`, 3 zeppelins, 62 `zeppelins.zrd` keys, `nap`
index 3 for 20 s, wake `[26, 50]`, kill `[40, 47]`, ticks 0..8 — so the only
differences in the committed copy are the run's own fingerprints
(`candidate_tree`, `created_at`, the two artifact hashes) and
`review.identity`, which now records the reviewer.

Two review fixes ride on that regeneration:

* the second artifact is now **valid JSON**. The harness spliced Rust's
  `Debug` output (`Some(3)`, `Some(20.0)`) into `world-observation.json`'s
  `nap_target_index` / `nap_seconds` fields, which made the whole artifact
  unparseable — `python3 -m json.tool` failed at that offset — while the
  structural validator still passed, because it hashes the artifact without
  reading it. The harness now renders `null`, numbers and arrays
  (`"nap_target_index": 3, "nap_seconds": 20`), and the committed copy's
  `world-observation.json` hash changes with it.
* the DEDG group-1 leg pins the negative half of its claim too: the block
  does **not** complete while group 1's four members are still in play, read
  before the despawn that completes it.

The review could not be run by a different model or agent: it was done by a
fresh session of the same agent identity that implemented the task, which is
recorded in the report's `review.identity` and in the Rally review note. It
is therefore not independent evidence, and no agent review replaces the
owner's approval; nothing here is `verified_original`.

## Status and limits

* `checked` only when merged. No original executable ran; `retail` is read
  access to the owner's installation. Nothing is `verified_original`.
* The 39 part-state chains stay unobservable (**#1481**), the three
  `ANIM_STATE` animations still have no producer (**#1482**), DEDG groups 2
  and 4 still have no observed member (**#1483**), and the windowed firing
  criterion is unmet (**M01-PLAY-TRIGGERS-WINDOWED** above).
* A bound world actor's pose is read from its binding entity's `Transform`.
  Nothing in this composition moves that entity after it is bound: M01's
  `zeppelins.zrd` records carry no route (`DeclaredMotion::Held`,
  `MOTION_RESIDUE`), so the binding pose and the world-actor session's
  canonical pose agree. Whether a driven world actor's binding follows its
  session is unmeasured and is not claimed here.
* The observer reports an entity it can see as **in play**. The in-play
  bit's producers (`+0x24` bit 4) remain outside the measured bound
  (`cs_script::ir::IN_PLAY_BIT_WRITERS_UNTRACED`): this module reports
  presence, it never derives it — which is why `["piratezep"]` is reported
  `InPlay` and the `INACTIVE` ladder over it (block 54) stays unarmed.
