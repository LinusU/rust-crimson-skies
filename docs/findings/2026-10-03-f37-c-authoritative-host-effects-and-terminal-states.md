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
  duplicate or never-allocated item ordinal, a resume cursor outside the item's
  action list, a queue that is not due-ordered, a consumed key from another
  session, a host record from another session, a foreign or duplicated host
  key, an effect recorded as both applied and outstanding, and an outstanding
  queue past its bound.
- **RNG rewind.** `SplitMix64` exposes no state getter, so the record stores how
  many draws the session took and a restore re-seeds the same domain-separated
  stream and replays them, bounded by `MAX_RNG_REPLAY_DRAWS`. A session past
  that bound is refused rather than restored approximately.
- **Deferred text in the record.** A budget stop defers the *unexecuted suffix*
  of an action list, so the resume point cannot be re-derived from program data
  alone; `ScheduledWork` carries the item's actions. Program data is immutable
  and shared, so this costs space in a record and never a second source of truth.

## Design bounds, not original limits

`MAX_RNG_REPLAY_DRAWS` (2^22 draws per restore) and `MAX_OUTSTANDING_EFFECTS`
(256 refused effects held for a retry) are engineering bounds on restore work and
record size. They are not measured original limits and must not be cited as
original behavior.

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

`accept_f37_c_*` covers AC03 (exact remaining ticks across a save/restore), the
restore refusal cases, mid-item resume at its cursor, RNG continuation,
teardown, host effect exactly-once, the retry of a refused reward, the resolved
outcome, abort-time refusal and the joint session save/restore. Each of the
snapshot, rewind, resume-cursor, exactly-once, catalog, teardown and
session-mismatch behaviours was checked by mutating the implementation and
observing the matching test fail.