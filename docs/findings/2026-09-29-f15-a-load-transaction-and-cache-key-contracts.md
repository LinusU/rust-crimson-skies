# F15-A: Load transaction and cache key contracts

Date: 2026-09-29. Task: F15-A "Define load transaction and cache key
contracts" (`specs/F15-asynchronous-asset-loading-and-private-cache.md`,
stage `### F15-A`). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary
build/test only — no retail data, so nothing here claims original
behavior; the whole design is `designed` by construction.

## Files and the one observable failure (listed before editing)

- `crates/cs_assets/src/cache/mod.rs` (new): module doc and re-exports.
- `crates/cs_assets/src/cache/key.rs` (new): `DecoderId`, `IrVersion`,
  `ConverterVersion`, `ConversionOption`/`ConversionOptions` (+`OptionsError`),
  `SourceSpanHash`, `CacheKey` (`new`, `from_hashed_inputs`, `digest`,
  `inputs`, `depends_on`), `CacheKeyError`, `CacheLabelError`.
- `crates/cs_assets/src/cache/entry.rs` (new): `EntryState`,
  `EntryHeader` (`writing`, `committed`), `verify_entry` →
  `VerifiedEntry`, `IntegrityError` (`Uncommitted`, `WrongKey`,
  `LengthMismatch`, `DigestMismatch`).
- `crates/cs_assets/src/cache/bound.rs` (new): `CacheDirectory`,
  `CacheLocationError`, `CacheBudget`/`BudgetError`/`BudgetExceeded`.
- `crates/cs_app/src/loading.rs` (new): `LoadSerial`, `LoadIdentity`,
  `LoadTarget`, `Criticality`, `LoadItem`/`LoadItemError`, `LoadRequest`,
  `LoadState` (+`permits`), `TransitionError`, `RecoveryPath`,
  `LoadFailure`, `IoOutcome`, `IoTicket`, `IoCompletion`,
  `CompletionVerdict`, `IssueError`, `LoadProgress`, `CancelReport`,
  `LoadTransaction` (`issue`, `begin`, `issue_io`, `accept`, `cancel`,
  `validate`, `progress`, `critical_closure_ready`,
  `is_world_interactive`, `ready_bundle`), `ReadyItem`, `HandoffError`,
  `LoadedItemBinding` (Bevy `Component`), `ReadyBundle` (`attach`).
- `crates/cs_app/src/assets.rs` (new): `CanonicalPayload`,
  `CanonicalAsset`, `ConvertedAsset` (`verify_fresh`), `ConversionError`.
- Tests (`crates/cs_assets/tests/`, selected by `accept_f15_a_`):
  `accept_f15_a_cache_contract.rs` (6 tests),
  `accept_f15_a_load_transaction.rs` (5 tests),
  `accept_f15_a_conversion_boundary.rs` (2 tests); shared synthetic
  fixture helpers added to `crates/cs_assets/tests/common/mod.rs`.

**One observable failure:** an IO read issued under world `zbd/c1`'s
session, completing only after the world switched to `zbd/c2`, is
accepted into c2's load and its entities spawn into c2's world — an old
mission texture attached to a new mission. Without the
`(SessionGeneration, LoadSerial)` identity checks in
`LoadTransaction::accept` and `ReadyBundle::attach`, the stale result is
indistinguishable from a live one.

## Design decisions

- **Cache identity covers exactly the four facets the sheet names.**
  `CacheKey` = installation hash + sorted/deduplicated
  `SourceSpanHash`es (themselves SHA-256 over every `SourceSpan` field)
  + `ConverterVersion` (decoder id, decoder version, `IrVersion`) +
  normalized `ConversionOptions`. The digest is computed once at
  construction over a domain-separated, length-prefixed encoding, so
  warm and cold derivations of the same content hash equal (the
  determinism half of AC04) and a key with zero inputs is refused —
  nothing derived from nothing can be invalidated precisely.
- **Per-input granularity makes AC03 expressible.** A derived asset
  lists exactly the spans it was built from. The test builds a texture
  keyed on `[a]` and a livery keyed on `[a, b]`, then edits `b`'s member
  digest: only the livery's key changes. `CacheKey::depends_on` exposes
  the same granularity to the F15-B store.
- **Entry integrity is a gate, not a hint.** `EntryHeader` is either
  `Writing` or `Committed { payload_len, payload_sha256 }`; only
  `verify_entry` turns stored bytes into a `VerifiedEntry`. An
  uncommitted header (a killed write), a header stored under another
  key, a truncated payload and a corrupted payload each fail with a
  distinct `IntegrityError` that means *rebuild*, never *serve* — spec
  F15 non-negotiable behavior 3.
- **Private and bounded are construction-time contracts.**
  `CacheDirectory::open` mirrors `vfs::export::ExportDirectory`: the
  root must exist, not be a symlink, and its canonical path must not lie
  inside the canonical installation root — the cache can never be
  configured to write into the read-only source tree. `CacheBudget`
  requires nonzero entry and byte bounds; `check` reports which bound a
  store state would cross.
