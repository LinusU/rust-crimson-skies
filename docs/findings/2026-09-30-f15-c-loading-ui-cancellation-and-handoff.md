# F15-C: Loading UI, cancellation and simulation handoff

Date: 2026-09-30. Task: F15-C "Wire loading UI, cancellation and
simulation handoff" (`specs/F15-asynchronous-asset-loading-and-private-cache.md`,
stage `### F15-C`). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary
build/test only — no retail data, no original run, no human play. Nothing
here is `verified_original`; the whole design is `designed` by
construction and every fixture is synthetic.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/loading.rs`: the F15-C section — `SourceFault` (with
  `From<ReadError>`/`From<ResolveError>`), the `LoadIo` producer trait,
  `SessionIo` (the real producer over a `ContentSession`), the
  `ExpectedLoad` resource plus the `HandoffError::Unannounced` variant it
  requires, `LoadingScreen`, `RetryError`, and `LoadingSession` (`new`,
  `identity`, `target`, `state`, `failures`, `driver`, `cancel_handle`,
  `announce`, `begin`, `pump`, `run`, `screen`, `cancel`, `retry`,
  `invalidate_source`, `deliver`, `close`). Smaller edits:
  `LoadDriver::load_item`'s `read_source` closure now returns
  `Result<_, SourceFault>` so a producer's own code and recovery path
  reach the transaction's failure record unchanged, and
  `LoadDriver::store_mut` exposes the store for the between-loads
  operations the cache owns.
- `crates/cs_app/src/assets.rs`: `ConversionError::Failed` — a converter
  refusing the canonical payload is its own failure class (`Abort`
  recovery), not a stale-key misreport.
- `crates/cs_assets/tests/accept_f15_c_load_session.rs` (new): six
  `accept_f15_c_*` tests.
- `crates/cs_assets/tests/accept_f15_b_load_driver.rs`: the `read_source`
  closure signature change, propagated (the two fault closures now wrap
  `ReadError` into `SourceFault`).

**One observable failure:** before this stage, `LoadDriver::load_item`
was the only production entry point and its `read_source` closure could
only answer `Result<_, ReadError>` — a VFS `ResolveError` (a key no mount
answers, i.e. a missing dependency) was *unrepresentable*, so a real
producer would have had to mislabel it as a retryable read fault, and
nothing owned the pump/cancel/retry/handoff lifecycle or gave a loading
screen anything to draw. Concretely: `ReadyBundle::attach` trusted a
caller-supplied `expected` identity, so a world had no record of which
load it was waiting for.

## Design decisions

- **`SessionIo` is the producer seam; `LoadIo` is the trait.** The driver
  already owns the cache half of every item. What F15-C adds is the real
  `LoadIo` implementation: each item is resolved inside the read, in the
  session the request belongs to (`session.resolve`, then
  `begin_read`/`complete_with`/`accept`), the load's own cancel switch is
  forwarded into the bounded `PendingRead` at each chunk boundary, and
  the read's chunk progress is forwarded to the screen model. Which
  converter a content kind maps to stays caller-supplied — the format
  stages (F08+/F17-B) own that dispatch, so `SessionIo` takes it per
  load rather than inventing a registry.
- **Error propagation keeps the producer's own verdict.** `SourceFault`
  carries `code`, `detail`, `recovery`; `From<ResolveError>` maps
  `not_found`/`ambiguous` to `MissingDependency` and `unmeasured_order`
  to `Abort`, and `From<ReadError>` keeps `source_read`/`Retry` (the
  F15-B surface is unchanged) while giving `Cancelled` and
  `ForeignSession` their own codes. The transaction's failure record
  therefore names the missing dependency and its recovery exactly as
  non-negotiable behavior 5 requires — a `not_found` is never disguised
  as a transient fault.
- **`LoadingSession::pump` is the frame step.** One call moves the load
  exactly one bounded step forward — the next unsettled item's read
  through `io`, or `validate_delivered` once every item has settled — so
  a UI loop draws `screen()` between pumps and each frame's work stays
  bounded. `run` is the same loop to terminal for the headless path. The
  two closures `load_item` holds need the producer mutably and never
  overlap in time, so a `RefCell` reborrows the one `io` between them;
  the step reporter and the source read's chunk progress share a `Cell`
  the same way. No threads, no executor — the schedule is the caller's.
- **Cancellation is one switch with two observers.** `cancel()` throws
  the driver's switch (a bounded step in flight stops at its next chunk
  boundary) and `LoadTransaction::cancel` flags every outstanding
  ticket; `cancel_handle()` is the same switch cloneable onto the UI's
  thread. A cancelled load's unfinished reads are cancelled *work*, not
  failures: `stop_cancelled` ends the transaction first, so its late
  completions are discarded and `failures()` records nothing — the
  `ItemRead::Failed { code: "read_cancelled" }` is the pump's report of
  the step, not a transaction failure. A cancelled load refuses `retry`
  (`RetryError::NotFailed`): a switch-away may already have replaced the
  session, so re-entering is a new request, not a retry.
- **Retry is a fresh transaction over the same store.** `retry` rebuilds
  the `LoadRequest` from the terminal transaction's own record — same
  session generation, same items — under a new `LoadSerial`, so no
  completion or bundle of the failed attempt can land in it, while the
  store comes along untouched: entries the failed attempt published
  still serve a warm re-read (the retry test proves the retry's hull
  item is a `CacheHit` and the source is never re-read).
- **The world decides which bundle may attach.** `ExpectedLoad` is the
  world's own record of the load it is waiting for; `announce` writes it
  and `deliver` checks it before `attach`, so an unannounced world
  (`Unannounced`), a world expecting another load (`Foreign`) and a
  repeat delivery all refuse and spawn nothing — the caller no longer
  supplies `expected` on trust, and a successful delivery consumes the
  expectation. `deliver` takes `&self`, so *when* the boundary happens
  stays the schedule's choice; the point of the design is that it is a
  boundary — a bundle produced by an old transaction cannot attach to a
  world that announced its replacement.
- **`invalidate_source` is AC03 at the session boundary.** The producer
  calls it when a source it previously resolved changed — the *old*
  `SourceSpanHash` is what the stale entries record — and the store
  drops only entries derived from that span. The minimum scenario wires
  it end to end: two liveries resolve through a real `ContentSession`,
  are composed by the real `read_bm`/`BmFile::compose` path, are cached
  under keys listing their own spans; editing one source and remounting
  invalidates exactly that livery's entry — the other is a warm hit and
  the store afterwards holds exactly {surviving, rebuilt}.
- **The screen model is measured, never guessed.** `LoadingScreen`
  carries `LoadProgress` in the items' declared work units, the bounded
  step's `ReadProgress`, the item in flight, and the failures with their
  recovery paths — a broken load shows what to fix instead of a bar
  stopped at 99 percent, and `cancellable` tracks the terminal state.

## Recorded unknowns and limitations

- **The schedule and the renderer are deliberately absent.** `pump` is a
  synchronous bounded step the caller drives; wiring it into a Bevy
  schedule (and `LoadingScreen` into an actual UI tree, font and layout)
  is the renderer/UI stages' work — `docs/01-ARCHITECTURE.md` has no
  finished UI layer for it to land in yet. The app state machine's
  `Loading` state itself is also later work: `LoadingSession` is the
  load-owner it will hold, not a patch onto `run.rs`'s synthetic scene.
- **The converter dispatch is supplied per load.** `SessionIo` cannot
  guess which converter a content kind needs — the format adapters do
  not exist yet — so the mapping is a constructor argument. When the
  format stages land, one production dispatch will replace the tests'
  `convert_member`; `ConversionError::Failed` is where a decoder's
  refusal goes.
- **One pump at a time.** `next` moves items strictly in request order
  and nothing races `load_item`: this stage wires the seam, it does not
  parallelize it. A real async executor (reads on worker threads,
  completions arriving out of order) would feed `IoCompletion`s into the
  same transaction through `accept`, which already stamps and discards —
  that is the seam the F15-A design kept open, not something this stage
  had to re-derive.
- **`close` cancels a live load and drops the transaction.** The store
  survives by design; the transaction record is not returned — a world
  switch keeps no handle on the dead load, which is exactly what the
  session-generation discipline guarantees anyway.
- **The mutual-borrow juggling is local.** `pump`'s `RefCell`/`Cell`
  exist because `load_item`'s two-closure signature needs the producer
  mutably in both; a future split of `load_item` into `read_item` +
  `convert_item` could remove it without changing behavior — recorded,
  not done, because the driver signature is shared infrastructure other
  work may already rely on.
- **No retail claim anywhere.** The fixture installation is synthetic
  and authored for these tests; the BM bytes exercise the real reader
  and composition but say nothing about the original files. F15-D owns
  the cold/warm/restart comparison on real content.

None of these needed a new task: they are the declared scopes of the
format stages and F15-D, so `create_tasks` was not used.

## Mutation verification (run, then reverted)

Each mutation was applied to `crates/cs_app/src/loading.rs`, the
`accept_f15_c_load_session` binary was run, and the file was restored
from a copy of the pre-mutation source.

| Mutation | Test that failed |
| --- | --- |
| `invalidate_source` returns an empty report | `accept_f15_c_changed_livery_source_invalidates_only_its_derived_assets` |
| `deliver` skips the world's `ExpectedLoad` check | `accept_f15_c_handoff_attaches_only_to_the_announced_world` |
| `SessionIo` reports a resolve refusal as `source_read`/`Retry` | `accept_f15_c_missing_dependency_names_the_failure_and_recovery` |
| `load_item` never observes the cancel switch | `accept_f15_c_cancel_stops_the_load_and_the_teardown_is_clean` |
| `retry` drops its `Failed`-only guard | `accept_f15_c_retry_is_a_fresh_transaction_over_the_same_store`, `accept_f15_c_cancel_stops_the_load_and_the_teardown_is_clean` |

## Commands run

All from the repository root on branch
`rally/63-wire-loading-ui-cancellation-and-simulat`, Rust 1.98.1.

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f15_c_ --include-ignored` | 0 |

