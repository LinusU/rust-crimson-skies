# F15-B: Bounded reads and the atomic private cache

Date: 2026-09-29. Task: F15-B "Implement bounded asynchronous reads and
atomic cache" (`specs/F15-asynchronous-asset-loading-and-private-cache.md`,
stage `### F15-B`). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary
build/test only — no retail data, no original run, no human play. Nothing
here is `verified_original`; the whole design is `designed` by construction
and every fixture is synthetic.

## Files and the one observable failure (listed before editing)

- `crates/cs_assets/src/cache/store.rs` (new): the on-disk store.
  `CacheStore` (`open`, `directory`, `budget`, `entries_root`,
  `staging_root`, `usage`, `recovery`, `begin_read`, `begin_write`,
  `commit`, `discard`, `invalidate_source`), `StoreError`, `CacheReadError`,
  `CacheLookup` (`Miss`, `Rebuild`, `Hit`), `PendingCacheRead` (`key`,
  `total`, `cancel_handle`, `complete`, `complete_with`),
  `PendingStoreWrite` (`key`, `total`, `written`, `is_sealed`, `staging`,
  `cancel_handle`, `append`, `write_all`, `seal`), `StoredEntry`,
  `StoreUsage`, `RecoveryReport`, `InvalidationReport`, the persisted
  record codec (`cs-cache-entry/2`) and `CACHE_IO_CHUNK`.
- `crates/cs_assets/src/cache/entry.rs`: one added constructor,
  `EntryHeader::committed_streaming` — a bounded writer streams the
  payload and never holds it whole, so it must be able to commit the
  length and the digest it computed incrementally. It is covered by the
  same `verify_entry` gate as `EntryHeader::committed`.
- `crates/cs_assets/src/cache/key.rs`: one added constructor,
  `SourceSpanHash::from_digest` — `CacheKey::from_hashed_inputs` is the
  "rebuild from a stored index" constructor F15-A documented, and it had no
  way to produce an input from a digest.
- `crates/cs_assets/src/cache/mod.rs`: module doc and re-exports.
- `crates/cs_app/src/loading.rs`: `LoadDriver` (`new`, `transaction`,
  `transaction_mut`, `store`, `into_parts`, `cancel_handle`,
  `is_cancelled`, `cancel`, `load_item`, `validate_delivered`),
  `StepProgress`, `RebuildCause`, `UncachedReason`, `ItemRead`,
  `DriverError` — the production path that takes a `LoadTransaction`'s
  bounded reads through the store and re-verifies what it delivered before
  the transaction may go `Ready`.
- Tests (`crates/cs_assets/tests/`, prefix `accept_f15_b_`):
  `accept_f15_b_atomic_cache.rs` (11 tests) and
  `accept_f15_b_load_driver.rs` (9 tests). `crates/cs_app/tests/` is
  outside this task's owner paths, so the driver tests live beside the
  store tests over the existing `cs_assets` dev-dependency on `cs_app`.

**One observable failure:** a process is killed while a derived entry is
being written, and the next startup either serves the half-written bytes
as a cache hit — an old mission's texture payload served for a new one —
or refuses to open the store at all because of the debris the killed
process left behind. Spec F15 AC02 demands the third answer: the entry is
rebuilt from its sources and startup completes cleanly.

## Design decisions

- **The commit is one directory rename.** A write is staged in
  `staging/<pid>-<serial>` with a `writing` record, filled in
  `CACHE_IO_CHUNK` steps, then sealed — payload flushed and synced,
  committed record written and synced, scratch directory synced — and only
  then published by renaming the finished directory onto
  `entries/<key-digest-hex>`. Everything before the rename is scratch
  (removable, never served); everything after it is a complete entry a
  reader can verify. A sealed-but-uncommitted write is scratch, which is
  why the test kills a second child process *after* the seal as well as
  mid-write: a store that published on the seal would serve an entry that
  was never committed.
- **The persisted record carries the whole cache key, not just its
  digest.** A reader rebuilds the key from the record's own facets
  (installation, sorted input hashes, decoder, decoder version, IR version,
  normalized options) and keeps the record only if those facets hash to the
  digest the record declares. A hand-edited record is therefore not
  decodable, and the same input list is what makes
  `CacheStore::invalidate_source` answerable from the store's own records
  (spec F15 AC03) without the caller supplying the keys.
