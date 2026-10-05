# F29-C.3: wiring `KillAwarded` into the score

Date: 2026-10-05. Task: F29-C.3 "Wire KillAwarded into the scoring and
progression consumer" (#519, re-homed from #107's title). Spec:
`specs/F29-damage-zones-armor-destruction-and-bailout.md`, section
`### F29-C` (AC01's scoring half). Shared contract:
`docs/contracts/STATE-TRANSACTIONS.md`. Capabilities used: ordinary
build/test only — no `CS_GAME_DIR` read, no render, no audio, so no
`private/evidence/` report is produced.

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/net_state.rs` (extend): the score consumer —
  `KillScoreValue` / `KillScoreError`, `KillAward`,
  `KillAwardReport`, `NetStateLedger::record_kill_award`,
  `NetStateLedger::apply_kill_awards`, `score` / `score_for` /
  `scored_kills` / `kill_score` / `set_kill_score`, the private
  `DestructionRecord` (the destruction record now also says *when* this
  session scored it), the `NetStateError::ScoreOverflow` refusal, and the
  module's new "# The kill is scored exactly once" section.
- `crates/cs_sim/src/lib.rs` (doc only): one paragraph in `net_state`'s
  crate-level description naming it the session's kill-score consumer.
- `crates/cs_sim/tests/accept_f29_c_kill_award.rs` (**new**): 5
  `accept_f29_c_kill_*` tests over the real resolver → consumer path.
- This file.

No protected path, no `cs_content` change, no original data, no binary.

**One observable failure, before the change:** F29-A made
`DamageEventKind::KillAwarded` the single scoring event of a destruction
(AC01), and F29-C wired the *part* and *visual* consumers — but nothing
recorded that award. A session that shot an actor down kept a score of
zero: `NetStateLedger` recorded the destruction and never moved a number,
`objectives::counters` counts destroyed actors for objective conditions
and is not a score, and the outcome-time progression transactions
(`campaign::CampaignState::apply_outcome`, `records::RecordBook`) only
ever see the score somebody else was supposed to have tallied. The
producer's event stream ended in nothing.

## The chosen owner, and why it is not a second score store

The task names three candidate consumers — `cs_sim/src/net_state.rs`,
`cs_sim/src/objectives/counters.rs` and the progression/reward
transactions — and asks for the real owner.

- **`objectives/counters.rs`** keys actors by `cs_script::ir::ActorId`,
  the mission-IR key space, which carries **no session generation**
  (`ActorFactTable`'s docs make the same point about the bridge).
  "Refuse a previous generation's award" is therefore not expressible
  there, and `CountKind` answers "how many actors left this category",
  not "what is this session's score". Its `from_lifecycle(PilotBailout)
  == None` is the rule this consumer *reuses in spirit*, not the store.
- **The progression/reward transactions** are outcome-time by
  construction: `HostLedger` is keyed by `ExecutionKey` and
  `CampaignState::apply_outcome` / `RecordBook::submit` by `OutcomeId`,
  both applied once per mission result. A kill is a per-tick, per-actor
  event; forcing it through those would mean minting a fabricated
  `ExecutionKey`/`OutcomeId` per kill, which is exactly the guessing the
  contract forbids.
- **`net_state::NetStateLedger`** already answers the question this
  feature needs answered, once per session-qualified `(actor,
  generation)`: `Destruction::awarded()` — "the first report … the kill
  is awarded here" — with `NetStateError::ForeignSession` for another
  generation and `NetStateError::AlreadyTerminal` for an actor that ended
  as something other than a destruction (a bailout is not a kill). Its
  keys are `cs_types::net::ActorId`, the session-qualified id the task
  names.

So the award rides the **one** destruction record the process already
keeps: `DestructionRecord` now carries `scored: Option<Tick>` beside its
tick, which is the same record's answer to "has this kill been paid?".
There is no second deduplication set, and `forget` still bounds the whole
record by actor lifetime (the credited-attacker tally is dropped with the
attacker too). That is what "do not build a parallel score store" means
here: one record, two questions, one owner.

## The designed behavior

`apply_kill_awards(&[DamageEvent])` walks one resolution batch and is the
only path from the producer to the score; `record_kill_award(victim,
credited, tick)` is the single awarding entry point behind it.

| input | what the score does |
| --- | --- |
| `KillAwarded` for a victim this session never scored | scores once (`KillAward::Awarded`), records the destruction it implies |
| a second `KillAwarded` for that victim — replayed batch, duplicate emission, same tick or later | absorbed (`KillAward::AlreadyAwarded { first }`), score unchanged |
| an event whose `EventId.session` is another generation | refused whole (`NetStateError::ForeignSession`) *before* anything is applied |
| an award naming a victim or a credited attacker of another session | refused (`ForeignSession`) — the id itself carries the session |
| `Lifecycle { PilotBailout }` (or any non-award event) | not a score event: ignored, no outcome, no refusal |
| a `KillAwarded` for an actor that already ended without one (bailout, despawn) | refused (`AlreadyTerminal { lifecycle: BailedOut }`), score unchanged |
| an award that would push the tally past `i64` | refused (`ScoreOverflow`) before anything is written |

Session generations (`STATE-TRANSACTIONS`): a restart or an aircraft swap
is a new generation with new actor serials, so an award from the old one
cannot be applied — it is refused by name — and no id of that generation
exists in this ledger's map to double-award. Refusals are decided before
any mutation, so a refused report leaves the score and the record exactly
as they were.

**Designed values, not original data.** The original's per-kill scoring
values are **unmeasured** (F29 "Research boundary"; nothing in the manual
or guides states a kill's worth). The value therefore travels as a
provenance-carrying `KillScoreValue { points, claim }`:
`KillScoreValue::designed()` is **1 point per kill** under claim
`f29-c3.designed-kill-score` — a counter of the kills the resolver
already decided happened, with no multiplier, grade or bonus invented —
and `NetStateLedger::set_kill_score` lets a caller that has measured a
value replace it (the setter hands the previous value back; a negative
value is refused at the value's own boundary, `KillScoreError::NegativePoints`).
The installation's only numeric score table, the `score_*` multiplayer
match table of `player.zrd` (`cs_content::stunts::SCORE_CONFIG_MEMBER`,
entries `score_kill`, `score_zep`, …), was measured in F42-D; nothing
measured binds it to a kill in a mission, so this consumer does not read
it and does not pretend to.
**Affected content:** every kill in every mission and scenario — the
tally a run's score will be built from. **Resolving tasks:** F29-D keeps
the original-family gate; the host that owns a ledger and hands it a
resolution (below) is F57/VS-M01's.

## Tests

Every test drives production code: the real `DamageResolver` with the
`synthetic_airframe_graph` fixture and the F29-A AC01 hit pair produces
the award, and `NetStateLedger::apply_kill_awards` /
`record_kill_award` consume it.

| test | what it pins |
| --- | --- |
| `accept_f29_c_kill_one_kill_awards_exactly_once` (minimum) | two same-tick lethal hits produce one `KillAwarded`; delivering the batch scores once, moves `score`, credits the attacker's session-qualified id and records the destruction; the same batch again is absorbed and moves nothing |
| `accept_f29_c_kill_a_same_tick_double_kill_does_not_double_award` | a second `KillAwarded` for the same victim *on the same tick* is `AlreadyAwarded { first }` and pays 0; a third delivery of the original batch still pays 0 |
| `accept_f29_c_kill_a_foreign_session_generation_is_refused` | an award stamped by another generation is refused by name with `ForeignSession { expected, found }`, changes neither the score nor the victim's record, and does not poison the ledger — this generation's own award still scores afterwards |
| `accept_f29_c_kill_a_bailout_awards_nothing` | a `PilotBailout` batch scores nothing and is neither award nor refusal; a `KillAwarded` arriving after the bailout is refused with `AlreadyTerminal { BailedOut }` (the refusal is what makes this test fail without the wiring) |
| `accept_f29_c_kill_value_carries_its_provenance` | the default value carries its claim id and points, a caller-supplied value is the one awarded, and a negative value is refused |

Selection: `cargo test --workspace --locked -- accept_f29_c_kill_
--include-ignored` (5 tests). The stage selection `accept_f29_c_` runs
these 5 plus #107's 8 `accept_f29_c_damage_consumers` tests in `cs_app`.

## Mutation probes

(Recorded after the suite ran; each probe edited one production line,
ran the task selection, recorded the failures and restored the file.)

| probe | edit | result |
| --- | --- | --- |
| _pending_ | _pending_ | _pending_ |

## Follow-ups left open (filed with `create_tasks`)

- **The host that owns a ledger must call it.**
  `NetStateLedger::apply_kill_awards` is the consumer seam; the only
  construction sites of a `NetStateLedger` today are the F57 network
  session (`cs_app::network::physics::NetSession`) and tests. A mission
  host that applies a local resolution must hand its batch to the ledger
  — that host (VS-M01 "Wire one original mission into the playable
  application", #359) is outside this task's owner paths, so the seam is
  delivered wired-and-tested and the session call is its to make.
- **The run's score at outcome time.** `campaign::MissionOutcome::score`
  and `ui::scrapbook::MissionResult::score` are outcome-time and nobody
  computes them yet; the session score this ledger totals is the input
  they are waiting for. Same owner paths problem, same follow-up.

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F29-damage-zones-armor-destruction-and-bailout.md` (`### F29-C`,
  AC01, non-negotiable 2/3), `docs/contracts/STATE-TRANSACTIONS.md`.
- `docs/findings/2026-09-30-f29-a-damage-graphs-hit-ordering-lifecycle.md`
  (the once-per-actor `KillAwarded` this consumer receives).
- `docs/findings/2026-10-02-f29-c-damage-consumers.md` (the follow-up this
  task re-homed; its state-driven consumer pattern).
- `docs/findings/2026-10-01-f47-a-scrapbook-records.md` and the F42-D
  reward survey (`docs/findings/2026-10-03-f14-d-8-stunt-scrapbook-collections.md`)
  for what a score value may and may not claim.
- `crates/cs_sim/src/net_state.rs`, `crates/cs_sim/src/damage/{events,resolver}.rs`,
  `crates/cs_sim/src/objectives/counters.rs`, `crates/cs_sim/src/mission.rs`,
  `crates/cs_sim/src/records.rs`.
