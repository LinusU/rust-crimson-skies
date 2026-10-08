# F47-D: the retail scrapbook audit — pages, artwork, mementos and replay links

Date: 2026-10-08. Task: #201 / F47-D "Verify all original scrapbook content and
unlock paths" (`specs/F47-scrapbook-records-mementos-and-mission-replay.md`,
section `### F47-D`). Shared contracts:
[`STATE-TRANSACTIONS`](../contracts/STATE-TRANSACTIONS.md) and
[`CLI-EVIDENCE`](../contracts/CLI-EVIDENCE.md). Required capabilities: `gpu`
and `retail` — both present (`CS_CAPABILITIES=retail,gpu,audio`), both used.
Task test prefix `accept_f47_d_`. Evidence: `docs/findings/evidence/F47-D.json`,
artifacts in `private/evidence/F47-D/` (git-ignored; original pixels and the
exported table never enter Git).

Implemented by `bunny-2` (agent `bunny-2`, session of 2026-10-08). This file is
the implementer's record; it is not a review and awards no
`verified_original`/`release_approved`. `retail` below means read access to the
owner's installation, never a run of the original executable: no agent ran
`crimson.exe`, and no capture here is a screenshot of the original renderer.

## One observable failure (listed before editing)

F14-D.8 populated the `scrapbook_item` collection and then recorded, as its own
unresolved limitation, that "*nothing normalizes its image, cell, page or
localization fields, so no downstream consumer can build the page yet*"; F47-A
defined the types, F47-B the persistence and F47-C the screen, and **nothing in
the repository could read the original table into pages or audit a declared
catalog against it.** Concretely, with this stage's production code removed:

* `cs_content::scrapbook::DiscoveredScrapbook::discover` does not exist, so no
  code can turn `ASSETS/SCRAPBOOK.CSV` into the 25 pages its keys spell, join
  each item's picture to the container's own members, or say which items name a
  picture the installation does not hold;
* `cs_content::scrapbook::audit` does not exist, so nothing can answer AC04 —
  "audit every discovered scrapbook page, memento and replay link against
  original progression" — for a declared catalog, a declared memento or a
  declared replay link;
* and no test can pin the measured shape of the owner's installation (461
  records, 25 pages, 294 pictures, 24 campaign missions).

`accept_f47_d_every_discovered_record_is_grouped_into_the_pages_its_keys_spell`
fails first: it calls `DiscoveredScrapbook::discover` on an authored temporary
installation and asserts the grouping, the picture join and every named gap.

## Files

Owner paths only (`crates/cs_content/src/scrapbook.rs`,
`crates/cs_app/src/ui/scrapbook/`, `tests/`, `docs/findings/`); no `Cargo.toml`
or `Cargo.lock` change was needed, so there is no wiring edit to list.

* `crates/cs_content/src/scrapbook.rs` — the stage's production code, next to
  F47-A's types:
  * `ARTWORK_DIRECTORY`, `MEMENTO_IMAGE_PREFIX`, `CAPTURE_FORMATS` — the
    container's artwork directory, the picture-name prefix the original
    memento-selection script itself compares, and this stage's **declared**
    capture preference (the table's `ImageType` is one of F12-I's unknown-kind
    positions, so nothing in the member says which stored file a picture comes
    from);
  * `ORIGINAL_UNLOCK_FIELDS` / `ORIGINAL_REPLAY_FIELDS` /
    `ORIGINAL_MEMENTO_RECORDS` — the three zeros this audit is built on, each
    documented with the field list it comes from;
  * `ScrapbookSourceError` — a refusal that always names the source that
    refused (missing archive, missing member, unreadable extent, a member with
    no record the documented schema covers);
  * `DiscoveredItem` / `DiscoveredPage` / `DiscoveredScrapbook` with
    `discover(install_root)` and `json()` — production discovery: production
    installation discovery, the production ROF mount
    (`cs_assets::rof::mount_rof_into`), the production keyed-list reader
    (`cs_content::config::ConfigDocument`), grouping by each record's **own
    entry key**, and a picture join over the container's own member spellings;
  * `AuditReport` / `audit(discovered, declared, progression)` / `json()` and
    the three `*_BACKING` constants that state what the original does not
    answer.
