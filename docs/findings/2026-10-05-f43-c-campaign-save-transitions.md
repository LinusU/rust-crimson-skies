# F43-C: campaign save transitions

Stage F43-C wires the campaign state (`cs_sim::campaign`) into the F48 profile
save through `cs_app::campaign::CampaignRun`.

## What is wired

* Results (`report_outcome`), briefing (`advance_interludes`), loadout
  (`purchase`, `sell`) each compute on a copy of the state, commit one whole
  profile revision through `ProfileSession::commit_with`, and adopt the copy
  only after the commit returned. A refused or failed save leaves the run and
  the save as they were. A transition that changes nothing (a replayed outcome)
  writes nothing.
* A conflict retry re-reads the stored campaign revision; if another writer
  moved it, the transition is refused as `CampaignSaveError::Stale` instead of
  overwriting that progression.
* `CampaignState::snapshot` / `restore` (cs_sim) carry the state as plain data;
  `restore` refuses nodes outside the graph, paid items that are not owned and
  applied outcomes of another run.

## Storage shape (designed, not original)

The profile document has no campaign progress fields beyond run id, money and
an applied list, so the rest is stored as `campaign.*` extra fields
(`meta`, `node.N`, `unlock.N`, `paid.N`, `applied.N`) in format `v1`.

## Known limits

* Each record is one extra field and a document holds at most 512 extras
  (`MAX_LIST_ENTRIES`). The applied-outcome ledger grows with every attempt, so a
  very long run can hit that bound; the commit then fails loudly (no silent
  truncation). Whether the original campaign length reaches it is unmeasured
  (F43-D); if so the document schema (F48, `cs_types`) needs a campaign
  section of its own.
* `ProfileSession::record_outcome`'s own applied list is a separate, older
  path and is not used by `CampaignRun`; the campaign ledger is authoritative.
* The crash tests simulate process death by stopping the commit phases
  (nothing written; temp written and old file rotated; fully committed) in-process
  and reopening. F48-D's tests cover a real killed child at the storage layer.
* No original reward amount, retry or skip rule is involved; fixtures are
  synthetic.
