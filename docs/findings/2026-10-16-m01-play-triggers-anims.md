# #1482 M01-PLAY-TRIGGERS-ANIMS: the integration step, the measured event → record join, and what stays unknown

Date: 2026-10-16. Task: `M01-PLAY-TRIGGERS-ANIMS` (Rally #1482), the parent
integration step of the two halves it was split into:
`M01-PLAY-TRIGGERS-ANIMS-ARM` (`#1497`) and `M01-PLAY-TRIGGERS-ANIMS-LAND`
(`#1498`). Capabilities used: `retail` (read-only `$CS_GAME_DIR`),
`retail` + `CS_ENGINE_IMAGE` for the composed retail members, plus
build/test. No `gpu`, no `audio`, **no original run** — nothing here is
`verified_original`; a Rally merge awards `checked` only.

This document does not repeat the two halves' findings; it records what the
parent contributes: the end-to-end acceptance, the **measured** join between a
`WAKE_ANIM` event name and the animation records the gates read, and the
complete list of what stayed designed or unknown across the three tasks.

## 1. The chain this task integrates

The owner re-scoped `#1482` on 2026-10-15 (`task.note`, Linus / Claude M3,
option (a)) after the first measurement showed no `WAKE_ANIM` event runs the
record its own block's `ANIM_STATE` gate reads. The chain the two halves build,
and the parent now checks end to end:

1. **Arm** (`#1497`). A `WAKE_ANIM` directive reaches the mission animation
   player through the host's stage-6b consumer
   (`WakeAnimationLedger::consume`) and runs exactly the record the site's
   single text argument names, resolved through the scope's measured
   name → carrier-record index. The woken record's own `OBJECT_ACTIVE_STATE`
   statements are recorded on the designed arming surface (`ArmedNode`).
2. **Land** (`#1498`). At host stage 6d the designed landing path reads the
   stage-4 observation (the player's pose and each landing node's composed
   position), the armed set and the world-group `landings.zrd` table, and — for
   a row whose node is armed, whose detection window is satisfied and whose
   landing falls inside the row's measured numbers — starts the row's record
   through the same player, with its measured `animation_refs` chain.
3. **The gate** (`#1482`). The records those two stages start publish their
   statements on the next script advance, so the **next** step's stage-4 fold is
   the first place any `ANIM_STATE` gate reads them. The host's completion pass
   then completes the block whose condition the fold satisfies.

Nothing writes a fact row directly: the host's stage-4 fold
(`MissionHost::step` step 4 over `self.animation.running()`/`finished()`) is
the only observer, exactly as the two halves document.

## 2. Measured: the `WAKE_ANIM` event → record join

Re-derived on every composed retail run and pinned by
`accept_m01_play_triggers_anims_arm_retail_the_three_woken_names_join_their_records`
(evidence class **ObservedTool**). M01's `WAKE_ANIM` sites spell four names;
the join binds them to four **distinct** carrier records of `mis_anim.zbd`
(`bind_mission_animation`, scope `zbd/c1c/m01`), through the same "exactly one
record stores the name byte for byte" rule the ARM half measured for
`startanims.zrd`:

| `WAKE_ANIM` name | carrier record | declaring member | measured duration | playable |
| --- | --- | --- | --- | --- |
| `activate_dropoff_node` | 498 | `wv_tailhook.zrd` | 0.0 (no `RUN_TIME`) | yes |
| `activate_pickup_node` | 500 | `wv_tailhook.zrd` | 0.0 | yes |
| `pzhomebase` | 188 | `pzep_hookup.zrd` | 3.0 | yes |
| `fadein_bszep` | 344 | — | 6.0 | **no** (`Undeclared`) |

The same measurement (committed at `b1c0ff5c`,
`docs/findings/2026-10-15-m01-play-triggers-anims-wake-anim-record-join.md`)
established the essential negative fact the owner's re-scope rests on: **each
`WAKE_ANIM` name is a different definition and a different carrier record from
the `ANIM_STATE` name its own block's gate reads.** The event names and the
gated names are not two spellings of one record:

| gate block | `ANIM_STATE` the gate reads | record | the block's own `WAKE_ANIM` name | record |
| --- | --- | --- | --- | --- |
| `OBJECTIVE11` (10) | `wv_drop_copilot RUNNING` | `wv_tailhook.zrd` #16 / record 511 | `activate_dropoff_node` | record 498 |
| `OBJECTIVE15` (14) | `wv_pickup_copilot EXECUTED` | #12 / record 507 | `activate_pickup_node` | record 500 |
| `OBJECTIVE18` (17) | `hooked_to_klondike EXECUTED` | `pzep_hookup.zrd` #3 / record 191 | `pzhomebase` | record 188 |

So no name-to-name mapping is invented anywhere in this chain: the `WAKE_ANIM`
consumer starts the woken definition (which arms a landing node), and the
gated records run only through a **landing** — M01's `landings.zrd` rows run
`wv_initiate_hookup` (which calls `wv_pickup_copilot` and, in its declaration's
own sequence, `wv_drop_copilot`) and `hooked_to_klondike`.

## 3. Measured: the landing table, and the one non-circular arming path

`zbd/c1c/zrdr.zbd :: landings.zrd` holds exactly three rows
(`docs/findings/2026-10-16-m01-play-triggers-anims-land.md`):

| row | `anim` | `node` | `angle` | `speed` | `auto` |
| --- | --- | --- | --- | --- | --- |
| 0 | `wv_initiate_hookup` | `wv_manual_land` | `[35]` | `[50, 220]` | – |
| 1 | `hooked_to_klondike` | `pz_manual_land` | `[35]` | `[50, 220]` | – |
| 2 | `hooked_to_klondike` | `pz_auto_land` | – | – | `[]` |

Under the designed node-arming surface exactly one arming path is
**non-circular** in a composed run: `OBJECTIVE49` (block 48) has no condition
but its own awakeness, and its `WAKE_ANIM` wakes `activate_dropoff_node`, whose
`OBJECT_ACTIVE_STATE` statement carries `wv_manual_land`; a landing there runs
row 0. The `pz` nodes arm only through `pzhomebase`, whose own block
`OBJECTIVE18` is gated by `hooked_to_klondike EXECUTED` — the landing's own
output — so rows 1 and 2 cannot fire until the original's arming rule is
recovered. This is the composed consequence of the unknown
`m01-anim.object-active-state-arming-designed`, recorded rather than bridged.

## 4. The parent's end-to-end acceptance

`crates/cs_app/tests/campaign/m01_play_triggers_anims.rs` adds two members
under the parent's own prefix `accept_m01_play_triggers_anims_`:

* **`…_the_whole_chain_closes_its_gate_only_on_the_landing`** (runs in CI, on
  the composed `synthetic_stage`). An authored control program carries a real
  `WAKE_ANIM` site (block 0) and a gate block (block 1) whose condition is
  `Condition::AnimationStates` over the name the landing's call chain starts —
  the same one-appended-pair `ANIM_STATE` shape M01's `OBJECTIVE15` spells. The
  member walks three legs and asserts:
  * armed but no landing → nothing runs, the gate's animation is never
    observed, and the gate is not `Done`;
  * a landing outside the row's speed window → nothing runs, and the gate is
    not `Done`;
  * the landing itself → the row runs, **the gate is still not `Done` on the
    tick the landing happens** (its chain has published nothing yet), and it
    closes only once its animation reaches `EXECUTED` through the production
    player, on a strictly later tick.
* **`…_retail_the_gate_closes_only_after_the_real_landing`** (ignored without
  `CS_GAME_DIR` + `CS_ENGINE_IMAGE`). The same three legs over the composed
  retail M01: `OBJECTIVE49`'s own `WAKE_ANIM` arms `wv_manual_land`, nothing
  lands there (never placed / placed outside the row's window), and
  `OBJECTIVE15` — whose gate is `wv_pickup_copilot EXECUTED` — stays open,
  including on the tick the real landing runs `wv_initiate_hookup`, and closes
  only after the row's measured chain reaches that state through the player.
  `OBJECTIVE49` completes, no `pz` row runs, and `hooked_to_klondike` is never
  observed.

Both members read the gate through the host's own lifecycle model
(`ObjectiveLifecycles::state`) and the animation state through the host's own
stage-4 fold; neither writes a fact row. The two halves' findings record the
mutation checks that show removing the stage-6d landing consumer, the stage-6b
wake consumer or the name join makes the chain stop; the parent members read
the very same states through the very same production path, so the same
removal fails them.

## 5. What is designed, and what is unknown

Everything below is recorded, with its claim id and its resolving measurement;
none of it is claimed as the original's behaviour, and none was removed to make
a check green (owner directive of 2026-09-28).

| # | what | claim | resolving measurement |
| --- | --- | --- | --- |
| 1 | the node-arming rule (`OBJECT_ACTIVE_STATE` arms a node) | `m01-anim.object-active-state-arming-designed` | evaluate `ACTIVATION_PREREQUISITE` / `OBJECT_ACTIVE_LIST`, and decode the event payload's node field |
| 2 | which node one statement addresses (an `ArmedNode` carries the record's whole vocabulary) | same as 1 | the same payload decode |
| 3 | what counts as a landing (radius 150 m, vertical speed ≤ 10 m/s) | `m01-landing.player-landing-detection-designed` | decode the `half_cone`/`sphere` extents under `land_on`, or an owner original capture of the docking approach |
| 4 | what a row's `angle`/`speed`/`auto` numbers gate | `m01-landing.row-window-designed` | the table's unit and the meaning of `angle`/`auto` in the original |
| 5 | how a node's world position joins the static container to the live actor's pose | `m01-landing.node-position-designed` | the original's join between the container node and the bound actor |
| 6 | when a started record's `animation_refs` fire (this chain starts them at the record's start) | `m01-landing.call-chain-at-start-designed` | tie a `CALL_ANIMATION` payload name to its statement's `START_TIME` |
| 7 | whether an animation's `ACTIVATION_PREREQUISITE` / `OBJECT_ACTIVE_LIST` gates a `CALL_ANIMATION` at all | `f20-anim.activation-prerequisite-unevaluated` | the same payload/statement measurement as #6 |

Composed consequences that are **not** claimed as closable gates:

* **`OBJECTIVE11`'s gate (`wv_drop_copilot RUNNING`) is not reachable through
  the measured chain.** Record 505 (`wv_initiate_hookup`) stores an
  `ANIMATION_REFS` table of four names — `letterbox`, `wv_hookup_player`,
  `wv_pickup_copilot`, `wv_unhook_player` — while its declaration's own
  sequence calls eight, among them `wv_drop_copilot`. No record's stored table
  names `wv_drop_copilot` anywhere in M01's closure, so the prescribed chain
  never starts it. The prescribed chain runs `wv_pickup_copilot` (row 0's
  gate) instead.
* **`OBJECTIVE18`'s gate (`hooked_to_klondike EXECUTED`) stays unreachable in a
  composed run** for the §3 circularity: its only publisher starts when the
  block completes.
* **`fadein_bszep` cannot be played by this join**: it matches carrier record
  344 but no member declares it (it is a `NAME1` state of `bswan_gasbags.zrd`),
  so the `WAKE_ANIM` consumer refuses it as `Undeclared`. Whether the original
  resolved it through that `NAME1` state is unmeasured.

`OBJECTIVE15`'s gate (`wv_pickup_copilot EXECUTED`) is the one gate the parent
acceptance closes end to end, and it closes only through the player's real
landing.

## 6. Evidence

Commands (exit codes in the submit summary and in the committed report):

```sh
cargo fmt --all -- --check                                        # 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # 0
cargo test --workspace --locked                                   # 0
cargo test --workspace --locked -- accept_m01_play_triggers_anims_ --include-ignored
```

The fourth selection discovers the parent's own members **and** the two halves'
(`…_arm_`, `…_land_`), because the parent's prefix is theirs: the integration
step re-runs every assertion the chain owns, on one commit.

Report per `docs/contracts/CLI-EVIDENCE.md`: harness
`evidence_report_m01_play_triggers_anims`, validated with
`tools/validate_evidence.py`, committed as
`docs/findings/evidence/M01-PLAY-TRIGGERS-ANIMS.json`. Its second artifact is a
second production observation that re-derives the join table of §2 and the
landing rows of §3 from `$CS_GAME_DIR` through the production readers. The
report's `unknowns` carry §5 exactly; `--require-pass` is not used, so the
declared unknowns stay machine-readable.

## 7. Nothing claimed here

No original executable ran; nothing is `verified_original`; no `human_play` or
`human_review` was available to an agent. `retail` is read access to the
owner's installation, not proof the original ran. Every designed rule and
unresolved issue in §5 stays recorded with its resolving path; a Rally merge
awards `checked` only.