The acceptance selection ran six tests in
`tests/accept_f15_c_load_session.rs`; all passed, and the mutations above
show the file fails when the wiring is removed.

## Wiring edits (outside owner paths, logic-free)

- `crates/cs_app/src/lib.rs`: the module doc for `loading` describes the
  F15 pipeline including this stage — documentation only.
- `crates/cs_assets/Cargo.toml`: `cs_formats` added to
  `[dev-dependencies]` — the tests' livery path runs the real `read_bm`
  and `BmFile::compose` instead of a nominal transform. The
  `cs_app`/`bevy` dev-only edges it sits beside were already documented
  there. `Cargo.lock` unchanged: `cs_formats` was already in the graph.

No protected path, original datum or binary file is involved.

## Sources

`specs/F15-asynchronous-asset-loading-and-private-cache.md` (AC03 is this
stage's minimum scenario; behaviors 2, 4 and 5 are the ones this wiring
implements), `docs/contracts/IDENTITY-CONTENT.md` (session generations
and the stale-completion rule the handoff relies on),
`docs/contracts/STATE-TRANSACTIONS.md` (a mission retry is a new session
generation — hence `retry` re-entering the same one),
`docs/contracts/UI-NETWORK.md` (UI requests domain transactions, failure
surfaces recovery), `docs/01-ARCHITECTURE.md` (the application lifecycle's
`Loading` state), and the F15-A/F15-B findings in `docs/findings/`, which
left the producer/consumer wiring to this stage.