- **The store trusts nothing about itself.** The bytes a reader consumes
  only ever become a `VerifiedEntry` through the F15-A `verify_entry`
  gate: the record is decoded again at read time, the declared length is
  checked against the file before anything is hashed, and the digest is
  recomputed. `EntryHeader::committed_streaming` is the one new way to
  build a committed header, and it is gated identically — a writer that
  misreports its length or digest is refused, not served.
- **Recovery is a reported startup step.** `CacheStore::open` sweeps every
  scratch directory and drops every published entry it cannot verify
  (uncommitted, undecodable, stored under another name, or declaring a
  payload length the file does not have), and says what it did in
  `RecoveryReport`. Anything below `entries/` that is not this store's own
  bookkeeping is counted in `kept_unknown` and left alone: a private cache
  never deletes what it did not write.
- **Eviction is deterministic and bounded.** Before publishing, the store
  evicts committed entries with the lowest write sequence until the new
  one fits; ties break on the digest spelling, so the order never depends
  on iteration luck, and a read does not refresh it (no timestamps, no
  clock, no host dependence). An entry that could not fit even an empty
  store is refused *before* anything is evicted, so a refusal never costs
  the store the entries it already holds.
- **A cache problem is never a load failure.** The driver treats a budget
  refusal, a store write failure and an unreadable store as
  `ItemRead::Uncached` with the reason reported: the derived bytes are
  already in hand, and spec F15 behavior 1 makes the cache an
  optimization, never the authoritative data source. A gameplay-critical
  *source* read failure still fails the load — the cache is not a way to
  paper over missing content.
- **An integrity refusal cancels the load, it does not fail the item.**
  Only the load-wide switch cancels a read, so a cancellation means the
  caller asked for the load to stop: `stop_cancelled` cancels the
  transaction (its state says `Cancelled`, not `Failed`, and every other
  outstanding ticket is flagged) and reports a retryable failure to the
  caller. A world switch must not be reported as broken content.
- **Validation re-verifies what the cache served.**
  `LoadDriver::validate_delivered` re-reads every cache-delivered entry
  through the bounded path and checks it against the digest the load
  recorded, then calls `LoadTransaction::validate`. This is the
  `Validating -> Failed` arc F15-A explicitly left open for this stage; a
  refusal fails the load with a `RebuildDerived` recovery path and the
  item named, so cache corruption can neither change campaign state nor
  make a world interactive.
- **One authority on what may be read.** The driver keeps no "already
  settled" flag of its own: `LoadTransaction::issue_io` already refuses an
  out-of-range index, a settled item and a load that is not `Loading`, and
  a second copy of that rule could only drift from it.
- **Progress is measured twice over.** `StepProgress` carries the
  transaction's progress in declared work units and the bounded step in
  flight in bytes, so a loading bar can be driven from real work at chunk
  resolution and the load's own units stay the authority.

## Recorded unknowns and limitations

- **The original engine had no derived cache**, so the layout, the record
  format, the eviction order and the chunking are new-engine design. No
  `verified_original` or `release_approved` claim is made anywhere in this
  work, and the F15-D cold/warm comparison on real content is still to be
  run.
- **The commit's atomicity rests on `rename(2)`.** The tests prove that a
  killed process leaves no published entry (two kill points: mid-write and
  after the seal) and that a partially published entry is refused and
  rebuilt, but a mutation that published by copying the two files instead
  of renaming them is *not* caught by any test: from userspace there is no
  window in which to kill the process between two `copy` calls, and the
  resulting half-published entry would be refused by the same gate (the
  partially-published test proves that half). The mechanism is named in
  the module doc and is one `rename`; a reviewer should read it, not just
  its test.
- **One store at a time owns a cache directory.** Startup recovery sweeps
  scratch directories, so a second live store on the same root would
  discard the first one's in-flight write. The root is a private per-user
  path (`CacheDirectory`), which is what makes that acceptable; a
  multi-process cache would need a lock file, which is not this stage.
- **Windows directory syncing is a no-op** in `sync_dir`: the platform
  offers no handle to fsync a directory, so there the rename's own
  ordering is what the code can rely on. Recorded in the function's doc.
  CI is Linux; the tests run on macOS and Linux.
- **A record that cannot be decoded is dropped at startup, not repaired.**
  Rebuilding it is the next load's job, which is the behavior spec F15
  requires, but it does mean a corrupt store re-derives its entries
  instead of salvaging them.
- **The decoder/IR version vocabulary is engine-authored**, unchanged
  from F15-A: concrete decoders register theirs as the conversion stages
  land (F08+). `IrVersion(1)` in the fixtures is not a schema claim.
