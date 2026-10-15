# M01-PLAY-TRIGGERS-ANIMS-ARM (#1497): the join, the `WAKE_ANIM` consumer and the designed arming surface

Date: 2026-10-15. Task: `M01-PLAY-TRIGGERS-ANIMS-ARM` (Rally #1497), the
arming half of parent #1482. Capabilities used: `retail` (read-only
`$CS_GAME_DIR`), `retail`+`CS_ENGINE_IMAGE` for the composed member, plus
build/test. No `gpu`, no `audio`, no original run. Implementer:
`bunny-alpha-2/bunny-alpha-2`. Nothing here is `verified_original`; a Rally
merge awards `checked` only.

This is the implementation half of #1482. The measurement it must not
contradict is
`docs/findings/2026-10-15-m01-play-triggers-anims-wake-anim-record-join.md`
(committed at `b1c0ff5c`, carried onto this branch): M01's `WAKE_ANIM` events
name **different records** than the same block's `ANIM_STATE` gates read, the
woken record arms a landing node, and the landing table runs the gated records
later. Nothing in this task starts a gated record, and no `ANIM_STATE` gate is
closed here.

## What this task adds

### 1. The name → carrier-record join surface (`crates/cs_app/src/animation/mission.rs`)

`MissionAnimationBinding` now carries a measured **name → carrier record**
index (`MissionAnimationBinding::names`, an `AnimationNameIndex`), built inside
`bind_mission_animation` over the same two carriers the startup identities are
joined with — the scope's own `mis_anim.zbd` first, its world group's
`cam_anim.zbd` second.

* `CarrierRecords` keeps one carrier's own record table (the production record
  walk's `anim_name`s, in carrier order) and the bytes a matched record is
  re-read from; the facts of a matched record are decoded on demand by the
  production walk (`own_record`), so a name nobody asks about is never decoded.
  A carrier whose walk refused keeps **no** table, so every name stays
  `NOT_WALKED` there.
* `match_record_by_name(carriers, name)` is the whole match rule: exactly one
  record between the two tables stores the name byte for byte
  (`RecordFacts::indices_named`), else `None`, `Ambiguous` (every match
  reported, in mission-then-camera order) or `NotWalked`. It is the same
  "exactly one" rule `carrier::bind_startup_identities` measured for
  `startanims.zrd`; nothing is normalized and no nearest match is taken.
* `join_named_animation(name, record, declarations, world)` joins the matched
  record with the closure's declaration for that name through
  `join_startup_animation` — the same function the startup rows use — with the
  name as the row's identity and the measured directive code
  (`WAKE_ANIMATION_SOURCE = DirectiveOperation::WakeAnimation.code() =
  "wake_animation"`) as its activation source. Every refusal a startup row can
  carry (undeclared name, object/sequence disagreement, playback gap) is
  produced here too.
* `AnimationNameIndex::resolve` is the one entry point: `Bound(row)` or
  `Unbound { reason, matches }` with the existing
  `UNBOUND_REASON_NO_RECORD` / `UNBOUND_REASON_AMBIGUOUS` /
  `UNBOUND_REASON_NOT_WALKED` texts, verbatim.
* `MissionAnimationBinding::authored` / `AuthoredMissionAnimations` is the seam
  a synthetic stage uses to state authored carriers, declarations and rows; it
  builds the index through the very same rules, so a synthetic stage exercises
  the production join on content it authored.

### 2. The player's name-keyed activation (`crates/cs_app/src/mission_animations.rs`)

`MissionAnimationPlayer::activate(at, row)` is the startup activation minus the
event filter: one activation per identity (`ActivationOutcome::AlreadyRunning`
otherwise), refused rows kept in the **same** refusal ledger, RUNNING until the
record reaches its measured duration and then EXECUTED, published through the
same `TickReport`. `start` and `activate` share one private `begin`, so a
startup row and a name-keyed row can never be started by two different rules.
Nothing writes `MissionFacts::animations` or any fact row: the host's stage-4
fold stays the only observer.

### 3. The `WAKE_ANIM` consumer (`crates/cs_app/src/mission_session/host.rs`)

`MissionHost::step` gains a documented **stage 6b**, after the script advance
(stage 6) and its completion pass, before the sound stage:

