# F39-B: continuous triggers, counted conditions, timer actions and the declared outcome latch

Status: designed behavior, **not** original-verified. Code: `cs_sim::objectives`
(`runtime`, `timer`, `terminal`), acceptance:
`crates/cs_sim/tests/accept_f39_b_objective_runtime.rs` (16 tests, prefix
`accept_f39_b_`).

Acceptance case this stage owns: **AC02** — "Destroy a protected actor on the
same tick as completing an objective; use declared terminal precedence."

## What F39-A left and what this stage adds

F39-A shipped the vocabulary, each piece usable alone: the seven
`ObjectiveState`s, `SweptTrigger`, `ActorCounters` and `EmissionLedger`. The
*continuous* part is F39-B and it is three new modules plus one continuous
driver:

- `runtime::ObjectiveRuntime` — one tick's facts in, one `EventKey`-ordered
  event stream out, in a declared phase order (counters, conditions, triggers,
  signals, objectives, timers, outcome).
- `runtime::CountCondition` / `CountReaction` — a *declared roster* plus a
  *declared single category* plus a *declared consequence*.
- `timer::MissionTimer` — a declared `TimerStart`, a validated gameplay
  `TimeDomain`, and exactly one `TimerAction` on expiry.
- `terminal::{TerminalOutcome, TerminalPrecedence, TerminalLatch}` — distinct
  endings, a declared tie-break, and a one-way latch.

## Designed rules (synthetic only)

- **Declared phase order.** Effects are applied in a fixed order and the
  returned stream is sorted by `cs_script::runtime::EventKey`
  (`session, tick, source, sequence`) — the contract's "not hash map or entity
  iteration order". Reproducibility is a test, not a claim.
- **No reentrancy.** A mission signal raised this tick becomes eligible to arm a
  `TimerStart::OnSignal` deadline on the *next* tick, and a signal is consumed by
  the tick that observes it. Two timers cannot chase each other in one tick.
- **Terminal precedence is order-independent.** A tick's requests are collected
  into a set and resolved once by `TerminalPrecedence`; the winning request's
  source becomes the `OutcomeSettled` event's source and the losers are reported
  in `superseded`. Two producers asking in opposite order agree.
- **The latch is one-way, and it ends the work.** Once an outcome is settled,
  `step` applies nothing and answers `StopReason::OutcomeSettled`. This is the
  contract's *"a protected actor destroyed after a success latch may or may not
  change the outcome"* turned into a stated rule instead of an assumption: on
  this engine it cannot. **Which outcome wins a same-tick collision is
  unmeasured**; `SyntheticConservative` (Failure > Extraction > Success) is the
  designed policy the contract permits for synthetic tests only, and it is named
  `Synthetic…` so a measured rule becomes a *new* variant instead of an edit.
- **A protected actor is a declared roster, not a heuristic.** A count condition
  names the actors, names the one `CountKind` that satisfies it, and names its
  `required` count; an empty roster or a zero requirement is refused at
  declaration. There is no operation in this module that turns "no enemies
  alive" into an outcome, and `counted(kind)` never returns a total across
  categories.
- **A completion is a fact; the mission ending is a declaration.** An objective
  completes because a declared `CountReaction` or `TimerAction` moved it to
  `Succeeded`; whether that requests `Success` is `ObjectiveCompletion`. A
  mission *failure* is declared on the condition that watches for it. This split
  is what makes AC02 expressible as two competing **requests** rather than a
  fact and a guess.
