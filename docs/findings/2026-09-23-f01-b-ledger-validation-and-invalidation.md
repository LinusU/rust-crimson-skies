# F01-B: ledger validation and dependency invalidation — design notes

Date: 2026-09-23. Task: F01-B "Implement ledger validation and dependency
invalidation" (`specs/F01-evidence-ledger-provenance-and-reference-policy.md`,
`### F01-B`). Shared contract: `docs/contracts/CLI-EVIDENCE.md`.
Capabilities used: ordinary build/test only.

## Files and the one observable failure

- `crates/cs_types/src/evidence.rs` (extended): `ObservedFingerprint`,
  `FingerprintIndex` (+`ObservationIndexError`), `LedgerError`,
  `LedgerRejection`, `StaleEvidence`, `ClaimInvalidation`,
  `UncheckedReason`, `UncheckedDependency`, `LedgerReport`,
  `validate_ledger`, `FingerprintKind::label`/`Display`.
- `tools/cs_inspect/src/evidence.rs` (extended): `check_ledger` front end and
  `synthetic_fingerprint_index` matching `synthetic_claim_fixture`.
- `tools/cs_inspect/tests/accept_f01_b_ledger_validation.rs` (new): ten
  `accept_f01_b_*` tests over the production path.
- Observable failure if the implementation is removed or stubbed: a claim
  whose evidence fingerprints container `X` at digest H1 still reports
  `valid` after `X` is re-observed at H2 (AC02); the dependent claim is never
  invalidated. Covered by
  `accept_f01_b_changed_fingerprint_invalidates_dependent_claim`.

## Design decisions

- **Dependencies are keyed by (kind, container), not digest alone.** An
  evidence record depends on the asset its `ObservationLocator.container`
  names, hashed in the role `Fingerprint.kind` describes. Two containers that
  happen to share a digest do not invalidate each other
  (`accept_f01_b_same_hash_other_container_stays_valid`); the same container
  under a different kind is a different dependency.
- **Validation is a report, not a rewrite.** `validate_ledger` never mutates
  the claim set: invalidated claims keep their records and statuses, and the
  report names each stale evidence record with its recorded and observed
  digests. This mirrors non-negotiable behavior 1 (preserve, don't choose)
  and CLI-EVIDENCE's "changing content invalidates affected approvals" — the
  approval stops standing; history is not erased.
- **Disjoint dispositions.** A claim is `rejected` (per-record admission or
  set-level rule), `invalidated` (admitted but a dependency went stale) or
  `valid`. Rejected claims are not also invalidated — a refused claim has no
  standing left to revoke.
- **Duplicate ids are all rejected.** The ledger refuses to pick the first or
  last occurrence. Same for `FingerprintIndex::from_observations`: conflicting
  observations of one (kind, container) are a contradiction in the input and
  are refused, not averaged.
- **Unchecked is a third signal, not silence.** Fingerprinted evidence whose
  asset was not re-observed (`NotObserved`) or that carries no locator
  (`MissingLocator`) cannot be confirmed or refuted. It does not invalidate
  the claim — nothing falsified it — but `LedgerReport::is_clean` requires an
  empty `unchecked` list so a strict audit cannot pass on ignorance.
- **Per-record failures surface through `LedgerError::Record`** so one report
  carries both record-level and set-level rejections.
- No serialization format was added: the F01-A findings left the on-disk
  ledger shape open, and F01-B's sheet does not require it. `serde` stays out
  of `cs_types`.

## Recorded open questions (not guessed)

- Whether `audit` should treat `unchecked` as a hard failure in non-strict
  mode is a CLI decision for F01-C; the report carries the list either way.
- Adjudication state for `contradicted` claims (AC03 semantics) remains
  free text on `ClaimRecord.adjudication`; the typed machine is F01-C scope.
- If ledger inputs ever arrive as untrusted JSON, `ClaimId` grammar and
  digest formats are already enforced by the record types; a deserialized
  size cap is not yet specified.
