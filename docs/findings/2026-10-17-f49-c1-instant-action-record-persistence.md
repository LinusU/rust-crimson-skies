# F49-C1: persisting the Instant Action record scope

Task F49-C1 (`accept_f49_c1_`). Follows up
`2026-10-08-f49-c-instant-action-screen-flow.md`. Designed behaviour on
synthetic fixtures; the original game's Instant Action record format is
unmeasured (F49-D).

## What was built

* `crates/cs_app/src/ui/instant_action/persist.rs`:
  `ProfileSession::record_instant_action` settles one `IaRecordEntry` through
  `ProfileSession::commit_with` as one whole profile revision.
  `ProfileSession::instant_action_records` / `read_records` read the scope
  back as an `IaRecordBook`.
* Storage: one `extra` field per session, `ia.record.<generation>` =
  `subject;seed;result;ended_tick;attempt`. The owning profile is the document's
  own identity (`campaign_profile_id(kind, slot)`), so a record cannot be filed
  under another pilot. No `campaign.*` field, `campaign.applied_outcomes`,
  currency or `OutcomeId` is read or written.
* Refusals run inside the commit's change, so they hold after a conflict retry
  too: `IaRecordError::ForeignProfile`, `IaRecordError::DuplicateGeneration`.
  A stored field that does not parse is `IaPersistError::Damaged`, never
  skipped.
* Generation authority: `InstantActionFlow::resume(book, catalog)` mints
  generations above the persisted book's highest, resolving the note on #775
  that per-flow counting from 0 would collide with stored entries.

## Limits

* The `extra` list is bounded (512 entries shared with every other extra
  field); a profile that fills it makes the commit fail as a session error
  rather than dropping old records. Retention policy is unmeasured and not
  decided here.
* The layout is a designed vocabulary of this build; it is not original data.

## Verification

`cargo test --locked -p cs_app -- accept_f49_c1_ --include-ignored` runs 5
tests in `crates/cs_app/tests/accept_f49_c1_ia_record_persistence.rs`
(restart round trip, campaign unchanged with a moving control, foreign and
duplicate refusal, resumed generations, damaged record).