* `WakeAnimationLedger::consume(state, names, player, tick)` walks
  `MissionState::directives()` and applies every `DirectiveOperation::WakeAnimation`
  emission whose **execution key** the ledger has not already applied — the same
  exactly-once rule `cs_sim::mission::Countdown::apply_new` uses for the timer
  directives — in the log's own `EventKey` order.
* The name is the site's own single text argument (`[Value::Str(name)]`, the
  measured shape; any other shape is a refusal with
  `WAKE_ANIM_ARGUMENTS_REASON`, never an interpretation). The name resolves
  through the host's own `AnimationNameIndex` (cloned from the stage's binding
  at launch, rebuilt on restart) and the record starts at the tick being
  stepped.
* An unresolvable name is a `WakeRefusal` carrying the join's own reason and
  its matches; a name whose row the join refuses is a `WakeRefusal` with
  `WAKE_ANIM_ROW_REFUSED_REASON` **and** the player's own refusal ledger. No
  silent skip, no different record.

**Which tick the state appears on.** The consumer runs after this tick's
`advance`, so the record is not advanced on its start tick. Its start-time-0
statements publish on the **next** advance, and stage 4 of that same next step
is the first place the fold can observe the record. A retail record with a
positive measured duration is therefore observed `RUNNING` for
`ceil(duration × ticks_per_second)` ticks and `EXECUTED` after that; a record
whose measured duration is zero (no `RUN_TIME` in any of its statements) goes
from its activation straight to `EXECUTED` on the first advance, and is never
observed `RUNNING`.

### 4. The designed node-arming surface (same module)

`armed_nodes_from_report(&TickReport, &AnimationNameIndex)` reads the committed
tick's published statements whose **installation spelling** is
`OBJECT_ACTIVE_STATE` and records an `ArmedNode` for each: the publishing
record's identity, the statement spelling and opcode, the committed tick, the
**publishing record's own node-reference names** (verbatim, in stored order —
entry `0` is the empty name every measured table starts with), and the record's
own bytes as a `SourceSpan`. `MissionHost::armed_nodes()` is the landing half's
input.

**This rule is designed, not measured** (claim
`m01-anim.object-active-state-arming-designed`). Two things stand between it
and a real arming rule, and both are recorded, not bridged:

* whether the original evaluates an animation's `ACTIVATION_PREREQUISITE` /
  `OBJECT_ACTIVE_LIST` before a `CALL_ANIMATION` is explicitly unmeasured
  (`UnmeasuredFieldFamily::PrerequisiteEvaluation`,
  `f20-anim.activation-prerequisite-unevaluated`), and
* the event payload's node field is not decoded
  (`animation::events` reads two timing words and nothing else), so **which**
  node one `OBJECT_ACTIVE_STATE` statement addresses is unmeasured.

What the surface therefore records is measured (the statement, its tick, the
publishing record and its own node vocabulary); what it does **not** claim is
that the original armed those nodes from those statements. No fidelity claim in
this report depends on it.

## Re-deriving the measured facts

```sh
cargo test --workspace --locked -- accept_m01_play_triggers_anims_arm_ --include-ignored
```

Six tests: four run in CI (three on authored content, one on the composed
synthetic host), two are `#[ignore = "requires CS_GAME_DIR…"]` retail members.

* `..._the_name_join_binds_one_match_and_refuses_the_rest` (CI) — the match
  rule over authored `RecordFacts`: one match binds, none is `NO_RECORD`, two
  are `AMBIGUOUS`, a refused walk is `NOT_WALKED`; the bound row's identity and
  activation source, its duration and its record node table; and the join over
  no declarations is `Undeclared`.
* `..._a_named_row_agrees_like_a_startup_row` (CI) — a record whose
  `object_name` disagrees with its declaring member is refused with
  `object_name_disagrees`, exactly as a startup row would be.
* `..._synthetic_wake_directive_runs_the_record_through_the_fold` (CI) — a
  composed synthetic stage whose control program carries two `WAKE_ANIM` sites:
  the stored name is applied exactly once and started, the unbound name is
  refused with `UNBOUND_REASON_NO_RECORD`, the fold observes the record
  `EXECUTED`, and its `OBJECT_ACTIVE_STATE` statement reaches the arming
  surface with the record's own node table.
* `..._a_woken_record_is_running_before_executed` (CI) — the order: `RUNNING`
  is observed before `EXECUTED`.
