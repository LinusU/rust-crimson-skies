# F37-B: bounded evaluator and event queues — design limits

The F37-B evaluator (`cs_script::runtime`, driven by `cs_sim::mission`) is a
new engine design. Nothing in it is measured from the original game.

## What this slice added

- `Action::Schedule { delay_ticks, actions }` enqueues a deferred work item;
  `delay_ticks == 0` appends to the queue being drained this tick.
- `Action::Reschedule { delay_ticks }` re-queues the action list containing
  the action — the objective's list, or the scheduled item's list. A
  zero-delay `Reschedule` is the AC02 case: it schedules itself.
- `Action::Draw { variable, min, max }` writes a uniform `i32` from the
  mission's explicit `SplitMix64` stream (`for_domain(session, "MSN_EVAL")`),
  satisfying the contract's "explicit RNG" without ambient entropy.
- `MissionState` now holds the pending queue (`BTreeMap<Tick, VecDeque>`),
  a session-unique item-ordinal counter, `WorkLimits` and the RNG stream.
- Every objective firing, pending dequeue and action execution spends one
  unit of `max_work_per_tick` (default 4096). Stored scheduled items are
  capped by `max_pending_items` (default 4096). `Schedule` nesting is bounded
  at validation (`MAX_ACTION_NESTING = 16`).

## Stop semantics (designed, not measured)

- A bound hit is a `TickResult.stop: Option<StopReason>`, never a mission
  failure and never an error: the interrupted action list is re-queued at its
  next action and resumes on a later tick; work already spent stays
  committed. Nothing is skipped and nothing is repeated (contract: "do not
  continue with arbitrary skipped instructions").
- `WorkBudget` re-queues at the front (strict resume order). `PendingLimit`
  (a `delay > 0` schedule against a full store) retries the failed action and
  the item goes to the back, so items still queued for the tick can run and
  free the cap.
- A budget stop inside an objective latches the objective and emits its
  completion event: it *did* fire; only its actions are deferred.
- Event-key layout: an objective's own events use sequence `0..=64`; a
  scheduled item's events are packed as `(ordinal + 1) * 65 + index + 1`, so
  items from one source never collide and a session can exhaust the space
  only after ~66 million items (`SequenceExhausted`).

## Unknowns, none resolved by this task

- Whether the original runtime has any comparable work bound, queue shape or
  "schedule" vocabulary is unknown; the IR is explicitly a new design
  (F37-A finding).
- Tick ordering between objectives and due pending items (objectives resolve
  first, then the queue drains in (due, enqueue) order) is a designed policy.
- The seed for the evaluator stream is currently the session generation;
  threading the run root seed is F37-C/host-integration work.
- Snapshot/restore of `pending`, `consumed`, `next_item_ordinal` and `rng`
  is F37-C (AC03). `MissionState` derives `Clone`, so the fields are already
  clone-consistent for that work.

## Constants are design bounds

`MAX_WORK_PER_TICK`, `MAX_PENDING_ITEMS`, `MAX_ACTION_NESTING` and the RNG
domain constant are arbitrary but fixed engineering bounds. They are not
measured original limits and must not be cited as original behavior.
