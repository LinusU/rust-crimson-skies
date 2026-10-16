# M01-PLAY-AI-WAKE (#1487): consuming M01's `WAKEUP_ENEMIES` wake

Date: 2026-10-16. Task: **#1487 `M01-PLAY-AI-WAKE`**, the follow-up
`mimo-1` filed in #1436's handover (M01-PLAY-AI, unmet criterion 4).
Capabilities used: `retail` (read of `$CS_GAME_DIR`). **No original
executable was run**; nothing here is `verified_original`. Every measured
claim below was re-read through this repository's own production readers.

This note records what the slice measured, what it designed and what it left
unknown, so a reviewer can tell the three apart without re-deriving them.

## The gap it closes

#1435 spawns M01's `bsfury_1` with its measured `starts_asleep` (#681) as a
held kinematic body, and #1436 gives every **awake** scripted record the
designed pursuit-and-fire law but gives a sleeping one **no law at all**
(`docs/findings/2026-10-15-m01-play-ai-designed-law.md` §3: "`WAKEUP_ENEMIES`
names it, but the directive has no consumer in the runtime yet"). This task
adds that consumer: when the control program emits the wake, the sleeping
record is **released** into exactly the body and law an awake record spawns
with.

## 1. Measured, and used verbatim

| Input | Where it comes from |
| --- | --- |
| The `WAKEUP_ENEMIES` operation and its measured semantics — wake each named **vehicle** whose asleep byte is set, else each named **zeppelin** whose dormant byte is set, and do nothing for a name already awake | `cs_content::mission_control::MeasuredDirective` (findings B/C/D of the M01-LC directive stage) |
| M01's own site: `OBJECTIVE13` spells `WAKEUP_ENEMIES [text bsfury_1, text blackswanzep]`, one site, applied on wake | `crates/cs_app/tests/accept_m01_lc_directive_c.rs` pins the shipped arguments; `accept_m01_lc_directive_e.rs` pins the operation per key |
| `bsfury_1` is the one record M01's `deactivated` field sleeps | `docs/findings/2026-10-15-m01-aiv-layout.md`, #681, and `M01-PLAY-ACTORS`'s census |
| The record's `team`, `primary_target`, stored pose and name-prefix dispatch | #1453 (`docs/findings/2026-10-15-m01-aiv-layout.md`) |
| The imported `bsfury` airframe (`vehicle.zrd` → `basic_airplane → fury → bsfury`) and every flight parameter the law consumes | the retail import (#796), provenance `OWNER-STATIC-2026-10-08` |
| The "already awake → do nothing" arm | the same measured directive semantics; the consumer implements it as a no-op, neither a release nor a refusal |

The release's one measured body mapping is the one #1436 already documents for
a spawn: mass = imported weight over the law's force scale (`W / 9.82`),
`NoAutoMass`, the recovered law on the AI seat (`is_player: false`), zero
initial velocity, and the AI behaviour the record's measured fields select.

## 2. Designed, with the value and why

Everything in this section is project design, and each item names what a
future measurement would replace it with.

| Item | Value | Why |
| --- | --- | --- |
| The behaviour a woken record takes (`woken_behaviour_for`) | `behaviour_for`'s reading of the same measured fields with the `starts_asleep` term removed | The wake consumed the sleep, so the record is no longer asleep; the rest of the reading (`team` vs the `player` record's team, the stored `primary_target`) is #1436's designed reading, unchanged. `bsfury_1` (team 2, target `player`) therefore takes `Pursue { Player }`. |
| The release is the body swap | `set_body_mode(Dynamic)` plus the same `Mass`/`NoAutoMass`/law/`MissionAircraftAi` an awake spawn attaches | #1436's spawn already decides "held kinematic body" for a sleeper and "dynamic AI body" for an awake record; the release is the awake half of that decision, run on the live body instead of a fresh one (`BodySpec` documents that a kinematic spec meaning to be released declares its mass so the release can). |
| The wake runs in `MissionHost::step`, after the script advance | stage 6c, beside the existing `WAKE_ANIM` consumer (stage 6b) | The directive log is the host's own record; the ledger that deduplicates it (by execution key, exactly as the animation ledger does) is host state and is reset on restart. The release is visible to the arm/decide pass on the **next** fixed tick. |
| A name that is neither a sleeping scripted aircraft nor an awake one is **refused by name** | `WakeEnemiesLedger::refused` | The measured effect's zeppelin arm (dormant byte, route) has no consumer here, so a name that resolves to a zeppelin (`blackswanzep`) is named rather than silently treated as a vehicle. Missing an aircraft airframe or a live body pose are likewise named, never half-released. |
| The release's timing bound | the record is released on the tick the host consumes the emission; the acceptance fly proof drives 3 s (180 ticks) | The original's release pose and post-wake behaviour are unmeasured, so the bound proves the record flies rather than matching any original number. |

## 3. Unknown, and left unknown

* **The original AI control law, `0x48c220`.** A released record flies the
  same designed pursuit-and-fire law every awake record flies; the original's
  post-wake behaviour is unrecovered. Gates: any fidelity claim about how a
  woken aircraft flies. Resolving task: static analysis of `0x48c220`, or an
  owner original run (#358).
* **The ±5 % AI parameter roll** (`0x4771c6`, #805): not applied, so a woken
  airframe flies the exact retail values.
* **What a `team` number means engine-wide, and the original's use of a
  stored `primary_target`** (M01-AIV-LAYOUT unknowns 4 and 5): the behaviour
  the woken record takes reads those measured fields with #1436's designed
  meaning.
* **The zeppelin half of `WAKEUP_ENEMIES`.** `blackswanzep`'s dormant byte
  and route are a world-actor concern; the name is refused by name. Affected
  content: the award zeppelin M01's `OBJECTIVE13` also wakes. Resolving task:
  a world-actor wake surface (F35-D).
* **The original's release pose and pose continuity.** The held body is
  released from the posed it spawned at, at rest; whether the original kept
  that pose or wrote a start velocity is unmeasured (the AI spawn speed is
  unrecovered, #796).

## 4. Where the tests live

* `crates/cs_app/src/mission_ai.rs` — `accept_m01_play_ai_wake_*` unit tests
  over the pure behaviour split and the site's argument shape. CI runs them:
  they need no installation.
* `crates/cs_app/tests/campaign/m01_play_ai_wake.rs` — the retail acceptance
  tests, `#[ignore = "requires CS_GAME_DIR"]`, driven with
  `--include-ignored` locally and by the reviewer, plus one **CI** synthetic
  member (authored stage, sleeping record, wake site) that proves the release
  and the fly path without an installation.
* `crates/cs_app/tests/evidence_report_m01_play_ai_wake.rs` — the evidence
  harness for `docs/contracts/CLI-EVIDENCE.md`; not prefixed.

The consumer's two seams in landed code are the new `WakeEnemiesLedger` field
on `MissionHost` (with its `wake_enemies()` accessor and its restart reset)
and the one call in `MissionHost::step` stage 6c. The release itself reuses
#1436's `ai_flight_record` / `spawn_mass_kg` / `MissionAircraftAi` and
`crates/cs_app/src/physics/body.rs`'s `set_body_mode`, so there is one body
mapping, one integrator and one command boundary.

## 5. Evidence

`private/evidence/M01-PLAY-AI-WAKE/` holds `acceptance.json`, the acceptance
`cargo-test.log` and the second-observation `wake-release.json`; the
committed copy is `docs/findings/evidence/M01-PLAY-AI-WAKE.json` (hashes and
paths only, no original content). The report declares the still-open
questions of §3, so `tools/validate_evidence.py … --require-pass` reports
`Unresolved issues` (exit 3) — deliberately, exactly as
`docs/findings/evidence/M01-PLAY-AI.json` records for its own pinned
unknowns; the owner directive of 2026-10-05 forbids removing an unresolved
issue from machine-readable evidence just to pass a validator. The
structural check without `--require-pass` passes.