- **A waypoint unlocks nothing by itself.** A crossing emits an event; the only
  thing that can start a deadline is a declared `TimerRequest::Arm` (or the
  timer's own declared start). F39 non-negotiable behavior 3 is a test.
- **Reveal rules gate the objective, not just its label.** A hidden objective
  leaves `Hidden` only through its own `RevealRule`, which moves it to `Pending`;
  a declared action that tries first is **refused and reported**. A declaration
  that pairs `Immediate` with a `Hidden` initial state is refused.
- **Deadlines count whole committed ticks only.** `TickInput` has no wall-time
  field, so a paused frame cannot shorten a deadline — the case is not
  expressible. A non-gameplay `ClockPolicy` domain is refused at declaration.
- **A timer runs once per declared start.** An automatic start refuses an
  already-expired timer, so a repeated signal cannot replay a wave or a radio
  line; only an explicit `TimerRequest::Arm` may run it again.
- **A refused tick changed nothing.** All of a tick's movements are validated
  before any is swept, the bound is checked against the tick's *declared* facts
  before any effect, and a `StopReason::EventBudget` tick is not marked as
  stepped. There is no partial application and no silently skipped work.
- **Stable instance ids.** An admitted spawn group allocates this session's ids
  from a monotonic per-session counter; a refused repeat consumes none of them,
  so the next group's ids follow the previous group with no gap.
- **A retry is a new generation.** The runtime owns every piece of its mutable
  state, so a new `SessionGeneration` and a new runtime carry nothing over. This
  is the *precondition* for F39-C's own AC03 ("retry after several waves"); F39-C
  still has to prove the wiring tears down.

## Measured sensitivity of the acceptance suite

Four mutations were applied and reverted; each is caught by a named test:

| mutation | caught by |
| --- | --- |
| `TerminalPrecedence` ranks `Success` first instead of `Failure` | `…protected_actor_destroyed_on_the_completion_tick_uses_declared_precedence`, `…precedence_is_order_independent_and_names_the_loser` |
| a trigger entry implicitly arms every `OnArm` deadline (non-negotiable 3 removed) | `…a_crossing_reports_and_only_a_declared_arm_moves_the_world` |
| the spawn admission pre-check removed, so a refused repeat burns instance ids | `…a_signal_arms_a_timer_once_and_a_repeat_does_not_replay_it` |
| the `RevealRule` filter removed, so any event reveals any objective | `…reveal_is_the_only_way_to_show_an_objective_and_a_reward_is_not_an_outcome` |

Removing `crates/cs_sim/src/objectives/runtime.rs`, `timer.rs` or `terminal.rs`
does not compile the acceptance file at all.

## Unknown / deferred

- **Everything about the original game.** Which terminal outcome wins a
  same-tick collision; which deadline a mission declares, in which domain, armed
  by what, performing which action; which actors a mission protects and what
  losing one does; the original reveal rules; the original event ordering inside
  a tick. All unmeasured; F39-D calibrates them with `retail` capability. No
  retail data was read and no claim here is a fidelity claim.
- **The authored content form.** `cs_content::objectives` — the provenance
  carrying record with source spans for objectives, trigger volumes, count
  rosters, timer declarations and reveal rules, plus the lowering boundary in
  `cs_app::objectives` — is **not** in this stage. This slice is the runtime
  those records would feed; F39-C is the wiring stage and owns both. The
  runtime's declaration API is the contract F39-C lowers into:
  `add_objective` / `add_condition` / `add_trigger` / `add_timer`, and
  `TickInput` is the whole producer surface (lifecycle transitions, real
  per-tick movement segments, signals, timer requests, objective requests,
  terminal requests).
- **Disabled and escaped actors still have no producer.** `CountKind::Disabled`
  and `CountKind::Escaped` remain reachable only by a caller that reports them
  directly; no lifecycle source produces them (unchanged from F39-A).
- **Trigger shapes beyond sphere and AABB** are still unmeasured.
- **Mid-mission snapshot/restore.** `ObjectiveRuntime` owns session-generation
  state but does not implement `MissionStateSnapshot`-style save/restore; the
  contract's "state snapshot/restore must preserve all gameplay-relevant pieces"
  is unmet for this module and belongs to F39-C's wiring, where the save
  boundary already exists for `cs_script::runtime::MissionState`.

## Follow-up filed

- The disabled/escaped actor-state producers and the `cs_content::objectives`
  form are named in the F39-C description; nothing here blocks F39-C from
  starting.
- The mid-mission save gap for this module's state is recorded above and is
  F39-C's to close together with the existing save boundary.
