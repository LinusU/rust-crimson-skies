# Rally task #337: admissible observation methods for `verified_original`

Date: 2026-09-23. Task: #337 "Constrain verified_original evidence methods"
(follow-up found while reviewing F01-C; spec
`specs/F01-evidence-ledger-provenance-and-reference-policy.md`, contract
`docs/contracts/CLI-EVIDENCE.md`). Capabilities used: ordinary build/test
only.

## The gap

`EvidenceRecord::verifies_original` required only: source not
`SyntheticFixture`, an installation/content fingerprint, and a locator. A
`verified_original` claim could therefore stand on evidence recorded as
`ObservationMethod::Authored` or `Inference` — records that are not
observations at all — which is a fabrication vector: an authored value with
a real hash and locator would pass admission.

## Decision

`verified_original` requires a **direct-observation** method, implemented as
`ObservationMethod::is_direct_observation`:

- **Admissible:** `ByteInspection`, `ToolProbe`, `RuntimeObservation`.
  These mean someone read the fingerprinted bytes or watched the original
  program run.
- **Not admissible:** `DocumentReview`, `Inference`, `Authored`.

`DocumentReview` does **not** qualify even when the record names
`EvidenceSource::OriginalInstallation`: the method means the claim restates
what a cited source says rather than observing the data. Evidence of that
shape backs `documented` (or `observed_tool` for tool output), and a claim
that wants `verified_original` must cite a direct observation. `Inference`
and `Authored` are excluded for the stronger reason that they are not
observations.

The source check is unchanged: `SyntheticFixture` still can never verify,
and the fingerprint must still identify installation or content data. The
method rule is additive on top of hash + locator (AC01), it does not
replace them.

## Files and the observable failure

- `crates/cs_types/src/evidence.rs`: new
  `ObservationMethod::is_direct_observation`; `verifies_original` now
  requires it; `ClaimError::UnverifiedOriginalEvidence` text names the
  method requirement.
- `crates/cs_types/tests/accept_t337_verified_original_methods.rs`: task
  tests under the `accept_t337_` prefix.
- Observable failure if the constraint is removed: an `Authored` or
  `Inference` evidence record with `OriginalInstallation` source, a
  content fingerprint and a locator is admitted as `verified_original`
  instead of rejected (`accept_t337_authored_and_inferred_methods_cannot_verify`).

## Open questions

- Whether `EvidenceSource::Document` or `ToolRun` records should be
  additionally constrained when they carry installation/content
  fingerprints is left to the consumers that produce them (F02+); the
  fingerprint kind already restricts what they can identify.
