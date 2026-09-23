# F01-A: claim and evidence records — design notes and what stays open

Date: 2026-09-23. Task: F01-A "Define claim and evidence records"
(`specs/F01-evidence-ledger-provenance-and-reference-policy.md`).
Capabilities used: ordinary build/test only.

## Files and the one observable failure

- `crates/cs_types/src/evidence.rs` (new): `ClaimId`, `ClaimStatus`,
  `ContentHash`, `Fingerprint`/`FingerprintKind`, `SourceSpan`,
  `ObservationLocator`, `EvidenceSource`, `ObservationMethod`,
  `EvidenceRecord`, `TestOutcome`, `ClaimRecord`,
  `ClaimRecord::validate`, `ClaimError`.
- `tools/cs_inspect/src/evidence.rs` (new, exposed through the new
  `src/lib.rs`): `check_claims`, `ClaimReport`/`ClaimRejection`,
  `diagnostic_lines`, `synthetic_claim_fixture`. `Cargo.toml` gains the
  `cs_types` path dependency so the check runs on canonical records.
- Observable failure if the implementation is removed or stubbed: a
  `verified_original` claim whose only evidence lacks a content hash or an
  observation locator is admitted instead of rejected with
  `ClaimError::UnverifiedOriginalEvidence` (test
  `accept_f01_a_verified_original_needs_hash_and_locator` /
  `accept_f01_a_unverified_original_is_rejected_by_name`).

## Design decisions

- **Two status vocabularies are deliberately not conflated.** The sheet's
  claim statuses (`documented`, `observed_tool`, `verified_original`,
  `inferred`, `designed`, `unknown`, `contradicted`) live on
  `ClaimRecord.status`. The CLI-EVIDENCE verification levels
  (`implemented`, `checked`, `verified_original`, `release_approved`) belong
  to acceptance *reports* (`schemas/evidence.schema.json` `claim` field) and
  are not modeled here; they are awarded by gates, not asserted by records.
- **Fingerprint kinds follow the schema vocabulary**: `Installation` and
  `Content` identify original data; `Artifact` (tool output, fixtures,
  reports) never does. `EvidenceRecord::verifies_original` additionally
  excludes `EvidenceSource::SyntheticFixture` so a mislabeled fixture hash
  still cannot back `verified_original`.
- **Hash and locator must coincide on one record.** Two evidence records
  carrying only a fingerprint and only a locator do not combine into a
  verification (`accept_f01_a_non_original_fingerprints_do_not_verify`).
- **`TestOutcome` is a separate field** with no coupling to `status`; the
  tests prove a green outcome cannot rescue an unverifiable claim.
- **Per-record vs set-level rules.** `ClaimRecord::validate` covers only
  what one record can violate. Duplicate ids, dependency invalidation on
  fingerprint change (AC02) and contradiction pairing across records (AC03
  semantics) are ledger concerns and belong to F01-B/F01-C.
- **`ContentHash::from_hex` rejects uppercase**: the schema pattern is
  `^[0-9a-f]{64}$`, so accepting it would emit records the schema refuses.
- The fixture fingerprint is the real SHA-256 of
  `fixtures/synthetic/flat-uncompressed.rof` (kind `Artifact`), not an
  invented digest.

## Recorded open questions (not guessed)

- The on-disk serialization of the ledger (JSON vs other) is undefined in
  the pack; F01-B owns it. No serde dependency was added speculatively.
- `ClaimRecord.adjudication` is free text for now; AC03's typed
  adjudication state is F01-C scope.
- Whether `ObservationMethod` needs further variants (e.g. differential
  reference-oracle runs) is left open until F13/F38 consumers exist.
