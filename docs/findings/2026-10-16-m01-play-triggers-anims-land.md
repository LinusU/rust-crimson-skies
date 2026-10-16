# #1498 M01-PLAY-TRIGGERS-ANIMS-LAND: the landing table, its nodes, and the designed landing path

Date: 2026-10-16. Task: `M01-PLAY-TRIGGERS-ANIMS-LAND` (Rally #1498), the
landing half of parent #1482. Capabilities used: `retail` (read-only
`$CS_GAME_DIR`), `retail` + `CS_ENGINE_IMAGE` for the composed members, plus
build/test. No `gpu`, no `audio`, **no original run** — nothing here is
`verified_original`; a Rally merge awards `checked` only. Implementer:
`bunny-alpha-1`.

This is the implementation half of #1482. The measurement it must not
contradict is
`docs/findings/2026-10-15-m01-play-triggers-anims-wake-anim-record-join.md`
(committed at `b1c0ff5c`): `landings.zrd` carries exactly three rows, the
landing table runs the gated records, and `WAKE_ANIM` events arm landing
nodes but never run a gated record. The arming half is #1497's
(`2026-10-15-m01-play-triggers-anims-arm.md`).

## 1. What this task adds, and where it lives

* `crates/cs_app/src/mission_session/landings.rs` — one module holding the
  production **reader** (rows, then the nodes' position rules) and the
  production **consumer** (the landing decision, the row activation and its
  call chain). It sits in `mission_session` because the consumer reads three
  records the mission host already holds — the stage-4 observation, the
  armed-node set and the name index — and starts rows through the host's own
  player; `cs_script`/`cs_content` must stay engine-free (AGENTS rule 7).
* The composition inserts the table as a `MissionLandings` resource
  (`compose::add_composition`), from the **stage mount's own** archives. The
  mount host is the *world-group directory* one level under the
  installation root, so the reader — which discovers the installation and
  addresses archives by logical keys — is handed the root the mount's
  `directory` spelling is relative to. A fixture installation that declares
  no landing member carries the reader's absence **by name** in the
  resource; `teardown` removes the resource beside the other session
  resources.
* The observation writer (`mission_session/observation.rs`, step 4) writes
  one row per landing node, composing its position from its anchor's
  observed row (§2).
* The host's `MissionHost::step` runs the consumer at **stage 6c** (§5).

## 2. Measured: the table and its three rows

Read from `zbd/c1c/zrdr.zbd :: landings.zrd` through production discovery
(`cs_assets::install::discover`), `discover_container` and `decode_zrd`,
re-derived on every composed retail run and pinned by
`accept_m01_play_triggers_anims_land_the_retail_reader_reports_m01s_rows_and_nodes`
and the pure parse members (claim `m01-landings.landings-zrd-rows-verbatim`,
evidence class **ObservedTool**):

| row | `anim` | `node` | `angle` | `speed` | `auto` |
| --- | --- | --- | --- | --- | --- |
| 0 | `wv_initiate_hookup` | `wv_manual_land` | `[35]` | `[50, 220]` | – |
| 1 | `hooked_to_klondike` | `pz_manual_land` | `[35]` | `[50, 220]` | – |
| 2 | `hooked_to_klondike` | `pz_auto_land` | – | – | `[]` |

The row shape is a flat `(key, value)` list. The reader keeps every field
verbatim and refuses by name any row whose shape it cannot read — an
unknown key, a non-text `anim`/`node`, a duplicate key, a `speed`/`angle`
list holding a non-number, a missing `anim` or `node` — while the readable
sibling rows still parse (a malformed row is dropped and named, never
partially read; no defaults, no guessed arity). The `.zrd` grammar decoder
yields no per-row byte offsets, so a row carries its **member's** span plus
its ordinal — never an invented byte range.

Why the reader lives where it does: `landings.zrd` exists in all eight
world-group archives (`ZBD/C1..C5, C1B, C2B, C1C/zrdr.zbd` — measured by
grep over the installation), so the archive is addressed by the mount's own
group directory, exactly like every other mission record the stage reads.

## 3. Measured: the nodes' positions in the world container

The three nodes exist in `zbd/c1c/gamez.zbd`'s node array, decoded through
`read_gamez_nodes` and `world_scene_graph_from_gamez` with the **retail
GameZ** coordinate source — the same conversion the definition import uses,
so a node's offset and a bound actor's pose are in one frame (mesh slots
and semantic bindings are passed empty: neither takes part in a node's
world transform, and the launch closure builds its verdict graph the same
way). Measured positions (container frame):

* `world1.workersvoyagezep.…​.underneath.wv_manual_land` —
  `[≈ -0.011, -67.05, 150.46]`
* `world1.piratezep.…​.pz_manual_land` and `…​.pz_auto_land` —
  `[≈ 0, -35.34, 116.61]`

Two facts matter here, both measured:

1. **The container frame is the composition's frame.** An unrelated node
   (`world1.g27816`, `[−2560, 1024, −10752]`) sits ~2 km from the composed
   player start `[−3694, 1318, −12482]` in the same coordinates, and every
   ancestor of the landing nodes up to `world1` has an identity transform.
2. **The container's zeppelin node is not where the mission's zeppelin
   flies.** `world1.workersvoyagezep` sits at the container origin, while
   the mission's workersvoyagezep is a bound world actor at
   `zeppelins.zrd`'s stored pose (`[−5972, 1460, −7680]`). The container
   states the landing node's position inside its **static** hierarchy; the
   player docks at the **live** actor. Nothing in the container states the
   join between the two — that join is the designed rule below
   (`m01-landing.node-position-designed`).

The reader therefore produces, per distinct node name, a
`LandingNodeEntry { node, anchor, offset }` where `anchor` is the ancestor
that hangs directly under the world root (`workersvoyagezep`,
`piratezep` — each exactly the subject-node name a bound world actor's row
is keyed under in the observation) and `offset` is the node's world
position minus the anchor's. A node the container does not state, or
states twice (`land_on` does, under both zeppelins — which is why no
landing row may name it), gets a named refusal and its row can never run.

## 4. Measured: the armed surface the landing consumes

The consumer's arming input is #1497's designed surface (`ArmedNode`:
an `OBJECT_ACTIVE_STATE` statement with the publishing record's **own**
node vocabulary). Measured over M01's carriers, exactly six records both
publish `OBJECT_ACTIVE_STATE` (opcode 6) and carry a landing node:

| record | carries | started by |
| --- | --- | --- |
| `activate_dropoff_node` (498) | `wv_manual_land` | `WAKE_ANIM` of block 48 (`ObjectiveAwake{48}` only — **non-circular**) and of block 10 (condition includes `wv_drop_copilot RUNNING` — the landing's own output) |
| `activate_pickup_node` (500) | `wv_manual_land` | block 14 (`wv_pickup_copilot EXECUTED` — the landing's own output) |
| `deactivate_dropoff_node` (499) | `wv_manual_land` | nothing in M01's closure |
| `pzhomebase` (188) | `pz_manual_land`, `pz_auto_land` | block 17 only (`hooked_to_klondike EXECUTED` — **the landing's own output**) |
| `pzep_interior_off` (195) | `pz_manual_land`, `pz_auto_land` | nothing: not a `WAKE_ANIM` name, not one of M01's seven startup identities (`generic_intro`, `wv_hookup_state`, `pzep_engines_start`, `wvzep_engines_start`, `bszep_engines_start`, `call_add_jack`, `player_setup`) |

So the composed run has one **non-circular** arming path: block 48
(`OBJECTIVE49`, condition = its own awakeness) wakes
`activate_dropoff_node`, whose statement arms `wv_manual_land`, and a
landing there runs row 0. The pz rows are **circular under this designed
surface**: their only possible publisher starts when `OBJECTIVE18`
completes, and that block's condition is the landing's own output
(`hooked_to_klondike EXECUTED`, produced only by rows 1/2). Whether the
original armed those nodes by another rule is exactly the unknown
`m01-anim.object-active-state-arming-designed` already records; this
finding adds the composed consequence (rows 1/2 cannot fire) rather than
inventing an arming.

## 5. Where the consumer runs, and why the state appears when it does

Stage **6c** of `MissionHost::step`, immediately after stage 6b extends the
armed set with this tick's report and before the sound stage:

* it reads the player's position and each node's composed position from
  `self.world_facts.observation()` — the stage-4 fold of *this* tick, the
  sole authority for the pose;
* it reads the body's committed `LinearVelocity` and `Rotation` from the
  live world: exactly one body carries them (the same one-body rule the
  observation writer applies to the pose), otherwise no landing;
* it reads the landing table from the world's `MissionLandings` resource.

A record started at stage 6c of tick *T* publishes its statements on the
script advance **after** its start tick, so the stage-4 fold of tick *T+1*
is the first place the running record — and the gates its calls satisfy —
are observed: exactly the behaviour a `WAKE_ANIM`-started record has
(`M01-PLAY-TRIGGERS-ANIMS-ARM`'s stage-6b note). The ledger marks a row
*done* when a landing got past the window and attempted it (started or
refused): one attempt per row per generation, cleared on restart beside
`wake` and `armed`.

Fail-closed throughout: a node with no composed row, a player with no
observation row, an unarmed vocabulary, a distance beyond the radius, a
descent beyond the sink bound, or a window outside the row's numbers each
run **nothing** — absence is never a landing, and only the malformed-window
case records a refusal (once per row).

## 6. Measured: the call chain — and a disagreement the task's premise met

The rule this task was told to use: when a running record reaches its call
—or, since no payload field is read out of a `CALL_ANIMATION` event
(`animation::events`), at its start — start the names in its measured
`animation_refs` through the same activation path. The retail measurement
of that path's source, read from the carrier record's stored table
(count byte at `+226`, 72-byte entries) and from the declaration's raw
`.zrd` text:

* Record **505** (`wv_initiate_hookup`, the row-0 record) stores an
  `ANIMATION_REFS` table of exactly **four** names: `letterbox`,
  `wv_hookup_player`, `wv_pickup_copilot`, `wv_unhook_player`. This is the
  production `AnimationRecordFacts::animation_refs`, re-derived by both the
  raw record walk and the join.
* The same name's **declaration** (`wv_tailhook.zrd`) carries a
  `SEQUENCE_DEFINITION` of nine statements whose `CALL_ANIMATION` names are
  **eight**: `letterbox`, `player_extend_hook`, `wv_hookup_player`,
  **`wv_drop_copilot`**, `wv_pickup_fassenb`, `wv_pickup_copilot`,
  `wv_hookup_state`, `wv_unhook_player` (plus `STOP_ANIMATION
  wv_hookup_range`). No record's stored table anywhere in M01's closure
  names `wv_drop_copilot` (measured over both carriers).

Consequences, all verified in the composed retail member:

* The prescribed chain runs `wv_pickup_copilot` (to `EXECUTED`,
  measured duration 6.0 stored units = 720 ticks at the player's 120 Hz)
  and `wv_hookup_player`; `letterbox` binds a record but no member declares
  it, so the player refuses it and the chain records that outcome with the
  refusal's reasons on the player's own ledger; a ref that binds no record
  at all lands on the landing ledger as a refusal (the synthetic member
  pins that leg).
* **`wv_drop_copilot` is not reachable through this chain**, so
  `OBJECTIVE11`'s gate (`wv_drop_copilot RUNNING`) cannot close from a
  landing under the prescribed rule. The task's goal sentence expects it to
  run; the stored table says otherwise. This finding records the
  disagreement rather than reaching past `animation_refs` into unread event
  payloads (AGENTS rule 4): the resolving measurement is tying a
  `CALL_ANIMATION` event's payload name to its statement — the gap
  `m01-landing.call-chain-at-start-designed` names.
* **Whether the original gated a call at all is unknown.** Whether an
  animation's `ACTIVATION_PREREQUISITE` / `OBJECT_ACTIVE_LIST` is evaluated
  before a `CALL_ANIMATION` runs is explicitly unmeasured
  (`UnmeasuredFieldFamily::PrerequisiteEvaluation`,
  `f20-anim.activation-prerequisite-unevaluated`, named by the arm finding
  `2026-10-15-m01-play-triggers-anims-arm.md`). The designed chain above
  starts each name **without** evaluating any prerequisite, so a call it
  starts may in the original have been gated — or may not have been. Neither
  is claimed here; the resolving measurement is the same one that would tie
  a ref to its statement.
* `OBJECTIVE15`'s gate (`wv_pickup_copilot EXECUTED`) **does** close, and
  the composed retail member asserts exactly that: the block completes only
  after the real landing (the same frames before the placement prove the
  landing is the cause). `OBJECTIVE18`'s gate stays unreachable in a
  composed run for the §4 circularity.

## 7. The four designed rules

| claim | rule | resolving measurement |
| --- | --- | --- |
| `m01-landing.player-landing-detection-designed` | landed = stage-4 position within **150 m** of the node's composed position **and** vertical speed ≤ **10 m/s** | decode the authored `half_cone`/`sphere` extents under `land_on` as an extent, or an owner original capture of the docking approach |
| `m01-landing.row-window-designed` | a non-`auto` row admits the landing when speed magnitude ∈ `speed[0]..=speed[1]` (inclusive, the composition's m/s) and tilt (body-up vs world-up, degrees) ≤ `angle[0]`; an `auto` row has no window; any other arity is a named refusal | the table's unit and `angle`'s/`auto`'s meaning in the original |
| `m01-landing.node-position-designed` | node = observed anchor row + container offset; no anchor row → no node row | the original's join between the static container node and the live actor pose |
| `m01-landing.call-chain-at-start-designed` | every record the landing starts activates its measured `animation_refs` at its start, transitively, each identity once per landing | tie a `CALL_ANIMATION` payload name to its statement's `START_TIME` |

The call-chain row also carries an unknown it does not resolve: whether the
original evaluated `ACTIVATION_PREREQUISITE` / `OBJECT_ACTIVE_LIST` before
running a call at all (`f20-anim.activation-prerequisite-unevaluated`) is
unmeasured, so the chain's unguarded starts are designed, not measured.

## 8. What the composed runs prove

CI (synthetic, production path end to end — authored stage, authored rows,
the observation writer composing node rows from anchor+offset, the real
consumer): the parse members, the refusal members, the pure rule pin, and
`…_a_composed_landing_starts_the_row_and_its_call_chain`, which walks four
legs — too-fast pass, off-window landing, unarmed node, then the landing —
asserting one activation, its call chain, both refusal kinds each recorded
exactly once, and the two non-landing rows never running. Removing the
stage-6c call makes this member fail (mutation-checked).

Retail (`CS_GAME_DIR` + `CS_ENGINE_IMAGE`, all
`accept_m01_play_triggers_anims_land_*`):

* the reader member re-derives the three rows and the three anchor/offset
  rules from the installation;
* `…_a_retail_landing_runs_the_row_and_the_gated_state` composes the real
  M01 with blocks 48 and 14 declared awake (the arm task's own
  declared-lifecycle device), lets the real program arm the node, lands the
  player at the node the writer composed, and asserts the row ran, the
  chain started, `wv_pickup_copilot` reached `EXECUTED` and `OBJECTIVE15`
  completed;
* `…_retail_no_landing_and_off_window_landings_run_nothing` asserts the
  four negative legs on the same composed world, including that the pz
  nodes are unarmed and their row's record never starts.

## 9. Evidence

Commands (exit codes in the submit summary and in the committed report):

```sh
cargo fmt --all -- --check                                        # 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # 0
cargo test --workspace --locked                                   # 0
cargo test --workspace --locked -- accept_m01_play_triggers_anims_land_ --include-ignored  # 8 discovered, all green
```

Report per `docs/contracts/CLI-EVIDENCE.md`: harness
`evidence_report_m01_play_triggers_anims_land`, validated with
`tools/validate_evidence.py`, committed as
`docs/findings/evidence/M01-PLAY-TRIGGERS-ANIMS-LAND.json`. The report's
`unknowns` carry §4/§6/§7 exactly; `--require-pass` is not used, so the
declared unknowns stay machine-readable (owner directive of 2026-09-28).

## 10. Nothing claimed here

No original executable ran; nothing is `verified_original`; no `human_play`
or `human_review` was available to an agent. The landing geometry, the
table's units, the node-position join, the call-chain timing, whether a
prerequisite gates a call (`f20-anim.activation-prerequisite-unevaluated`),
the pz arming, and the `wv_drop_copilot` reachability are all recorded above
with their resolving paths — none of them is claimed as measured original
behaviour, and none of them was deleted to make a check green.
