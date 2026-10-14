# F29-C.3b: the kill-score ledger's two wirings

Date: 2026-10-14. Task: F29-C.3b "Call the kill-score ledger from the mission
host and hand its score to the run outcome" (#704). Spec:
`specs/F29-damage-zones-armor-destruction-and-bailout.md`, section `### F29-C`
(scoring). Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
Capabilities used: ordinary build/test only — no `CS_GAME_DIR` read, no render,
no audio, so no `private/evidence/` report is produced. Preceding stage:
`docs/findings/2026-10-05-f29-c-3-kill-award-scoring.md` (#519, the consumer).

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/score.rs` (**new**): `SessionScoreLedger` (the Bevy
  resource around `cs_sim::net_state::NetStateLedger`), `OutcomeRecordError`,
  and the two builders `mission_outcome` / `mission_result`.
- `crates/cs_app/src/lib.rs` (wiring): `pub mod score;`.
- `crates/cs_app/src/damage.rs` (extend): the schedule step that hands each
  accepted batch to the ledger, `DamageScheduleReport::score`,
  `DamageScheduleMissing::ForeignScoreLedger`.
- `crates/cs_app/src/mission_session/terminal.rs` (extend):
  `MissionTerminal::score` and its line in the report line.
- `crates/cs_app/src/mission_session/host.rs` (extend): `settle` reads the
  ledger back, `MissionHostRefusal::ScoreLedger`, the fresh ledger a restart
  opens, and `mission_host_tick` ordered behind the damage consumers.
- `crates/cs_app/src/mission_session/compose.rs` (extend): the F27 weapon step
  and the F29 damage consumers installed for the composed mission, the ledger
  opened for the session the host serves, removed by `teardown`.
- `crates/cs_app/tests/campaign/f29_c_3b_score.rs` (**new**): 3
  `accept_f29_c_3b_` tests; `tests/campaign/main.rs` links it.
- This file.

No protected path, no `cs_sim`/`cs_content` change, no original data, no
binary.

**One observable failure, before the change:** #519 delivered
`NetStateLedger::apply_kill_awards`, but the only constructor of a ledger was
the F57 `NetSession` and tests — so no single-player resolution ever reached
the consumer (`DamageScheduleReport` had no score field at all), and
`campaign::MissionOutcome::score` / `ui::scrapbook::MissionResult::score` were
two fields nobody computed: a finished run had no score to record.

## The chosen owner, and why there is still only one score store

`cs_app::score::SessionScoreLedger` is a **wrapper, not a second store**: it
holds the one `NetStateLedger` and adds only the ECS handle (`Resource`) —
`cs_sim` must not depend on Bevy (`AGENTS.md` rule 7) — plus the session
identity every call is checked against. The tally, the destruction record, the
per-attacker credit and the provenance-carrying `KillScoreValue` stay where
#519 put them; `cs_app` adds no counter, no set and no dedup of its own.

Two wirings were left open by #519 and both are now made:

| wiring | where it lives | what moves |
| --- | --- | --- |
| 1. the host that applies a resolution hands its batch to the ledger | `damage::drive_damage_schedule`, the F29-C consumer schedule the composed mission now installs | the session's score, once per victim |
| 2. a finished run's score reaches its outcome records | `MissionHost::settle` → `MissionTerminal::score` → `score::mission_outcome` / `score::mission_result` → `mission_records::apply_mission_record` and `scrapbook::record_mission` | `MissionOutcome::score`, `MissionResult::score`, once per `OutcomeId` |

### Wiring 1: the damage schedule is the host

`drive_damage_schedule` drains each committed tick's published
`DamageTickBatch`es and, for a ledger whose `SessionId` matches the installed
`SessionDamageResolver`, calls `apply_kill_awards` on the same merged batch
the collider pass gets. The `KillAwardReport` travels on
`DamageScheduleReport::score` (`None` = no ledger installed, which is also the
honest statement that nothing could have scored); a ledger still serving a
torn-down generation is never written to and is named as
`DamageScheduleMissing::ForeignScoreLedger`.

The **composed mission** previously installed neither the weapon step nor the
damage consumers (`PhysicsSession::production_builder` did, the windowed
composition did not), so nothing in a mission would ever have run that code.
`mission_session::compose` now installs the pair exactly as the production
session declares them, opens the ledger for `host.served()`, and
`install_mission_host` orders `mission_host_tick` **behind**
`drive_damage_schedule` — a run that settles on a tick settles on the score
that tick's kills resolved, which is what makes wiring 2 deterministic.

### Wiring 2: read back, never recomputed

`MissionHost::settle` reads `SessionScoreLedger::score()` for the session it
serves and puts it on `MissionTerminal::score` (the report line prints it).
A world with no ledger for that generation — or with one still holding another
generation — had *necessarily* scored nothing for this run (nothing else
scores), so the number is `0` and the absence is named under
`MissionHostRefusal::ScoreLedger` instead of looking like a run that scored
nothing by itself.

`cs_app::score::mission_outcome` / `mission_result` then fill
`campaign::MissionOutcome::score` and `ui::scrapbook::MissionResult::score`
from that terminal; both records keep the caller's `OutcomeId`, so the
exactly-once rule is the one the outcome transaction already enforces —
`CampaignState::apply_outcome` (`applied` set) and `RecordBook::submit`
(`AlreadyApplied`) — and the reconciled entry point
`cs_sim::mission_records::apply_mission_record` moves both stores together.

## Designed value, unchanged and still provenance-carrying

The original's per-kill scoring values remain **unmeasured** (F29 "Research
boundary"). Nothing here invents a number: the score is the ledger's
`KillScoreValue::designed()` under claim `f29-c3.designed-kill-score` (one
point per kill), read back through `ledger.kill_score()`, and a caller that
later measures a value still replaces it with
`NetStateLedger::set_kill_score`. No new designed rule was added to the score
value; the only new designed *decisions* are structural and named here:

1. **A settlement reads the score after the same tick's damage consumers** —
   ordering in `install_mission_host`, not a scoring rule.
2. **A missing/foreign ledger reads as `0` at settle** — correct because
   nothing but that ledger can move a number, and named under
   `MissionHostRefusal::ScoreLedger`.
3. **`TerminalOutcome::Extraction` is refused by name** in both builders
   (`OutcomeRecordError::Extraction`): what an extraction records as a
   campaign/scrapbook outcome is unmeasured, so no `Succeeded`/`Aborted`
   default was invented. Affected content: every extracting run of every
   mission. Resolving task: **#1427**.

## Tests

All three are in `crates/cs_app/tests/campaign/f29_c_3b_score.rs` and drive
production code: the composed mission (`build_headless`), the real
`DamageResolver` with the AC01 hit pair, the installed damage schedule,
`MissionHost::settle`, and the two record entry points.

| test | what it pins |
| --- | --- |
| `accept_f29_c_3b_a_kill_resolved_during_a_session_moves_the_score_once` (minimum) | the composition opens the ledger for its session; `drive_damage_schedule` scores the resolved batch once (`report.score`, tally, `scored_kills`), and the identical batch again is `AlreadyAwarded` and moves nothing |
| `accept_f29_c_3b_the_run_carries_its_score_once_per_outcome_id` | the batch pushed before the first frame reaches the ledger through the **installed** schedule before the host settles: `terminal.score == ledger.score`; `mission_outcome`/`mission_result` carry it; `apply_mission_record` raises the campaign best and the book slot once, and the replay answers `AlreadyApplied` in both stores with `victories == 1`; `record_mission` behaves the same over the same `MissionResult` |
| `accept_f29_c_3b_a_restarted_session_starts_from_zero` | the composed restart opens a fresh ledger for the generation the host now serves (session matches, score `0`, no destruction record carried over), and the rebuilt session's own kill scores exactly one kill's worth again |

Selection: `cargo test --workspace --locked -- accept_f29_c_3b_ --include-ignored`
(3 tests, no `#[ignore]`: none needs original data).

## Mutation probes

Each probe edited one production line, ran
`cargo test -p cs_app --test campaign --locked -- accept_f29_c_3b_`, recorded
the failing tests and restored the file from git (the tree was clean
afterwards).

| probe | edit | result |
| --- | --- | --- |
| the wiring is removed | the score block in `drive_damage_schedule` deleted | **all 3** failed (0 passed): `report.score` stayed `None`, the tally never moved, the terminal carried `score=0` |
| the read-back is removed | `settle`'s ledger read replaced by `let score = 0;` | only `..._the_run_carries_its_score_once_per_outcome_id` failed (2 passed) — exactly the criterion the read-back owns |
| the fresh ledger on restart | the replace-if-session-changed block in `mission_host_restart` deleted | only `..._a_restarted_session_starts_from_zero` failed (2 passed): the ledger still scored the torn-down `SessionId(3)` while the host served `SessionId(4)` |
| the composition opens no ledger | `SessionScoreLedger::open(host.served())` in `compose` deleted | **all 3** failed (0 passed) |

## Follow-ups left open (filed with `create_tasks`)

- **#1426 — the spawn host must admit the actors it spawns.**
  `record_kill_award` refuses a victim the ledger does not know
  (`NetStateError::UnknownActor`, reported by name in
  `DamageScheduleReport::score`), and no production path today spawns a damage
  actor in a single-player mission: `SessionDamageResolver`, `WeaponSession`
  and the `DamageActorBinding` spawn path are installed only by tests. The
  admission call (`SessionScoreLedger::admit`) exists and is exercised by the
  tests as the host's own duty; VS-M01 (#359) makes it real when it opens the
  weapon session. Nothing fabricated an authoritative `NetActorState` to paper
  over the gap.
- **#1427 — what an extraction records.** `OutcomeRecordError::Extraction`
  refuses the mapping; see the designed decisions above.

## Evidence

Synthetic fixtures and designed contracts only: the harbor stage, the
`synthetic_airframe_graph` AC01 pair and the synthetic campaign of
`declared_synthetic_campaign`. No original-data, visual, audible or
ordinary-play claim; this stage can award at most **checked**. The retail M01
members of the campaign suite were run only if `CS_GAME_DIR` is present on the
machine; they do not touch this change's subject (M01 settles no terminal
headlessly).

## Sources

- `specs/F29-damage-zones-armor-destruction-and-bailout.md` (`### F29-C`,
  scoring), `docs/contracts/STATE-TRANSACTIONS.md`.
- `docs/findings/2026-10-05-f29-c-3-kill-award-scoring.md` (#519: the
  consumer, the designed value, the two wirings it left).
- `docs/findings/2026-10-13-f43-v2-campaign-scrapbook-record-reconciliation.md`
  (`mission_records::apply_mission_record`, the one entry point that moves
  both stores).
- `crates/cs_app/src/damage.rs` (the F29-C schedule),
  `crates/cs_app/src/mission_session/{host,terminal,compose}.rs`,
  `crates/cs_sim/src/net_state.rs`, `crates/cs_sim/src/mission_records.rs`.