- **The load transaction is a state machine over session/serial
  identity.** `Requested → Loading → Validating → Ready|Failed`, plus
  `Cancelled` reachable from any non-terminal state; every transition is
  checked by `LoadState::permits` and a terminal state accepts nothing.
  Each issued read is an `IoTicket` stamped `LoadIdentity { session:
  SessionGeneration, serial: LoadSerial }` — the session generation
  comes from `cs_assets::vfs`, the serial from a process counter, so a
  retry or successor can never share it. `accept` returns `Foreign` for
  a completion stamped by another transaction and `Discarded` for one
  arriving outside `Loading` (including cancelled). `cancel` flags every
  outstanding ticket's `ReadCancel` — the same mechanism
  `vfs::PendingRead` checks mid-read — so in-flight work is detached,
  not orphaned.
- **Failure keeps the dependency and the recovery path.** A deferred
  item's `Fault` is recorded (`LoadFailure` with `AssetKey`, code and
  `RecoveryPath`) and the load continues; a gameplay-critical `Fault`
  ends the transaction in `Failed`, because a world without its critical
  closure may never become interactive. `omitted` keys are carried on
  the `ReadyBundle` rather than hidden.
- **The ready bundle is the only path to entities.** `ready_bundle()`
  exists only for `Ready`; `attach` checks the world's expected
  `LoadIdentity` before spawning a single `LoadedItemBinding` entity and
  refuses a foreign bundle with `HandoffError::Foreign`, spawning zero.
  This is the controlled boundary non-negotiable behavior 4 wants; F15-C
  owns when in the schedule it is invoked.
- **The conversion boundary is typed, not yet run.** `CanonicalAsset`
  (content id + `CacheKey` + digested `CanonicalPayload`) and
  `ConvertedAsset<T>` (stamped with producing load and cache-key digest,
  `verify_fresh` refuses a stale key) are the records F15-B/C converters
  produce and consume. "Only cs_app converts" stays structural: lower
  crates have no Bevy dependency.
- **Progress is measured units.** `LoadItem::new` refuses zero work
  units; `progress()` sums declared units of settled items, so a bar can
  never show a guessed percent.

## Recorded unknowns and limitations

- **The on-disk layout and atomicity mechanism are undecided** — F15-B
  owns temp/rename/journal mechanics; this stage fixes only that an
  uncommitted or corrupt entry can never verify.
- **The eviction policy is undecided** — `CacheBudget` bounds and
  reports; which entries a full store drops is F15-B.
- **The decoder/IR version vocabulary is engine-authored**, like every
  label set so far; concrete decoders register theirs as the conversion
  stages land (F08+, F15-B). `IrVersion(1)` in tests is a fixture, not a
  schema claim.
- **Whether the original engine had any derived cache is unknown** and
  irrelevant to correctness here: the cache is new-engine design, an
  optimization, never the authoritative data source. No
  `verified_original` claim is made anywhere in this work.
- **`attach` spawns binding-marker entities only.** Real scene/mesh
  spawning is F15-C and the F11/F17 stages; the marker is the
  contract-level proof that a stale load contributes zero entities.

None of these needed a new task: they are the declared scopes of F15-B,
F15-C and F15-D, so `create_tasks` was not used.

## Mutation verification (run, then reverted)

- `ReadyBundle::attach` with the identity check removed →
  `accept_f15_a_stale_bundle_attaches_no_entities_to_successor_world`
  fails (the stale bundle spawns). Reverted.
- `LoadTransaction::accept` with the foreign-identity check removed →
  `accept_f15_a_cancelled_loads_late_completion_spawns_nothing` fails
  (the successor no longer reports `Foreign`). Reverted.

## Commands run

All from the repository root on branch
`rally/61-define-load-transaction-and-cache-key-co`, Rust 1.98.

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f15_a_ --include-ignored` | 0 (13 tests: 6 cache contract, 5 transaction, 2 boundary) |

## Wiring edits (outside owner paths, logic-free)

- `crates/cs_assets/src/lib.rs`: `pub mod cache;` plus a doc paragraph.
- `crates/cs_app/src/lib.rs`: `pub mod assets; pub mod loading;` plus a
  doc paragraph.
- `crates/cs_app/Cargo.toml`: `cs_assets` moved to `[dependencies]`
  (`src/loading.rs` uses `vfs::SessionGeneration`/`ReadCancel` and
  `cache::CacheKey`; `src/assets.rs` uses `cache::CacheKey` and
  `install::sha256`); comment updated. Architecture permits it —
  `cs_app` may use "all necessary lower crates" (`docs/01-ARCHITECTURE.md`).
- `crates/cs_assets/Cargo.toml`: new `[dev-dependencies]` on `cs_app`
  and `bevy` so the acceptance tests exercise the transaction and its
  Bevy entity boundary. Dev-only edge; the production graph stays
  acyclic.
- `Cargo.lock`: regenerated for the two manifest changes.

No protected path, original datum or binary file is involved.

## Sources

`specs/F15-asynchronous-asset-loading-and-private-cache.md`,
`docs/contracts/IDENTITY-CONTENT.md`, `docs/contracts/STATE-TRANSACTIONS.md`
(session reset semantics), `docs/01-ARCHITECTURE.md` (dependency table,
asset pipeline), and the F04-A/F04-C/F11-A/F14-A findings for precedent
(session generations, `ExportDirectory`, `SceneNodeBinding`, contract
stage structure).
