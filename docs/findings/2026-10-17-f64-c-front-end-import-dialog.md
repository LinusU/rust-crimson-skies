# F64-C's migration report drawn by the front-end import dialog

Date: 2026-10-17. Task: #757 "Draw the F64-C import report in the front-end
import dialog" (follow-up to F64-C, task #256;
`specs/F64-legacy-custom-aircraft-and-optional-save-import.md` `### F64-C`,
presented through `specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`
`### F45-A`/`### F45-B`/`### F45-D`). Shared contracts:
`docs/contracts/UI-NETWORK.md`, `docs/contracts/STATE-TRANSACTIONS.md`.

Task test prefix: **`accept_f64_c_import_dialog_`** (9 tests, all unignored;
it also matches F64-C's own `accept_f64_c_` selection).

Owner paths used: `crates/cs_app/src/ui/front_end/` (the new
`import_dialog.rs` plus `mod.rs`, `machine.rs`, `screens.rs`, `flow.rs`,
`paths.rs`), `crates/cs_app/tests/` (the new
`accept_f64_c_import_dialog.rs`, plus the F45 decks/walks that enumerate every
screen and every supplied input: `tests/ui/{main,screens,stage_d}.rs`),
`docs/findings/`. **Wiring edits:** `crates/cs_app/src/lib.rs` gained one doc
sentence inside the existing `pub mod ui { .. }` block naming
`[front_end::ImportDialog]` — no logic there.

Capabilities used: **ordinary build/test only**. Every fixture is designed
fixture content (`tests/f64_c_support/`), read through the production
`read_legacy_profile`, resolved by the production `LegacyIdMap`/`Catalog` pair
and judged by the production `ConstructionRules::validate`, under the explicit
fixture admission. No original file was read for this task and no original
executable was run, so nothing here is `verified_original`: F64-B measured
that the installation ships no legacy artifact and this build has measured no
byte layout for any class, so a retail offer is still refused by name
(`no_measured_layout`) before a byte is judged — F64-C's retail test pins
that, unchanged.

## What this task adds

`crates/cs_app/src/ui/front_end/import_dialog.rs` is the **screen** F64-C
deliberately left open ("no front-end screen: `crates/cs_app/src/ui/front_end/`
is not an owner path either; the module is the boundary the screen will draw").

- `ImportDialog` owns exactly what a screen may own: an immutable **snapshot**
  of the `ImportContext` it was opened with (F64-C's own words: "the screen
  builds one of these when it opens and hands it to every `ImportFlow::offer`
  until the dialog closes"), the **offered source** (bytes, declared proposal,
  the layout found for the class, its limits, its optional field map, the new
  `TargetProfile` and the installation identity), and the `ImportFlow` whose
  attempt is on screen. `ImportDialog::open(context, offer)` runs the first
  attempt; `retry`, `confirm` and `cancel` are the three decisions.
- **`Screen::ImportDialog`** is a screen of the F45-A table, reached from
  `Screen::ProfileSelect` by **`Action::OpenImportDialog`** (the profile
  screen's own "import an old plane/save" button). Its rows are
  `ConfirmImport → RequestKind::ConfirmImport`, `Retry → None` and
  `Cancel → ProfileSelect → None`; `Cancel` is the screen's escape, so
  `validate_table` proves it is reachable, escapable and can reach the menu.
- **Confirm asks the domain only when the report says it may.** `perform`
  calls `ImportDialog::confirm` (which is `ImportFlow::confirm`, i.e.
  `MigrationView::confirmable()` first) and only then emits
  `Effect::Request(Request::ConfirmImport)`. A refusal comes back as
  `Refusal::Import(<the producer's own ConfirmError>)` — `Refused { code }`,
  `Unsupported { reason }`, `NoConformingRecord`, `NothingToConfirm` — with no
  effect and the report still on screen. With no source offered, `Refusal::
  NoImportAttempt`; nothing is invented.
- **The screen never re-derives a row.** `ImportDialog::lines` returns the
  flow's own `ReportLine`s in producer order, so a code, a record's identities,
  a `LimitBreach` pair or a verdict cannot be reformatted by the UI. The
  `fixture_admitted` marker is not special-cased: when the producer marked the
  report so, its `ReportLine::Notice { code: "fixture_admitted" }` is already
  in the stream and is drawn like every other row. The acceptance test pins
  this by comparing the screen's rows with the rows F64-C's own `ImportFlow`
  renders for the same offer.
- **No write path.** The dialog holds bytes and a relative spelling, never a
  file handle, a writer or a host path. The acceptance test drives every
  action the screen offers (including a confirmed one) over a real filesystem
  and asserts the source's bytes, size and timestamp and a destination tree are
  byte-for-byte unchanged.

## Design decision: a table screen, not a hidden modal

The alternative was a modal sub-state of `Screen::ProfileSelect` (like
`FrontEnd::pending_discard`). It was rejected: the task's goal is that the
dialog be **in the state table**, and a table screen is what makes the
navigation review (`paths.rs`) and the deck validation (`ScreenDeck`) apply to
it — every row classified, every hotspot a working button, an escape, a path
back to the menu. The cost is that F45's own enumerations had to learn about
one more designed screen:

- `tests/ui/screens.rs`: the preflight deck authors the profile screen's
  `open-import-dialog` button and a new `art-import-dialog`
  (`confirm-import`, `retry`, `cancel`). The deck's equality with "every
  preflight screen", the staged walk and its `cancelled_at` set now include
  `ImportDialog`, so the existing test still proves exactly what it proved
  before (Cancel at every preflight screen asks the domain for nothing)
  about one more screen. The order is the *authored* order, so the F45-B
  focus test is unaffected.
- `tests/ui/stage_d.rs` and `paths.rs::NavigationInputs`: the dialog's
  `ConfirmImport`/`Retry` rows are behind a declared input, exactly like the
  flight check's loadout and the construction screen's open draft — "an offer
  is what the presentation hands the machine, never a table row". Without it
  the inputless walk reaches `ImportDialog` and the two rows are refused
  (`no_import_attempt`), which is the honest classification; with it the walk
  applies them and `RequestKind::ConfirmImport` is recorded as asked. The
  review's completeness rule is unchanged. `NavigationInputs` therefore lost
  its `PartialEq`/`Eq` derives (the dialog holds a migration report, measured
  data whose equality the producer defines), which nothing depended on.

## The tests (`accept_f64_c_import_dialog_*`, 9)

| test | what it pins |
| --- | --- |
| `..._a_refusal_renders_the_producers_own_code` | a hostile table's `unreadable` reaches the screen verbatim, with the `nothing_written` notice; no verdict; the owner action is refused with that same code |
| `..._a_report_renders_its_verdict_records_and_fixture_marker` | verdict row, record rows with the **declared** identities, the producer's `fixture_admitted` marker, and the screen's rows `==` the F64-C flow's rows |
| `..._confirm_asks_the_domain_only_when_the_report_is_confirmable` | the request, and the retained `ConfirmedImport`, only when `confirmable()` is `Ok`; `NoConformingRecord` and `NoImportAttempt` otherwise, report left on screen |
| `..._cancel_tears_the_attempt_down_and_asks_for_nothing` | Cancel leaves the screen, drops the attempt and conserves the attempt counter |
| `..._retry_reruns_the_pipeline_from_a_clean_stage` | attempt counting, deterministic re-read, no request asked |
| `..._no_screen_path_writes_anything` | source bytes/size/timestamp and a destination tree unchanged after every action |
| `..._an_unsupported_document_reaches_the_screen_with_its_reason` | non-negotiable 5: `unsupported` with `id_not_mapped` on screen, `ConfirmError::Unsupported` on the owner action, never a blank profile called imported |
| `..._is_the_same_screen_the_presentation_drives` | the same screen through `ScreenSession` (an empty deck: the dialog is not deck-dependent), open/read/confirm/cancel |
| `..._the_flow_refuses_a_confirm_it_cannot_persist` | `FrontEndFlow` reaches the screen, `ConfirmImport` comes back `FlowError::ImportNotPersistable`, and the domain-before-transition order leaves the report on screen and nothing confirmed; Retry and Cancel work through the flow |

Each test drives the real front end: `FrontEnd::new`, `apply(InstallVerified)`,
`apply(NewProfile)`, `apply(OpenImportDialog)`, `open_import_dialog(...)`, then
the dialog's own `Action`s and the `Effect`/`Request` stream they produce. The
screen's state assertions are the machine's own.

**Discriminating (measured, not argued):** with
`effects.push(Effect::Request(Request::ConfirmImport))` removed from
`FrontEnd::perform`, `..._confirm_asks_the_domain_only_when_the_report_is_confirmable`
fails; with `ImportDialog::cancel` made a no-op and `ImportDialog::lines` made
to return nothing, 5 of the 9 fail.

## Deliberately not done

- **No persistence.** `Request::ConfirmImport` is emitted and refused by the
  flow's domain with `FlowError::ImportNotPersistable`: writing a confirmed
  import into a new profile is the profile store's transaction and belongs to
  Rally **#756** ("persist a `ConfirmedImport` into a new profile
  atomically"), which owns `crates/cs_app/src/profile.rs`. The refusal is by
  name rather than a silent success, so no screen can report an import nothing
  carried. `FrontEndFlow::open_import_dialog` and the `Cancel`/`Retry` rows work
  end to end today; only `Confirm` on that flow refuses until #756 lands.
- **No hover/focus *rendering*.** The dialog's rows are text and codes with no
  authored artwork: which original image or font draws an import dialog is not
  read anywhere (#742 resolved the original front-end layout only for the
  screens F45-B.1 measured, and no import dialog is among them). The screen
  presents through the table and its own rows; F45-B's deck machinery draws
  whatever art a loader supplies.
- **No original capture.** An import dialog does not appear in the original
  front-end screen inventory F45-D measured, so there is nothing to capture;
  capturing this screen would be a capture of this renderer, not the original.
