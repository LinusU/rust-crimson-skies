# F29-C.3c: what an extraction records as a campaign and scrapbook outcome

Date: 2026-10-18. Task: F29-C.3c "Decide what an extraction records as a
campaign and scrapbook outcome" (#1427). Follow-up of
`docs/findings/2026-10-14-f29-c-3b-score-wiring.md`. Capabilities used:
ordinary build/test only; no original run, no `private/evidence/` report.

## Decision: the refusal is kept

`cs_app::score::mission_outcome` and `mission_result` keep refusing
`TerminalOutcome::Extraction` with `OutcomeRecordError::Extraction`. No
`Succeeded`/`Failed`/`Aborted` default is introduced (AGENTS.md rule 4).

## Evidence searched

- `docs/research/` (`FINDINGS.md`, `FORMAT-NOTES.md`, `SOURCES.md`),
  `docs/findings/`, `data/owner-static/engine-facts.toml` and the F39/F63
  specs: the only statement is F39 "extraction ... are distinct" from
  success and failure. Nothing says which record outcome an extraction
  writes.
- `cs_script::ir::Outcome` has `Succeeded`, `Failed`, `Aborted`; no
  extraction variant exists, and no measured lowering produces one.
- `cs_sim::objectives::terminal` documents the F39 `Extraction` ordering as a
  designed policy, not a measurement.
- No owner decision for a designed rule has been recorded.

## Affected content (stays unresolved)

Every run of every original mission that settles `TerminalOutcome::Extraction`
produces no campaign record and no scrapbook record. Claim id:
`f29-c3c.extraction-record-unmeasured` (status: unknown, no provenance
earned). Resolving it needs either an original-run capture showing what the
original's campaign/scrapbook shows after leaving the mission area, or an
explicit owner-approved designed rule; both are outside what an agent can
supply (`human_play`/original run).

## Test

`crates/cs_app/tests/campaign/f29_c_3c_extraction.rs`, prefix
`accept_f29_c_3c_`: pins the refusal of both builders and that success and
failure still record. Fails if the refusal is replaced by any mapping.