* `crates/cs_content/tests/accept_f47_d_scrapbook_audit.rs` (new): the four
  `accept_f47_d_*` tests — three synthetic (authored table in a temporary
  installation, production discovery and production audit) and the retail one
  (`#[ignore = "requires CS_GAME_DIR"]`).
* `crates/cs_app/tests/scrapbook/stage_d.rs` (new) +
  `crates/cs_app/tests/scrapbook/main.rs` (the `mod stage_d;` line and the
  target's header paragraph — wiring): the retail `gpu` capture test.
* `crates/cs_content/tests/evidence_report_f47_d.rs` (new): the evidence
  harness, deliberately **not** named `accept_f47_d_*`.
* `docs/findings/evidence/F47-D.json` (new): the validated acceptance report;
  this file.

## What the installation measures

Everything below comes from the production discovery/audit run the acceptance
test performs over `$CS_GAME_DIR` (pinned by `accept_f47_d_retail_…`) and from
the independent second reading the evidence harness does of the same bytes. No
text of the member is reproduced here — only counts, structure, identifiers and
digests.

| measurement | value |
| --- | --- |
| decoded member (`ASSETS/SCRAPBOOK.CSV`) SHA-256 | `28b5144c54120f52c36717a3f1e094cb75845ecb1f854334a5686d5f6c6af5c1` (the digest F12-D, F12-H, F12-I and F14-D.8 recorded) |
| records the documented schema covers | **461** |
| pages (the entry key's first component) | **25**, numbered `0..=24`, each a contiguous run |
| items that belong to a page | 461 (every record) |
| items whose picture the container holds | **294** |
| items whose picture the installation does not hold | **167**, all named `Snap_*`, counted as `item_without_artwork` |
| pages with no picture at all | **0** |
| pictures found outside `ASSETS/GRAPHICS/SCRAPBOOK/` | 0 |
| records the schema does not cover / repeated or ambiguous keys | 0 / 0 / 0 |
| distinct table images shaped like the memento script's own picture name (`MS_P_*`) | **15** |
| campaign mission rows the audit compares against | **24** |
| `Objective` converted to a number | 461 of 461 |

The identity cross-check is asserted, not asserted about: the 461
`id_key`s this discovery derives (`install_file_key` of each entry key) are
**equal as a set** to F14-D.8's 461 `scrapbook_item` catalog rows, which are
produced by a different code path over the same member. Two readers of one
member that disagreed would fail the retail test.

The audit of the installation as it stands (an empty declared catalog, because
the runtime declares none — see "Unknowns"):

| audit field | value |
| --- | --- |
| `declared` / `declared_matched` / `undeclared_items` | 0 / 0 / **461** |
| `unlock_known` / `unlock_unknown` / `unlock_unbacked` | 0 / 0 / 0 |
| `replay_links` / `declared_mementos` | 0 / 0 |
| `original_unlock_fields` / `original_replay_fields` / `original_memento_records` | **0 / 0 / 0** |
| `is_complete()` | **false**, by construction: `undeclared_items != 0` |

`is_complete()` being false *is* the audit's answer: the stage reports the gap
instead of closing it with invented entries.

## The GPU half

`accept_f47_d_retail_gpu_every_discovered_page_draws_a_measured_frame` walks
the 25 discovered pages, takes each page's first item whose picture resolves,
decodes that original image through `cs_app::ui::front_end::FrontEndScreens`
(the F45-D production reader) and draws it with
`cs_app::ui::front_end::capture_artwork` on the real adapter (Apple M3 Pro,
Metal, in this run), reading each PNG back and pairing frame↔picture in both
directions. Result: **25 pages captured, 0 refused flat, 0 pages without a
picture**, one `f47-d-page-NNN-<stem>.png` per page in
`private/evidence/F47-D/`, hashed as evidence artifacts.

This proves the original artwork of every discovered page decodes and draws in
this engine. It does **not** claim the original's layout: the items' `X`, `Y`,
`DrawOrder`, `Alpha`, `Zoom*` and rectangle fields are read and reported, but
nothing composites a spread, because the member documents no layout rule and
`Width`/`Height` are `0` in the shipped records.

## Design decisions a reviewer should check

1. **A table that cannot be read is an error, never an empty reading.** A
   missing archive, a missing member, an undecodable extent and a member that
   holds no record the documented schema covers all return
   `ScrapbookSourceError`, each naming its source. The keyed-list reader would
   happily accept a member with no records in it, so the last case is checked
   explicitly — otherwise an unreadable table would look like an empty
   scrapbook, and an audit of nothing would look like an audit of an empty
   scrapbook.
2. **Identity is the entry key, never the line**, encoded through the public
   `install_file_key` so it is the same identity F14-D.8's catalog rows use.
   A key the table declares twice with identical fields is one item plus a
   `duplicate_entry_key` count; a repeated key whose records disagree yields no
   item and an `ambiguous_entry_key` count (the rule F14-D.8's review fixed for
   this very member, reused rather than reinvented). A key that is not
   `<page>_<spread>_<slot>` is counted as `entry_key_not_page_structured`, not
   guessed into components.
3. **"Page" is a labelled reading of a measured structure.** The member
   documents no key grammar. What is measured: every one of the 461 keys is
   `<digits>_<digits>_<digits>`, the first component yields exactly 25
   contiguous runs `0..=24`, 21 of those runs are introduced by the member's
   own section comments, and the sibling picture naming (`SB_<page>_<spread>_…`)
   agrees. Whether the original *calls* the component a page is recorded as
   unmeasured (below), and the grouping is by the component alone, so a
   different reading would change the page list rather than silently move
   items.
4. **Pictures are found by name, not by a path this stage invented.** Every
   member of the container is indexed by its file stem (case-insensitively); an
   item's `ImageName` resolves to the members with that stem, and a member
   outside `ARTWORK_DIRECTORY` is still found but counted as
   `artwork_outside_artwork_directory`. The capture preference
   (`.png`, `.jpg`, `.jpeg`, `.bmp`, `.tga`) is a **declared** constant of this
   stage, not an original rule; on the owner's installation every one of the
   294 resolves to a `.png`.
5. **The audit reports; it never fills a hole.** The table documents no unlock
   field, so every `Known` rule of a declared entry is counted
   `unlock_unbacked` — a rule can only have come from an authored source — and
   `is_complete()` refuses to pass while any is present. Rule subjects and
   replay targets are checked against the real campaign rows, so a declaration
   that names content the installation does not hold is listed by id. Mementos
   are counted on both sides (`declared_mementos`,
   `original_memento_records = 0`) rather than assumed.
6. **The retail test audits an empty declaration on purpose.** The runtime
   really does declare no entry, and the test asserts `declared == 0` and
   `undeclared_items == 461` so the gap cannot drift silently into a passing
   number. When F47-C2 (#773) lands, those two assertions move with it.
7. **The captures are evidence-gated.** A flat frame is refused by name and
   leaves no file (the F45-D gate, reused), and the frame↔picture pairing in
   both directions is asserted, so a capture that ignored its pixels fails even
   if the corpus legitimately repeats a picture.

## Test selection and sensitivity

`cargo test --workspace --locked -- accept_f47_d_ --include-ignored` discovers
and runs **5** tests (exit 0): three synthetic, one retail read/audit, one
retail+GPU capture. The full four checks pass locally: `cargo fmt --all --
--check`, `cargo clippy --workspace --all-targets --all-features --locked --
-D warnings`, `cargo test --workspace --locked`, and the selection above.

| test | what it fails without |
| --- | --- |
| `accept_f47_d_every_discovered_record_is_grouped_into_the_pages_its_keys_spell` | the grouping, the member-order, the picture join, the capture preference, any named gap, or the member fingerprint |
| `accept_f47_d_the_audit_reports_every_declaration_against_the_original` | the matched/undeclared/fabricated split, the unbacked-rule count, the missing subject or mission, the memento count, or the three original-field zeros |
| `accept_f47_d_a_table_that_cannot_be_read_is_an_error_not_an_empty_reading` | any of the three refusals: a missing archive, a missing member or a member with no record would then read as an empty table |
| `accept_f47_d_retail_the_installation_scrapbook_is_discovered_and_audited` | the record/page/artwork counts, the member digest, the identity agreement with F14-D.8's rows, the 24-mission progression, or the empty-declaration gap |
| `accept_f47_d_retail_gpu_every_discovered_page_draws_a_measured_frame` | fewer than 25 captures, a page with no picture, a capture that ignored its pixels, or a refused frame left on disk |

Sensitivity was checked by mutation, then reverted (each run:
`cargo test -p cs_content --test accept_f47_d_scrapbook_audit -- <test>`):

| mutation | result |
| --- | --- |
| the artwork join returns an empty list (no picture lookup) | the synthetic test fails on *"the picture is the container's own member, by name"* (`[]` vs `ASSETS/GRAPHICS/SCRAPBOOK/ART_A.PNG`), and the **retail** test fails on the measured `294` (`left: 0`) |
| `audit` counts every declared entry as matched (the unmatched arm removed) | the audit test fails on `declared_matched` (`4` vs `2`) |
| an explicit `Unknown` rule is also counted as `Known`/unbacked | the audit test fails on `unlock_known` (`4` vs `3`) |

## What remains unknown (recorded, not guessed)

- **The runtime declares no scrapbook entry at all.** There is no production
  `ScrapbookCatalog` in the tree, so all 461 original items are reported
  `undeclared`, `is_complete()` is false, and no scrapbook page, memento or
  replay link is reachable by ordinary play. **Affected content:** the whole
  scrapbook UI's data and AC04's "complete" reading. **Resolving tasks:**
  **#773 / F47-C2** (declare the original catalog without inventing rules) on
  top of **#760 / F47-C1** (dispatch the front-end scrapbook screen, still
  `todo`), which F47-C already recorded as the gate on exercising a page by
  ordinary play.
- **No original unlock field exists to read.** `Objective` is the only
  documented name that could carry progression; all 461 values convert to
  numbers (the distribution, measured in this stage's private scratch pass over
  the exported member and **not** pinned by a test, is `-12..31` over 26
  distinct values), and its meaning is unmeasured, so every `Known` rule is
  reported `unlock_unbacked`. **Affected content:** every unlock path, hidden
  page and reward claim in F47 and F42. **Resolving task:** **#774 / F47-D.2**,
  with F38 (native behaviour bindings) and F13/F07-D (mission language) owning
  what the original engine evaluates.
- **No original mission field exists to read.** The original
  table-of-contents script names a replay control, which proves a replay exists
  but not what it launches, so a declared link's target can only be checked
  against the campaign rows, never confirmed as *the* original target.
  **Affected content:** every replay link's mission and variant in F47-C's
  launch. **Resolving task:** **#774 / F47-D.2**.
- **No memento record exists, and the memento set is native.** Every
  schema-covered record is a `Mission_Spread_Item`, and the original
  memento-selection script receives its picture name from a native callback
  (message 2150) this engine has not decoded, so the set, its order and its
  unlock state are unmeasured; the audit only counts the 15 `MS_P_*`-shaped
  pictures the table references, and `MEMENTO_IMAGE_PREFIX` documents that its
  own evidence is a single literal comparison in that script. **Affected
  content:** the cabin memento choice (spec F47 non-negotiable 5) and any
  declared `EntryKind::Memento`. **Resolving tasks:** **#774 / F47-D.2** and
  F38.
- **167 items name pictures the installation does not hold** (`Snap_*`), so
  they can never draw a frame; every page still has at least one picture the
  container holds. A declared catalog must carry that state rather than a
  placeholder. **Affected content:** those 167 items' artwork. **Resolving
  task:** **#773 / F47-C2**.
- **A page is this stage's reading** of the key's first component (see
  "Design decisions" 3); the member documents no key grammar. **Affected
  content:** the page grouping and numbering of every scrapbook screen.
  **Resolving task:** **#774 / F47-D.2**.
- **The captures are this engine's renderer drawing decoded original pixels.**
  No original executable ran in any agent session, so nothing here compares
  with what the original presents. **Affected content:** every visual fidelity
  claim for F47. **Resolving task:** REF-OWNER-FIRST-CAPTURE / **#358**, an
  owner-supplied original run.
- **No `human_play`/`human_review` evidence exists.** Those capabilities belong
  to the owner; the report's `claim` is `implemented` at most, and `retail` in
  it means read access only.

## Follow-ups filed

* **#773 / F47-C2** "Declare the original scrapbook catalog the runtime
  projects" (depends on #760).
* **#774 / F47-D.2** "Measure what the original scrapbook's `Objective` field
  and replay control do".

## Sources used

* `specs/F47-scrapbook-records-mementos-and-mission-replay.md` (stage
  `### F47-D`, AC04, non-negotiable behaviour 1, 4 and 5).
* `docs/contracts/CLI-EVIDENCE.md` and `schemas/evidence.schema.json`;
  `docs/contracts/STATE-TRANSACTIONS.md`.
* `docs/findings/2026-10-01-f47-a-scrapbook-records.md` (what F47-A declared
  as unknown, which this stage audits rather than closes),
  `docs/findings/2026-10-08-f47-b-scrapbook-persistence.md`,
  `docs/findings/2026-10-08-f47-c-scrapbook-paged-ui-and-replay.md` (the
  F47-C1 gate this stage inherits).
* `docs/findings/2026-10-03-f14-d-8-stunt-scrapbook-collections.md` (the 461
  rows, the container and member spellings, the duplicate/ambiguous-key rule
  and the recorded "not normalized" limitation this stage works on).
* `docs/findings/2026-09-29-f12-i-record-kind-schemas.md` (the sixteen
  documented fields, the three unknown-kind positions, the member digest) and
  `docs/findings/2026-10-02-f12-d-installation-wide-configuration-account.md`.
* `docs/findings/2026-10-07-f45-d-front-end-screen-capture.md` (the capture
  path, the uniform-frame gate, the frame↔picture pairing and the evidence
  harness pattern this stage reuses).
* `crates/cs_content::catalog::baseline` (`SCRAPBOOK_CONTAINER`,
  `SCRAPBOOK_MEMBER`, `install_file_key`, `retail_baseline` for the
  progression), `crates/cs_content::config` (`ConfigDocument`,
  `RecordSchema`, `RecordView`), `crates/cs_assets::rof` (`mount_rof_into`),
  `crates/cs_app::ui::front_end` (`FrontEndScreens`, `capture_artwork`).
* The owner's installation, read-only: `GOSDATA/ASSETS/crimson.rof`, its
  `ASSETS/SCRAPBOOK.CSV` member and its artwork members. The original
  scrapbook/memento scripts were opened read-only in the stage's private
  scratch to locate the replay control, the memento picture path and the
  `MS_P_` prefix; **no script line, record line or picture is committed
  anywhere**, and every claim taken from them is stated as a lead or as an
  explicit unknown.

## Review (2026-10-08)

Reviewed by `bunny-2` (agent `bunny-2`, Rally #201 review claim of
2026-10-08T11:51:33Z) in a **separate session whose context was fresh** and
never saw the implementation sessions, but under the **same agent name and the
same model** as the implementer — so per `AGENTS.md` this is same-agent review,
**not independent evidence** for format, mission-semantics or fidelity claims,
and no agent review replaces the owner's human approval. The identity is
recorded byte-for-byte in `review.identity` of `docs/findings/evidence/F47-D.json`
and in the harness literal that writes it
(`crates/cs_content/tests/evidence_report_f47_d.rs`), so the two cannot drift.

What the reviewer read: `specs/F47-scrapbook-records-mementos-and-mission-replay.md`
(section `### F47-D`, AC04, non-negotiable 1/4/5), `docs/contracts/CLI-EVIDENCE.md`
and `docs/contracts/STATE-TRANSACTIONS.md`, the production discovery and audit in
`crates/cs_content/src/scrapbook.rs`, all five `accept_f47_d_*` tests, the
evidence harness, and the F14-D.8/F12-I findings the measurements lean on.

**Fixed during review** (two commits, then everything was re-run):

1. `ScrapbookSourceError::Document`'s no-record reason carried a run of 18
   literal spaces — a broken line continuation in the source
   (`… Mission_Spread_Item schema                  covers`) that reached every
   user of that refusal. Rewritten as a proper `\` continuation. No test pinned
   the text (the error test only pins `Mission_Spread_Item`), which is why it
   shipped; the message now reads as written.
2. The finding and `DiscoveredPage`'s doc both record *"25 contiguous runs
   `0..=24`"* as a **measured** property of the retail member, but nothing
   pinned it: discovery groups by the key's first component alone, so a page
   scattered through the member would satisfy every existing assertion. The
   retail test now reads every item's line number, walks the pages in member
   order and asserts the run sequence is exactly `0..=24` with no page resuming
   after another.

**Sensitivity re-checked by the reviewer** (mutation applied, run, reverted):

| mutation | result |
| --- | --- |
| the artwork join returns an empty list | synthetic test fails on `[]` vs `ASSETS/GRAPHICS/SCRAPBOOK/ART_A.PNG`; retail test fails on `294` (`left: 0`) |
| `audit` stops counting a `Known` rule as unbacked | audit test fails on `unlock_unbacked` (`0` vs `3`) |
| the `grouped.is_empty()` refusal removed | the error test fails: the member now reads as an empty `DiscoveredScrapbook { records: 0, pages: [], gaps: {} }` instead of refusing |

**Checks run by the reviewer** (after rebasing onto `origin/main`, which had
moved by six commits; the rebase applied without conflicts and neither a
`Cargo.toml`/`Cargo.lock` nor any file this branch changes was touched, but the
branch also gained commits of its own, so the *full* four checks were run rather
than the lighter re-push set):

| command | exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f47_d_ --include-ignored` (recorded as `private/evidence/F47-D/cargo-test.log`) | 0 — **5 discovered, 5 passed** (3 synthetic, 1 retail, 1 retail+GPU on the Apple M3 Pro/Metal adapter) |
| the evidence harness on the rebased tree | 0 |
| `python3 tools/validate_evidence.py private/evidence/F47-D/acceptance.json --artifact-root private/evidence/F47-D --require-pass` | 0 (28 artifacts) |
| `python3 -m unittest discover -s tools/tests -p 'test_evidence_review_identity.py' -v` | 0 |

**Evidence regenerated, not reused.** The report the implementer committed
carried `candidate_tree` `db6e9620…`, the tree of a pre-rebase HEAD that no
longer exists as a commit on this branch — the rebase moved it, exactly the case
`CLI-EVIDENCE.md` describes ("old reports cannot be reused for new code"). The
reviewer re-ran the acceptance selection with `CS_EVIDENCE_DIR` set, re-ran the
harness, and re-validated: the committed report now carries
`candidate_tree` `9e156681…` (the tree of `6d25378c`, the branch head the run
tested), created `2026-10-08T12:42:00Z`, `tests`
`5/5/5/0/0`, the same 28 artifact paths with new digests (the captures were
re-drawn and the log re-recorded), `unknowns` `[]`, `claim` `implemented`. The
only deltas after that run are this review section and the report's own copy
under `docs/findings/evidence/` — neither is read by the acceptance suite.

**Kept as it stands** (checked, deliberately not changed): `audit` counts a
declared replay link against the real mission rows but has no `replay_unbacked`
counter, while a declared memento does fail `is_complete()`. The asymmetry is
real and documented on both sides (`REPLAY_BACKING` / `MEMENTO_BACKING`, and
`ORIGINAL_REPLAY_FIELDS` is 0 while `ORIGINAL_MEMENTO_RECORDS` counts *table*
records only): the original's replay control lives in a script, not in the
table, so there is no table record to demand a match against. AC04 asks that a
replay link be audited **against original progression**, which the mission-row
check does.

**Not fixed here, filed instead:** `DiscoveredScrapbook::discover` re-implements
the container-find / mount / read / group-with-duplicate-rules path that
`catalog::baseline::scrapbook_rows` (F14-D.8) already owns, because
`crates/cs_content/src/catalog/baseline.rs` is outside this task's owner paths.
The two readers are cross-checked on retail data (equal 461-key sets and equal
counts), so a divergence fails the retail test rather than being believed, but
the duplication itself is follow-up work.
