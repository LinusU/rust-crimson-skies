# F15-B: Bounded asynchronous reads and the atomic private cache

Date: 2026-09-29. Task: F15-B "Implement bounded asynchronous reads and
atomic cache" (`specs/F15-asynchronous-asset-loading-and-private-cache.md`,
stage `### F15-B`). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary
build/test only — no retail data, no original run, no human play. Nothing
here is `verified_original`; the whole design is `designed` by construction
and every fixture is synthetic.

Status of this file: the pre-edit plan required by the stage ("Before
editing, list the specific functions/files and one observable failure") is
recorded first; the design, verification and command records are appended
below as the work lands.

## Files and the one observable failure (listed before editing)

- `crates/cs_assets/src/cache/store.rs` (new): the on-disk store.
  `CacheStore` (`open`, `entries_root`, `staging_root`, `usage`,
  `recovery`, `begin_read`, `begin_write`, `commit`, `discard`,
  `invalidate_source`), `StoreError` (`Io`, `Budget`, `EntryTooLong`,
  `ReadCancelled`), `CacheReadError` (`Cancelled`, `Corrupt`,
  `CorruptHeader`, `Io`), `CacheLookup` (`Miss`, `Rebuild`, `Hit`),
  `PendingCacheRead` (`total`, `cancel_handle`, `complete`,
  `complete_with`), `PendingStoreWrite` (`total`, `written`,
  `cancel_handle`, `append`, `write_all`, `seal`), `StoredEntry`,
  `StoreUsage`, `RecoveryReport`, `InvalidationReport`, the persisted
  header codec (`cs-cache-entry/1`) and `CACHE_IO_CHUNK`.
- `crates/cs_assets/src/cache/entry.rs`: one added constructor,
  `EntryHeader::committed_streaming` — the bounded writer streams the
  payload and never holds it whole, so it must be able to commit the
  length and the digest it computed incrementally. It is covered by the
  same `verify_entry` gate as `EntryHeader::committed`.
- `crates/cs_assets/src/cache/mod.rs`: module doc and re-exports for
  `store`.
- `crates/cs_app/src/loading.rs`: `LoadDriver` (`new`, `cancel_handle`,
  `cancel`, `load_item`, `validate_delivered`, `transaction`,
  `transaction_mut`, `store`, `into_parts`), `ItemOutcome`,
  `CacheSkipReason`, `LoadDriverError` — the production path that takes a
  `LoadTransaction`'s bounded reads through the store and re-verifies what
  it delivered before the transaction may go `Ready`.
- Tests (`crates/cs_assets/tests/`, prefix `accept_f15_b_`):
  `accept_f15_b_atomic_cache.rs` and
  `accept_f15_b_load_driver.rs`; `crates/cs_app/tests/accept_f15_b_load_driver.rs`
  holds the driver tests (the driver is `cs_app` code, and the store
  tests need no `cs_app` edge).
- `crates/cs_assets/Cargo.toml` only if a new dev-dependency is needed.

**One observable failure:** a process is killed while a derived entry is
being written, and the next startup either serves the half-written bytes
as a cache hit — an old mission's texture payload served for a new one —
or refuses to open the store at all because of the debris the killed
process left behind. Spec F15 AC02 demands the third answer: the entry is
rebuilt from its sources and startup completes cleanly.

## Recorded unknowns and limitations

(filled in below as the work lands)
