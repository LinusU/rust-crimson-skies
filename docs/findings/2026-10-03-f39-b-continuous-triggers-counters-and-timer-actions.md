# F39-B: continuous triggers, counted conditions, timer actions and the declared outcome latch

Status: designed behavior, **not** original-verified. Code: `cs_sim::objectives`
(`runtime`, `timer`, `terminal`), acceptance:
`crates/cs_sim/tests/accept_f39_b_objective_runtime.rs` (21 tests, prefix
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
- **A timer runs once per declared start, and performs its action once.** The
  action belongs to the *expiry*, not to the `Expired` state the timer then sits
  in: a deadline that ran out on tick 4 does not re-grant its reward, re-raise
  its signal or re-request its wave on ticks 5, 6 and 7. An automatic start is
  consumed by the tick that took it, so a one-shot `AtTick` declaration does not
  emit a refusal on every later tick either; a repeated `OnSignal` is still
  refused and reported, because "you already had this" is worth saying. Only an
  explicit `TimerRequest::Arm` runs a deadline again.
- **A refused tick changed nothing.** All of a tick's movements are validated
  before any is swept — including a watched actor listed twice, which is only
  detectable once the first listing has moved the trigger, so it is caught in
  validation rather than mid-tick — the bound is checked against the tick's
  *declared* facts before any effect, and a `StopReason::EventBudget` tick is not
  marked as stepped. There is no partial application and no silently skipped
  work.
- **A named reference is never dropped.** A timer request or objective request
  naming a declaration the runtime does not have applies nothing and is reported
  as `RequestRefused`, so "the program asked for a deadline" and "the world did
  nothing" are distinguishable from the stream alone. A mission whose deadline
  never existed must not look like one that simply never came due.
- **Stable instance ids.** An admitted spawn group allocates this session's ids
  from a monotonic per-session counter; a refused repeat consumes none of them,
  so the next group's ids follow the previous group with no gap.
- **A retry is a new generation.** The runtime owns every piece of its mutable
  state, so a new `SessionGeneration` and a new runtime carry nothing over. This
  is the *precondition* for F39-C's own AC03 ("retry after several waves"); F39-C
  still has to prove the wiring tears down.

## Measured sensitivity of the acceptance suite

Nine mutations were applied and reverted; each is caught by a named test. The
first four are the ones the implementer measured:

| mutation | caught by |
| --- | --- |
| `TerminalPrecedence` ranks `Success` first instead of `Failure` | `…protected_actor_destroyed_on_the_completion_tick_uses_declared_precedence`, `…precedence_is_order_independent_and_names_the_loser` |
| a trigger entry implicitly arms every `OnArm` deadline (non-negotiable 3 removed) | `…a_crossing_reports_and_only_a_declared_arm_moves_the_world` |
| the spawn admission pre-check removed, so a refused repeat burns instance ids | `…a_signal_arms_a_timer_once_and_a_repeat_does_not_replay_it` |
| the `RevealRule` filter removed, so any event reveals any objective | `…reveal_is_the_only_way_to_show_an_objective_and_a_reward_is_not_an_outcome` |

The remaining five are the review pass (see below), and each was applied and
reverted and measured to fail:

| mutation | caught by |
| --- | --- |
| the action list re-derived from the timer table, so every expired timer replays its action on every later tick | `…an_expired_timer_performs_its_declared_action_exactly_once`, `…a_signal_arms_a_timer_once_and_a_repeat_does_not_replay_it` |
| the `NotArmed` filter dropped from the automatic arms, so an `AtTick` start is retried every tick | `…an_expired_timer_performs_its_declared_action_exactly_once` |
| the repeated-actor guard dropped from movement validation | `…a_tick_that_lists_a_watched_actor_twice_is_refused_whole` |
| the `RequestRefused` guards reverted to a silent `return` | `…a_request_naming_nothing_is_reported_not_dropped` |
| the reserved-symbol check dropped from `add_condition` | `…every_declaration_is_checked_for_a_collision` |

Removing `crates/cs_sim/src/objectives/runtime.rs`, `timer.rs` or `terminal.rs`
does not compile the acceptance file at all.

## Review pass, 2026-10-03 (bunny-alpha-2)

Reviewer and implementer are the same agent instance, so this is **not**
independent evidence; it is a same-session audit. It found five real defects and
one dead branch, all fixed in the same branch. None of them is an original-fidelity
question; all five are engine correctness.

1. **An expired timer replayed its declared action on every later tick.**
   `apply_timers` re-derived the action list from the timer table filtered by
   `TimerState::Expired`, but an expired timer *stays* expired. A deadline that
   ran out on tick 4 therefore re-granted its `GrantOptionalReward`, re-raised
   its `Signal` and re-requested its `SpawnGroup` on ticks 5, 6, 7 … The
   `EmissionLedger` hid the repeat for spawns and cues and nothing else, and
   `TimerState::Expired`'s own doc ("its action was performed once") and the
   sheet's non-negotiable behavior 4 were both false. A `Signal` action made it
   worse than a cosmetic repeat: it re-armed its own `OnSignal` deadline's
   successor forever. The fix takes the action list from the timers that expired
   *this* tick.
2. **A one-shot `AtTick` start was retried on every tick.** `auto_arms_on` is
   `tick >= at`, so once the declared start had been taken, every later tick
   re-attempted the arm, got `AlreadyExpired` and emitted a `TimerRefused` —
   an unbounded event stream from a declaration that is supposed to run once.
   The automatic arms are now filtered to timers still `NotArmed`, which is what
   "armed automatically at the first evaluated tick at or after this tick" means.
3. **A tick listing a watched actor twice was refused *mid-tick*.**
   `validate_movements` checked finiteness and tick advance but not repetition, so
   the first listing advanced the trigger and the second made `observe` return
   `NotAdvancing` — from phase 3, after the counter and condition phases had
   already applied. `step` returned `Err` with the destruction already counted
   and the tick *not* marked stepped, so a retry re-ran on top of half-applied
   state. The doc claimed "a refused tick changed nothing at all". Now refused in
   validation, with `TriggerError::RepeatedActor`.
4. **A request naming an undeclared timer or objective was dropped silently.**
   `arm_timer`, the cancel path and `change_objective` all `return`ed on a missing
   declaration and emitted nothing, so a mission whose deadline did not exist was
   indistinguishable from one that had not come due. A test named
   `…timer_requests_are_applied_or_reported_never_ignored` asserted this never
   happens while not covering it. Now reported as `RequestRefused`.
5. **`add_condition` accepted the reserved `ACTOR_EVENT_SOURCE`.** Every field of
   `CountCondition` is public, so a struct literal bypassed `CountCondition::new`
   and its reserved-symbol refusal, and a condition could claim the source a
   counted actor event reports under. The check is now in `add_condition` too, and
   the other two degenerate shapes (`EmptyRoster`, `ZeroRequired`) are refused
   there as well rather than only by the constructor.
6. **`ObjectiveEventKind::OutcomeRefused` was unreachable.** `step` returns before
   `resolve_outcome` whenever the latch already holds an outcome, and
   `resolve_outcome` is the only place the latch can settle, so no tick could ever
   produce it. The variant and its branch were removed rather than left as a
   diagnostic no consumer can observe; the tick that finds a settled latch reports
   `StopReason::OutcomeSettled` instead.

Also corrected: `declared_event_count` claimed to be an upper bound. It is an
estimate of the tick's *declared* work used to refuse an obviously unbounded tick
before anything is applied, and a reaction's own cascade (the state change, the
reveal it triggers, the deadlines that state arms) is covered by the separate
objective and timer terms, so a tick may legitimately produce fewer events than it
reports. The doc now says so.

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
- **Cross-references are checked when they are used, not when they are
  declared.** A `RevealRule::OnCondition`/`OnTimer`, a `TimerStart::OnSignal`/
  `OnObjectiveState` or a `TimerAction::SetObjectiveState` naming a declaration
  that does not exist simply never fires; there is no declaration pass that
  reports it up front. So an objective whose reveal condition latched *before*
  the objective was declared never becomes visible. Declaring everything before
  the first `step` — which is what F39-C's lowering does — makes this
  unreachable, but it is a rule a future caller could break silently.
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
