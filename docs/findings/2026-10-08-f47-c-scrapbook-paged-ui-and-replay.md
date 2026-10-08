# F47-C: the paged scrapbook screen and its replay launch

Designed (not measured from the original) integration of F47-A's catalog and
F47-B's persisted records into a screen, plus the replay launch.

* `crates/cs_app/src/ui/scrapbook/paged.rs` — `ScrapbookUi` holds only
  presentation state: a caller-declared page size, the projected pages, a page
  index, one selection by stable `ContentId`, and the ids currently on screen.
  It never awards anything: every fact comes from the `ScrapbookRecords` the
  caller passes in.
* **Producer.** `refresh_stored` reads the selected profile through
  `ui::scrapbook::stored` (F47-B) and re-projects; `refresh` does the same for
  records already in memory. No selected profile, a refused commit and a
  `scrapbook.` field the reader rejects propagate as `PersistError` and leave
  the screen exactly as it was.
* **Locale.** Titles resolve through the one `TextSession` that owns the
  selected locale (F51-C): `TextId::try_from_content` + `TextCatalog::resolve`,
  so a title id no locale answers stays `None`. Every refresh re-checks the ids
  the screen was showing with `ui::scrapbook::resolve_saved` and reports the
  ones that no longer resolve, and a selection whose id stopped resolving is
  dropped and reported instead of moving to another entry (AC03).
* **Consumer.** `launch` turns the selection into a `ReplayLaunch`: the F47-A
  `ReplayRequest` plus a `cs_types::asset_id::MissionScope` validated with
  `MissionScope::new`, and `load_target(world)` composes the `crate::loading::LoadTarget`
  the normal loading path issues its `LoadRequest` from. The world group is a
  parameter because the dependency closure and its target are application-
  declared (`crate::ui::front_end::LoadPlan`), not derivable from an entry.
* Teardown and retry: `close` is idempotent and empties the screen (a later
  `refresh` reopens it from the producer), a refused launch mutates nothing so
  the same press retries once the producer records the missing fact, a page
  index whose pages shrank is clamped, and `TextSession::switch_locale`
  refusing an unanswered locale leaves the previous locale in force.

## Designed choices, not original measurements

* **Page size and layout.** The original scrapbook's page size, arrangement and
  input bindings are unmeasured; the page size is caller-declared and the walk
  (`go_to_page`/`next_page`/`prev_page`, out-of-range refused) is new-engine
  design. F47-D audits it against the original.
* **Variant → mission scope.** A link's `variant`, when present, is the
  mission-kind element the load runs under (`m1-night` for `mission/m1`'s link),
  otherwise the mission itself. F47-A already recorded that how the original
  names variants is unmeasured; this mapping is the designed one and is what
  F47-D must check. A mission key the scope grammar refuses (for example a
  leading `-`, or a key longer than the 64-byte label bound) is reported as
  `LaunchError::Scope` with the offending entry, never silently reshaped.
* **Selection semantics.** Selection is by stable id and never moves the page;
  hidden entries cannot be selected; a locked but shown entry can be selected
  and the launch is what refuses it. The membership test reads the projection,
  not the saved-id buffer: `restore` overwrites that buffer with a stored
  screen state, and until the following `refresh` re-projects it the buffer
  says nothing about what the screen is showing (review correction —
  `accept_f47_c_select_reads_the_projection_not_a_pending_stored_state`).

## Known limit

`Screen::Scrapbook` in `crates/cs_app/src/ui/front_end/mod.rs` has transitions
(`Cabin`/`Results` → `Scrapbook`, `Scrapbook` → `Back`) but no paging,
selection or launch action, and `Action::Launch` has no `Scrapbook` row.
Dispatching the screen is outside this task's owner paths
(`crates/cs_content/src/scrapbook.rs`, `crates/cs_app/src/ui/scrapbook/`,
`crates/cs_sim/src/records.rs`, `tests/`, `docs/findings/`), so it is filed as
**F47-C1 (#760) “Dispatch the front-end Scrapbook screen to the paged scrapbook
UI”**, which depends on #200, instead of being reached into here. Affected
content: the scrapbook screen stays unreachable from the running front end
until that task lands, so no scrapbook page, memento or replay link can be
exercised by ordinary play yet and AC04/F47-D stays gated on it. Until then,
the profile-wide limit recorded in
`docs/findings/2026-10-08-f47-b-scrapbook-persistence.md` (512 `extra` entries)
still bounds how long a profile's dedup ledger can grow.
