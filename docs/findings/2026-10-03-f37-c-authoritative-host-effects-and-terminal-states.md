# F37-C: authoritative host effects, terminal states and mid-mission save

The F37-C slice (`cs_script::runtime` save record, `cs_sim::mission` host
ledger) is a new engine design. Nothing in it is measured from the original
game, and no original opcode, reward or precedence rule is claimed here.

## What this slice added

- `cs_script::runtime::MissionStateSnapshot`: a versioned save record of the
  mutable execution state. It carries variable values, the latched objectives,
  every consumed execution key, the terminal state, the last evaluated tick,
  the whole pending queue in `(due, enqueue)` order with its eligibility ticks,
  the item-ordinal counter, the work limits and the RNG draw count.
  `MissionState::snapshot` / `MissionState::restore` move that record.
- `MissionState::pending_timers` and `MissionStateSnapshot::pending_timers`
  expose the same `PendingTimer` view (due tick, exact remaining ticks, resume
  cursor, action count) for live state and for a record, so AC03 ("save/restore
  at a pending timer preserves the exact remaining ticks") is observable on both
  sides of the restore.
- `MissionState::teardown` drops the deferred work queue; `MissionState::abort`
  settles the session as `Aborted` and tears it down. Neither is idempotence-
  unsafe: a session that already resolved an outcome keeps it.
- `cs_sim::mission::HostLedger`: the authoritative consumer. It owns the
  exactly-once ledger over `ExecutionKey`s, the catalog of rewards it may
  apply, the session's one resolved outcome, the outstanding refusals and the
  teardown tick.
- `MissionSession::advance` is the wired path: `MissionState::step` is the
  producer, `HostLedger::apply` the consumer, and a terminal state tears the
  session down before `advance` returns. `MissionSession::step` stays available
  for a caller that only wants to evaluate the program.
- `MissionSession::snapshot` / `MissionSession::restore` move the evaluator
  record and the host record together, because a reward the host already applied
  must not be applied again after the restore.

## Designed policies, not measured behavior

- **Reward catalog.** `MissionSession::launch` takes the reward ids the host may
  apply. A `GrantReward` outside that catalog is *refused* with
  `HostFault::UnknownReward` and held for a retry, never silently honoured:
  a mission may declare a reward the installation cannot grant, and an
  unappliable grant must surface as an error. No namespace or kind rule is
  inferred — F37-B's own tests use `ContentKind::Blueprint` for a reward, so
  any kind rule here would be invented.
- **Retry.** A refused reward keeps its execution key outstanding until it is
  applied or the record is dropped. Re-offering the same result while an effect
  is outstanding is a no-op and reports nothing: the effect's fate is already
  decided, and `HostLedger::retry` is the only thing that changes it. This is
  what closes the hole where the evaluator consumed the key at emission but the
  host refused it: the intent is still the session's to hand over.
- **Terminal precedence.** Both `TerminalRequested` events of a tick that
  requests conflicting outcomes are recorded as applied *requests*, and the
  outcome the evaluator's `PrecedencePolicy` resolved is what the ledger settles
  on. A second, different outcome for a session that already settled is refused
  with `HostFault::OutcomeConflict`. The precedence rule itself is still the
  unmeasured synthetic conservative policy inherited from F37-B.
- **Teardown.** A terminal or aborted session refuses every *newly offered*
  effect (`HostFault::AfterTeardown`) and drops its deferred work. Effects
  already earned before teardown stay outstanding for a retry: refusing them is
  the one thing teardown does not do, because an earned reward must not
  evaporate with the session. Whether an original aborted mission still paid
  out is unknown.
- **Restore refusals.** A record is refused whole, with its defect named:
  another snapshot version, another mission, a variable or objective the program
  no longer declares, a value whose type the program does not declare, a
  declared variable the record is missing or holds twice, a queued item whose
  action list this program would refuse at launch, a duplicate or never-allocated
  item ordinal, a resume cursor outside the item's action list, a queue that is
  not due-ordered or is past the queue cap, a consumed key from another session,
  a host record from another session, a foreign or duplicated host key, an effect
  recorded as both applied and outstanding, and an applied, outstanding or
  catalog collection past its bound.
- **A record may tighten the engine's bounds, never lift them.** The work and
  queue limits are host policy, so they travel in the record and survive a save.
  A restore clamps them to `MAX_WORK_PER_TICK` / `MAX_PENDING_ITEMS`, so a save
  file cannot talk the evaluator out of the bounds the process guarantees.
- **RNG rewind.** `SplitMix64` exposes no state getter, so the record stores how
  many draws the session took and a restore re-seeds the same domain-separated
  stream and replays them, bounded by `MAX_RNG_REPLAY_DRAWS`. A session past
  that bound is refused rather than restored approximately.
- **Deferred text in the record.** A budget stop defers the *unexecuted suffix*
  of an action list, so the resume point cannot be re-derived from program data
  alone; `ScheduledWork` carries the item's actions. Program data is immutable
  and shared, so this costs space in a record and never a second source of truth.
- **Retry order.** `HostLedger::retry` applies and reports in execution-key
  order whatever order the refusals were held in, because the contract fixes
  ordering keys and not arrival order. A refusal that stays outstanding is handed
  back in arrival order, which is what the record stores.

## Design bounds, not original limits

`MAX_RNG_REPLAY_DRAWS` (2^22 draws per restore), `MAX_OUTSTANDING_EFFECTS`
(256 refused effects held for a retry), `MAX_APPLIED_EFFECTS` (2^16 applied
effects in one host record) and `MAX_REWARD_CATALOG` (2^12 catalog ids in one
host record) are engineering bounds on restore work and record size. The last two
are unreachable for a live session — every applied effect cost one bounded work
unit — so they refuse a record only. They are not measured original limits and
must not be cited as original behavior.

## Review findings (bunny-2, review of ee45cee5)

The review of the submitted commit found the save record trusted in places its
own documentation claimed it was checked. Each was reproduced first as a probe
against the submitted code and then fixed:

- A queued item's action list comes out of the record, not out of the program, and
  was never validated. A record carrying `Action::Unknown` in a deferred item
  **aborted the process** on the next tick (the evaluator's
  `unreachable!("validated program")`), a record carrying `Draw { min: 1, max: 0 }`
  **aborted it** with a modulo by zero, and a record carrying a write to an
  undeclared variable made the restored session **unsaveable** — its next record
  held a variable the program does not declare, which the next restore refuses.
  Restore now validates every queued list with a new
  `ValidatedProgram::validate_actions`, the same rules launch validation applies.
- A record could drop or duplicate a declared variable. A dropped one is not
  inert: a condition on an absent variable is false, so the mission could be
  restored into a state where it can never reach its own objectives. Both are
  refused now.
- A record could lift the engine's work and queue bounds to `u64::MAX` /
  `usize::MAX`, which the live path's caps exist to prevent. A restore clamps
  them to the engine's maxima; a host-tightened budget still survives a save.
- A record could carry a pending queue longer than the cap the live path
  enforces, and a host record an unbounded applied ledger or catalog. All three
  are refused now.
- `HostLedger::retry` reported in arrival order while its documentation claimed
  execution-key order. It sorts by execution key now; the refusals that stay
  outstanding are still handed back in arrival order.
- `HostFault::OutcomeConflict` and `HostOutcome::SessionRefused` were documented
  behaviours with no test. Both arms are covered now.

The review's own regression tests are named `accept_f37_c_*` like the rest, and
each fix was mutation-checked: reverting it individually makes the matching test
fail.

## Unknowns, none resolved by this task

- Whether the original runtime has any comparable save record, reward catalog,
  retry rule, teardown policy or terminal precedence is unknown. The IR and its
  runtime are explicitly a new design (F37-A and F37-B findings).
- The evaluator's RNG is still seeded from the session generation, not from the
  run's root seed; threading the run seed is host-integration work that belongs
  with the application layer, which does not own this slice.
- Whether an original mission that aborts or is torn down still delivers the
  rewards it had already earned is unmeasured. The designed policy keeps them.
- The original mission reward vocabulary, its ids and whether a reward can fail
  to apply at runtime are unknown; the catalog here is whatever the caller
  declares, and a refusal is the visible outcome of a mismatch.

## Acceptance

`accept_f37_c_*` (16 tests) covers AC03 (exact remaining ticks across a
save/restore), the restore refusal cases — foreign, corrupt, unrunnable deferred
work, missing or duplicated variable, out-of-bounds queue and bounds, RNG replay
past the bound — mid-item resume at its cursor, RNG continuation, teardown, host
effect exactly-once, the retry of a refused reward and its ordering, the resolved
outcome and the refusal of an outcome that contradicts it, abort-time refusal and
the joint session save/restore. Each of the snapshot, rewind, resume-cursor,
exactly-once, catalog, teardown, session-mismatch, deferred-action,
missing-variable, queue-bound, bounds-clamp, host-record-bound, retry-order and
outcome-conflict behaviours was checked by mutating the implementation and
observing the matching test fail.

## Follow-ups filed

- `SplitMix64` exposes no state getter, which is why the record stores a draw
  count and a restore replays it (`MAX_RNG_REPLAY_DRAWS`,
  `RestoreError::RngReplayTooLong`). `SplitMix64::new` is public, so one getter in
  `cs_types::random` would let the record store the state itself and make the
  restore O(1). Filed separately; `cs_types` is outside this stage's owner paths.
- The evaluator's RNG is still seeded from the session generation rather than the
  run root seed; that is `F37-C-FU1` (#581), already open.