# F47-B: scrapbook persistence in the profile document

Designed (not measured from the original) persistence of `ScrapbookRecords`.

* Stored as `scrapbook.*` entries of `ProfileDocument::extra`: `fact.N`, `rule.N`,
  `slot.N`, `applied.N` and `memento`. Values are space-separated tokens (content
  ids, difficulty ids and outcome ids contain no space); an outcome is
  `profile,run,session,event-session,tick,source,sequence`. The same state always
  yields the same fields.
* Every change goes through `ProfileSession::commit_with`: load the stored
  scrapbook, apply one change, replace only the `scrapbook.` fields, write one
  whole revision. A conflict re-applies the change to the stored revision. A
  replayed `OutcomeId`, a repeated stunt or a repeated memento choice writes nothing.
* An unreadable or unknown `scrapbook.` field refuses the change and is never
  overwritten.

## Known limit

The profile document holds at most 512 `extra` entries of 256 bytes. The dedup
ledger grows by one entry per (outcome, mission) pair, so a long-lived profile
reaches that bound; the commit then fails with `TooManyEntries` (propagated as
`PersistError::Session`), it does not drop ledger entries. Pruning the ledger or
moving the scrapbook to its own save section needs a decision on the profile
schema (F48, protected paths) and is not guessed here.
