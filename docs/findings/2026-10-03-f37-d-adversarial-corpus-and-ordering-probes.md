# F37-D: adversarial mission-runtime corpus and reference ordering probes

The F37-D corpus (`cs_script` acceptance tests, `cs_sim::mission` tests) is a
**new engine design under adversarial input**. Nothing in it is measured from
the original game, no original opcode, reward, event order or terminal
precedence rule is claimed, and no original mission program has been decoded
into this IR — F38 owns that, and until it exists there is no original program
the corpus could be compared against. The ordering probes below therefore
measure the *engine's own* reference ordering; they do **not** resolve the two
unknowns F37-A deferred to this stage, and both remain unknown.

## The corpus

Six programs, each valid, each built to make the bounded evaluator's job
harder than a plain latch (`crates/cs_script/tests/accept_f37_d.rs`):

1. objectives declared out of symbol order (`9, 3, 7, 1, 5`), each with its own
   delayed reward, so the tick's event order and the drain order disagree;
2. two objectives that latch on one tick and both write the same variable;
3. a self-scheduling list with a positive delay — a bounded loop that keeps
   producing work;
4. a 64-action list behind a delay, so a small budget splits it and every split
   point is a save/restore boundary;
5. zero-delay work interleaved with a phase latch and later objectives;
6. conflicting terminal outcomes, a pending reward that is too late to be
   granted, and an explicit RNG draw.

Each program is run three ways — straight through, with a snapshot and restore
at *every* tick boundary, and with one save in mid-run — and the three traces
must be identical: same per-tick `(tick, source, sequence)` event keys, same
rewards, same variable values, same terminal state. That is the integration
evidence this stage produced, and it is the property that would catch a save
record that forgot a piece of state.

## Reference ordering probes

The expected order is recomputed in the test from the documented key rule —
(session, tick, source symbol, sequence) — rather than trusted from the
runtime's own sort, so a change to the ordering key fails the probe.

- One tick, five objectives with conflicting outcomes: the emitted sequence is
  `(1,1,0..2), (1,3,0..2), (1,5,0..2), (1,7,0..2), (1,9,0..2)` and the five
  `Finish` actions resolve to exactly one answer.
- The same five objectives declared reversed and shuffled emit the **same**
  sequence: the observation order is a function of the keys, not of declaration
  order or map iteration.