- **The driver's conversion step is injected.** F15-B is not a format
  task, so `load_item` takes the source read and the conversion as
  closures; the real per-format converters arrive with F08+/F15-C. What is
  production here is the read, the atomic publish, the settle and the
  validation gate around them.

None of these needed a new task: they are the declared scopes of F15-C
(UI, schedule, simulation handoff) and F15-D (cold/warm/restart on real
content), so `create_tasks` was not used.

## Mutation verification (run, then reverted)

Each mutation was applied, the two `accept_f15_b_` binaries were run, and
the file was restored from a copy of the pre-mutation source.

| Mutation | Test that failed |
| --- | --- |
| `open` skips the scratch sweep | `accept_f15_b_killed_cache_write_recovers_on_next_startup` |
| `begin_read` trusts the record's declared length | `accept_f15_b_partially_published_entry_is_refused_and_rebuilt` |
| `complete_with` returns the bytes without `verify_entry` | `accept_f15_b_tampered_entry_is_refused_never_served` |
| `make_room` never evicts | `accept_f15_b_store_evicts_within_its_budget_and_refuses_the_impossible`, `accept_f15_b_store_never_writes_inside_the_installation` |
| the driver stops passing its switch to the bounded read | `accept_f15_b_cancelled_cache_read_stops_the_load` |
| `validate_delivered` skips the re-verification | `accept_f15_b_validation_refuses_a_cache_entry_that_changed` |
| `invalidate_source` ignores `depends_on` | `accept_f15_b_invalidation_drops_only_the_dependent_entries` |
| a bounded read never observes the cancellation | `accept_f15_b_reads_are_chunked_measured_and_cancellable` |
| `seal` accepts a write holding fewer bytes than it declares | `accept_f15_b_seal_refuses_an_incomplete_write` |
| the driver serves a refused entry instead of rebuilding it | `accept_f15_b_refused_entry_is_rebuilt_not_served` |
| recovery deletes entries it did not write | `accept_f15_b_unreadable_cache_still_loads_from_the_source` |
| a cache hit delivers bytes that differ from the rebuild | `accept_f15_b_warm_and_cold_loads_deliver_equal_content` |
| an unpublished write leaves its scratch behind | `accept_f15_b_dropped_write_leaves_no_entry` |
| `issue_io` lets a settled item be read again | `accept_f15_b_an_item_is_read_once_per_driver` |
| a failure always names the first item | `accept_f15_b_failures_name_the_dependency_and_the_recovery_path` |
| the driver publishes a derived form it never sealed | `accept_f15_b_warm_and_cold_loads_deliver_equal_content`, `accept_f15_b_refused_entry_is_rebuilt_not_served`, `accept_f15_b_uncacheable_item_is_delivered_not_failed`, `accept_f15_b_failures_name_the_dependency_and_the_recovery_path` |

One mutation was **not** caught and is recorded as a limitation above:
publishing by copying the staged files instead of renaming them. The two
mutations that first appeared uncaught (the driver's own "already
settled" flag, and a failure that names the wrong item) led to real fixes
— the duplicate flag was removed, and the test now asserts which item a
failure names.

## Commands run

All from the repository root on branch
`rally/62-implement-bounded-asynchronous-reads-and`, Rust 1.98.1.

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 (926 tests) |
| `cargo test --workspace --locked -- accept_f15_b_ --include-ignored` | 0 (20 tests: 11 store, 9 driver) |

## Wiring edits (outside owner paths, logic-free)

None. `crates/cs_assets/src/lib.rs` already declares `pub mod cache;`, the
driver lives in `crates/cs_app/src/loading.rs` (an owner path), the tests
use the existing `cs_assets` dev-dependency on `cs_app`, and no manifest
or `Cargo.lock` changed.

No protected path, original datum or binary file is involved.

## Sources

`specs/F15-asynchronous-asset-loading-and-private-cache.md` (AC02 is this
stage's minimum scenario; behaviors 1, 2, 3 and 5 are the ones this code
implements), `docs/contracts/IDENTITY-CONTENT.md` (canonical lowercase hex
hash strings; the closure-hash and unsupported-reason rules), and the
F15-A finding `docs/findings/2026-09-29-f15-a-load-transaction-and-cache-key-contracts.md`,
which left the on-disk layout, the atomicity mechanism, the eviction policy
and the `Validating -> Failed` re-verification to this stage.
