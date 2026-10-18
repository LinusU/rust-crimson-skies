# F43-V3: the results screen feeds both record stores in one revision

Task #1389. Resolves the two limitations of
`2026-10-13-f43-v2-campaign-scrapbook-record-reconciliation.md` that named it.

* `CampaignRun::report_mission_record` (`crates/cs_app/src/campaign.rs`) runs
  `cs_sim::mission_records::apply_mission_record` inside one
  `ProfileSession::commit_with`: the campaign snapshot and the `scrapbook.`
  fields are written together, **one profile revision** (the campaign-first
  fallback was not needed). The scrapbook is read from the document inside the
  commit, so a conflict retry recomputes both halves.
* The front-end flow's `apply_outcome` calls it under `RecordRule::DEFAULT`
  (no per-mission direction is invented; the original's is unmeasured, F47-D).
* The mission-succeeded fact is noted with the record, as F47-B's
  `record_mission` does.
* A replay that the campaign already holds but the scrapbook lacks finishes
  the scrapbook half without paying twice; a complete replay writes nothing.
* `report_outcome` (campaign only) remains for the F43-C pins.

Still open: the kind-policy asymmetry (a failed run's score can raise the
scrapbook best) — F47-D.