- Deferred work keeps the documented key layout: an objective's own events use
  sequence `0..=64`, a scheduled item's events are `(ordinal + 1) * 65 + index
  + 1`, so an objective's items follow its own events in ordinal order and a
  later tick's objective sorts after them.
- The host ledger applies and reports in key order. A result whose events are
  handed over reversed is still applied in key order.

### Two orders, not one (designed, newly pinned)

The probes showed that `MissionProgram::objectives` feeds **two** orders that
are not each other's sort, which the field's own doc comment got wrong (it
claimed declaration order was the ordering key):

- **Execution order is declaration order.** Objectives resolve in declaration
  order, then the pending queue drains. Two objectives writing one variable on
  one tick leave the **later declaration's** write standing: declared `5, 2`
  the survivor is `2`, declared `2, 5` it is `5`.
- **Observation order is `EventKey`**, by source symbol, independent of
  declaration order. So the objective whose write was overwritten reports
  *first*. A reader of an event stream cannot infer the surviving value from the
  report order.

Both are now documented on `MissionProgram` and in the `cs_script::runtime`
module header, and pinned by
`accept_f37_d_simultaneous_writes_follow_execution_order_events_follow_key_order`.
Neither is a claim about the original game.

### The bounds are part of a session's determinism

A tighter budget defers an objective's actions instead of running them on their
own tick, and the deferred queue drains after the later tick's objectives, so
the same program under a different budget **draws the same values in a
different order** and reports its events on different ticks. The reward *set* is
invariant; the order is not. This is why `WorkLimits` travels in the save
record: a restore that dropped it would reproduce a session that never existed.

## Defects the corpus found, and their repairs

Each was reproduced against the pre-repair code first, then fixed, then
mutation-checked: reverting the fix alone makes the matching
`accept_f37_d_*` test fail.

1. **A work budget of one unit per tick silently stalled every mission.**
   Admitting a work item costs one unit (an objective firing or a pending
   dequeue) and running its first action costs another, so a budget of one
   admitted items and executed nothing: objectives latched,
   `StopReason::WorkBudget` came back on every tick, and no reward was ever
   granted, no terminal request ever made and the queue never drained. The
   corpus caught it as "the whole reward set came back empty". The budget now
   has a floor, `MIN_WORK_PER_TICK = 2`, raised by `MissionState::set_limits`.
   Charging admission and the first action as a single unit instead was
   rejected: it would change the meaning of every work count F37-B documents and
   every test that asserts one.

2. **A save record could claim a budget that could never execute anything.**
   `RestoreError::WorkBudgetTooSmall { found }` refuses a record below the
   floor. Same rule as the F37-C clamp of record bounds to the engine maxima, in
   the same direction: a record may tighten the engine's bounds, never to a
   session that cannot progress.

3. **A save record could claim an item-ordinal counter past the sequence
   space.** Every later `Schedule` would stop with `SequenceExhausted` and no
   deferred work would ever run again, so the restore would silently disarm the
   program. `RestoreError::SequenceSpaceExhausted { ordinal }` refuses it. The
   live path's `alloc_ordinal` also stopped using `ordinal + 1` (an overflow in
   debug builds once the counter reached `u32::MAX`) and now shares the
   `ordinal_allocatable` check with the restore.

4. **The host ledger applied another session's events.** `HostLedger::apply`
   inserted any execution key it was handed with no session check, so a replay
   of an earlier run's `TickResult` granted this session a reward it never
   earned *and* wrote a foreign key into its record — and that record then
   failed `HostLedger::restore` with `ForeignExecutionKey`, leaving the session
   permanently unsaveable. A result carrying a foreign key is now refused whole
   with `HostFault::ForeignSession { session }`, effects and outcome claim
   together: the ledger cannot tell which part of a crossed result belongs to
   this session, so it applies none of it.

5. **`HostLedger::apply` trusted the caller's event order** while
   `HostReport` claimed key order. The order is now recomputed from the keys.

6. **A host record could claim the session settled while `Running`.** Every
   later outcome would be a contradiction and the session could never settle.
   `HostRestoreError::SettledWhileRunning { tick }` refuses it.

7. **`MissionProgram::objectives` was documented as the ordering key.** It is
   the execution order; the observation order is `EventKey`. Documentation
   corrected (see "Two orders, not one").

## Unknowns, none resolved by this task

- **Terminal precedence for simultaneous success/failure is still unmeasured.**
  F37-A deferred it to "F37-D reference ordering probes"; the probes could not
  resolve it. There is no original observation to compare against: no original
  mission program has been decoded into this IR (F38), and no original-run
  capture of a mission with conflicting end conditions exists. The runtime keeps
  `PrecedencePolicy::SyntheticConservative` (Aborted > Failed > Succeeded),
  which remains valid for synthetic tests only. This must not be cited as
  original behavior and must gate any fidelity claim about a mission whose end
  conditions can conflict.
- **Original event ordering inside one tick is still unmeasured.** The probes
  measured the engine's own key order, which is what the contract fixes
  ("session/tick/source/program sequence, not hash map or entity iteration
  order") and what the IR documents. Whether an original mission orders by
  declaration, by authoring order or by another key is unknown and needs F13/F38
  plus an original observation.
- **Conflict resolution for two writes to the same variable on one tick is a
  designed policy**, not a measured one. The corpus pins what the engine does
  (later declaration wins) because it must be stable, not because it was
  observed.
- **`max_pending_items == 0` still stalls a program that schedules with a
  delay** — every such schedule reports `StopReason::PendingLimit` and retries
  forever. Unlike the work budget this is left alone: it is reported to the
  caller on every tick, it is coherent ("no deferred work may be stored"), and
  zero-delay work still runs. A caller wanting stored deferred work asks for a
  cap.
- **A `TickResult` carries no session of its own**, so the ledger can only
  check provenance per event. A stale result with *no* events and a terminal
  claim is indistinguishable from a live tick and could still settle this
  session. `MissionSession::advance` cannot produce one; a caller that
  hand-builds results could. Filed as `F37-D-FU1` (#588).
- **Whether an original aborted or torn-down mission still delivers rewards it
  had already earned is unmeasured** (F37-C finding, unchanged).
- **The evaluator's RNG is still seeded from the session generation** rather
  than the run root seed (`F37-C-FU1`, #581, still open). The corpus confirmed
  the consequence that matters: the stream continues across a save/restore,
  because the record carries the draw count.

## Evidence class

Synthetic only. The corpus is a real integration probe of the production path
(`MissionState::step`, `MissionState::snapshot`/`restore`,
`MissionSession::advance`, `HostLedger::apply`), and its results are
reproducible from the committed tests, but it says nothing about original-data
behaviour. The F37 sheet's evidence clause — "record the actual input
fingerprint and consumer trace", "synthetic fixtures alone cannot certify
original-data behavior" — is **not** satisfied by this stage and cannot be until
F38 decodes an original mission program into this IR. F37 stays *checked*, not
recreated.

## Follow-ups filed

- `F37-D-FU1` (#588): carry the session generation on `TickResult` so the host
  can check result provenance, not just per-event provenance.
- `F37-D-FU2` (#589): measure the terminal precedence and tick-ordering rules
  against an original observation, or make their unresolved status gate the
  fidelity claims they affect. See the note on #589: the real dependency is
  F38-D's campaign-reachable instruction evidence plus an owner-supplied
  original-run capture.

## Acceptance

`accept_f37_d_*` (14 tests: 10 in `crates/cs_script/tests/accept_f37_d.rs`, 4 in
`cs_sim::mission`) covers AC04 (an unknown instruction in a condition, an action
list, a list nested two `Schedule` levels deep and a save record's deferred work
— each refuses the whole launch, so the rewards declared before it are
unreachable, and `MissionSession::launch` returns `TerminalState::Unsupported`
without a session to run), the reference ordering probes above, the corpus
replay equality, the work-floor stall and its repair, budget-split plus
save-in-the-same-tick resumption, the bounds at their exact edges (16 nesting
levels legal and 17 refused, 64 actions legal and 65 refused, for both an
objective's list and a scheduled one), and the four host-ledger repairs.