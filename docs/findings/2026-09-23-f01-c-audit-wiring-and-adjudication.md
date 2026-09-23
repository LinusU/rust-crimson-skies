# F01-C: audit wiring and typed adjudication — design notes

Date: 2026-09-23. Task: F01-C "Wire provenance into inspector and content
exports" (`specs/F01-evidence-ledger-provenance-and-reference-policy.md`,
`### F01-C`). Shared contract: `docs/contracts/CLI-EVIDENCE.md`.
Capabilities used: ordinary build/test only.

## Files and the one observable failure

- `crates/cs_types/src/evidence.rs` (extended): `Adjudication` typed state
  machine (`Open`, `Ruled { upholds, rationale }`),
  `ClaimRecord::adjudication` retyped `Option<String>` →
  `Option<Adjudication>`, new `ClaimError` variants
  (`ContradictionWithoutAdjudication`, `AdjudicationWithoutDispute`,
  `RulingOutsideDispute`, `EmptyRationale`), `FingerprintIndex::observations`
  so a stored index can feed the audit as producer input.
- `tools/cs_inspect/src/evidence.rs` (extended): `audit_claims`,
  `AuditError`, `AuditReport`, `ContradictionReport`, `DisputeEdge` — the
  producer→consumer wiring the `audit` command and content exports call.
- `tools/cs_inspect/tests/accept_f01_c_audit_wiring.rs` (new): six
  `accept_f01_c_*` tests over the production path.
- Observable failure if the implementation is removed or stubbed: two
  source-backed claims that disagree (one `contradicted` with an
  adjudication state) are audited and the report shows no contradiction view
  and no adjudication state (AC03). Covered by
  `accept_f01_c_disagreeing_claims_and_adjudication_preserved`; a mutation
  that drops the contradiction collection fails three of the six tests.

## Design decisions

- **The adjudication state is typed and required on `contradicted` claims.**
  F01-A/F01-B left `adjudication` as free text; AC03's "preserve the
  adjudication state" is only enforceable if the field exists and is typed.
  `Adjudication::Open` records the disagreement with no ruling;
  `Ruled { upholds, rationale }` records a ruling that names a party to the
  dispute — the claim itself or one of its `disputes` — and a non-empty
  rationale. A ruling narrows the disagreement; it never deletes or rewrites
  the losing claim's record.
- **Adjudication requires a dispute.** `Adjudication::Open` or `Ruled` on a
  claim with empty `disputes` is meaningless and rejected as
  `AdjudicationWithoutDispute`; `contradicted` claims without disputes keep
  failing as `ContradictionWithoutDispute` (check order unchanged).
- **`audit_claims` is the wiring, not a new CLI verb.** `main.rs` is outside
  this task's owner paths, so the deliverable is the inspector-side entry
  point the `audit` command calls when command parsing lands: claims +
  freshly measured `ObservedFingerprint`s in, `AuditReport` out. Content
  exports attach the same report as provenance; `cs_content` is likewise
  outside this task's owner paths and consumes `AuditReport` when it lands.
- **Error propagation over silent degradation.** The producer stage
  (`FingerprintIndex::from_observations`) can fail on conflicting
  observations; `audit_claims` aborts with
  `AuditError::ConflictingObservations` rather than folding the conflict into
  an empty index, which would silently mark every fingerprinted dependency
  unchecked. `AuditError` wraps the index error and exposes it via
  `std::error::Error::source`.
- **Teardown/retry.** The audit is stateless: it borrows the claim set,
  consumes the observations and returns a complete report. Nothing is
  allocated, opened or cached, so there is no partial state to tear down;
  retry is calling again with corrected observations (covered by
  `accept_f01_c_conflicting_observations_propagate_and_retry`).
- **The contradiction view includes rejected claims.** A `contradicted`
  claim that breaks a rule is still listed in `AuditReport::contradictions`
  with its edges and adjudication — a malformed disagreement stays visible
  instead of disappearing with the claim's standing.
- **Rejected contradictions keep truthful edge presence.** `DisputeEdge.
  present` reports whether the disputed id exists in the audited set; the
  ledger separately rejects dangling edges as `UnknownDispute`.

## Recorded open questions (not guessed)

- On-disk serialization of claims/observations/reports is still undefined in
  the pack; the audit works on in-memory records. When the ledger file
  format lands, `audit_claims` already accepts exactly the producer output
  (`Vec<ObservedFingerprint>`) a measuring pass would emit.
- Whether a non-strict `audit` should treat `unchecked` dependencies as a
  warning remains a CLI decision; `AuditReport::is_clean` keeps the strict
  semantics (unchecked is not clean).
- A `Ruled` state that keeps *neither* side standing (both sources wrong) is
  not modeled: `upholds` must name a dispute party. If a real adjudication
  needs "both rejected", extend the enum then.