* `..._retail_the_three_woken_names_join_their_records` (retail) — M01's real
  closure bound by `bind_mission_animation`:

  | name | carrier | record | declaring member | measured duration | playable |
  | --- | --- | --- | --- | --- | --- |
  | `activate_dropoff_node` | `mis_anim.zbd` | 498 | `wv_tailhook.zrd` | 0.0 | yes |
  | `activate_pickup_node` | `mis_anim.zbd` | 500 | `wv_tailhook.zrd` | 0.0 | yes |
  | `pzhomebase` | `mis_anim.zbd` | 188 | `pzep_hookup.zrd` | 3.0 | yes |
  | `fadein_bszep` | `mis_anim.zbd` | 344 | — | 6.0 | **no** (`Undeclared`) |

  `fadein_bszep` is the fourth `WAKE_ANIM` name of M01's record: the name
  matches carrier record 344, but no member declares it (it is the `NAME1`
  state of `bswan_gasbags.zrd` the #1482 finding measured), so the join refuses
  the row as undeclared. A name nothing stores
  (`m01_nothing_binds_this_name`) is refused with `UNBOUND_REASON_NO_RECORD`.
  The two carriers' tables hold 880 records.

* `..._retail_a_woken_record_runs_through_the_host_fold` (retail) — M01's real
  stage and **real control program**, launched through the production
  `MissionHost::launch`, stepped on a bare world through the production
  `MissionHost::step`. `OBJECTIVE11` (block index 10) and `OBJECTIVE18` (17)
  are declared awake, and the test supplies the two gate rows
  (`wv_drop_copilot RUNNING`, `hooked_to_klondike EXECUTED`) through the same
  `MissionWorldObservation` port `mission_session::observation` writes, so
  M01's own `WAKE_ANIM` sites run. Then: both emissions are M01's own
  directive sites, the host consumed each exactly once, `pzhomebase` is
  observed `RUNNING` and then `EXECUTED` through the fold, the zero-duration
  `activate_dropoff_node` is observed `EXECUTED` and never `RUNNING`
  (pinning the difference instead of pretending the two records behave alike),
  and `activate_dropoff_node`'s `OBJECT_ACTIVE_STATE` statement reaches the
  arming surface with its record's own node table.

  **What the test supplies, and why**: the two gate states are the landing
  path's outcome (M01's `landings.zrd` rows run `wv_initiate_hookup`, which
  calls `wv_drop_copilot`, and `hooked_to_klondike`), and the landing half is
  M01-PLAY-TRIGGERS-ANIMS-LAND (#1498). No gate is closed by supplying them;
  what the member measures is that a **real directive** starts its record and
  the fold observes it. The two blocks' lifecycles are declared awake for the
  same reason: the measured wake chain is #1498's subject.

The evidence report is produced by
`crates/cs_app/tests/campaign/evidence/m01_play_triggers_anims_arm.rs`, its
second artifact being a census that resolves the four names over the
installation, and is committed as
`docs/findings/evidence/M01-PLAY-TRIGGERS-ANIMS-ARM.json`. It carries two
unresolved issues (the designed arming rule, and #1498 owning the gates), so it
is validated with the plain structural form of `tools/validate_evidence.py`
(exit 0), never `--require-pass`.

## What stays unknown and what follows

* **The arming rule is designed** (claim
  `m01-anim.object-active-state-arming-designed`): no claim here says the
  original armed a node from an `OBJECT_ACTIVE_STATE` statement, and none
  depends on it.
* **Which node one statement addresses** is unmeasured: the event payload's
  node field is not decoded, so an `ArmedNode` carries the record's whole node
  vocabulary, not the one node.
* **The gates stay open**: M01's `OBJECTIVE11`/`OBJECTIVE15`/`OBJECTIVE18`
  read records this task never runs. **M01-PLAY-TRIGGERS-ANIMS-LAND (#1498)**
  owns the landing detection, the `landings.zrd` reader and the measured call
  chain; the parent **#1482** returns afterwards as the integration step.
* **`fadein_bszep` cannot be played by this join**: no member declares it, so
  `WakeAnimation` refuses it as `Undeclared`. Whether the original resolved it
  through the `NAME1` state the #1482 finding measured is unmeasured.
* The retail fold member's two gate rows are test-supplied world state, as its
  doc says; they are not evidence that the gates close.
* No original executable ran: `retail` is read access to the owner's
  installation, and nothing here is `verified_original`.
