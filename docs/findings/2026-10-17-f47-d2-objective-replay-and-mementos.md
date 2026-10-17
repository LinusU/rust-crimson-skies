# F47-D.2: what the original scrapbook's `Objective` position, replay control and mementos are

Date: 2026-10-17. Task: #774 / F47-D.2 "Measure what the original scrapbook's
Objective field and replay control do", a follow-up of #201 / F47-D
(`specs/F47-scrapbook-records-mementos-and-mission-replay.md`, section
`### F47-D`). Shared contract:
[`STATE-TRANSACTIONS`](../contracts/STATE-TRANSACTIONS.md); evidence contract
[`CLI-EVIDENCE`](../contracts/CLI-EVIDENCE.md). Required capability: **`retail`**
(read-only `$CS_GAME_DIR`), used. The original executable was **not run**, so
nothing here is `verified_original`.

Implementer: `bunny-alpha-2` (agent `bunny-alpha-2`). This file is the
implementer's record; it is not a review and awards no
`verified_original`/`release_approved`.

## One observable failure (listed before editing)

F47-D published a discovery and an audit that carried the `Objective` position
as an unconverted number (`DiscoveredItem::objective: Option<i64>`) and said so:
"*its meaning is unmeasured … the audit records its value distribution and
refuses to read it as a rule*". With this stage's production code removed:

* `cs_content::scrapbook::DiscoveredScrapbook::objective_rules` does not exist,
  so nothing can say what the table's `Objective` position indexes, which items
  it binds to a campaign mission and which it leaves unattributed;
* `AuditReport` carries no objective census and no `OBJECTIVE_BACKING`, so an
  AC04 re-run cannot report the one scrapbook field that could carry
  progression;
* and no test can pin the measured relation (447 numbered positions, 14 special
  ones, 165 of 167 `Snap_<page>_<n>` names agreeing with their position).

`accept_f47_d2_the_objective_position_binds_a_snapshot_to_its_missions_objective`
fails first: it calls `DiscoveredScrapbook::objective_rules` on an authored
temporary installation and asserts the binding, the explicit unknowns, the
snapshot census and the provenance.

## What was measured

Everything below is read from the owner's installation, read-only. No original
text is reproduced; identifiers, counts, offsets and digests only. The three
questions are answered in order.

### 1. The `Objective` position indexes a mission's numbered objective

The table `ASSETS/SCRAPBOOK.CSV` (member of `GOSDATA/ASSETS/crimson.rof`; the
**decoded** member's SHA-256 is
`28b5144c54120f52c36717a3f1e094cb75845ecb1f854334a5686d5f6c6af5c1`, the digest
F12-D/F12-I/F14-D.8/F47-D recorded) holds 461 `Mission_Spread_Item` records.
`Objective` is the first of the sixteen documented positions (F12-I) and is a
whole number in every record.

| measurement | value |
| --- | --- |
| records | 461 |
| distinct positions | 26 |
| position range | `-12 ..= 31` |
| `Objective <= 0` (names no numbered objective) | **14** (`0` ×10, `-11` ×3, `-12` ×1) |
| `Objective >= 1` (a numbered objective) | **447** |
| pages the positive positions name | every page `1 ..= 24` |

**The decisive evidence is the member's own picture names.** 167 of the 461
records name a runtime snapshot, `Snap_<a>_<b>`. For **165** of those 167 the
position is exactly `<b>` and the key's page is `<a>`. The two that disagree are
`22_1_6` (`Snap_22_18`, position `1`) and `22_1_7` (`Snap_22_19`, position `1`) —
both on page 22, the page that also carries a commented-out block of
`Snap_18_*` placeholders, so they read as leftover template rows rather than a
different rule. The engine itself names snapshots this way: the decrypted image
(`crimson.decrypted.exe`, SHA-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`) carries the
format string `%1!s!\Snap_%2!d!_` at image offset `0x21f0a4` (and `Snap_` at
`0x21f068`), with the `<page>_<spread>_<slot>` key format `%1!d!_%2!d!_%3!d!` at
`0x21ef18`. Numbers only: nothing is claimed about the code that calls them.

**The page is a campaign mission.** The member's own section comments name five
five-mission chapters — page 1, 6, 11, 16 and 21 begin `Hawaii`, `Northwest`,
`Hollywood`, `Skyhaven` and `Manhattan`, matching the campaign layout's five
chapters of 5/5/5/5/4 missions (`cs-inspect campaign`; `mission/ch1-m01` …
`mission/ch5-m04` in campaign order). Page `p` (`1 ..= 24`) is therefore the
`p`-th campaign mission in order; page 0 is the initial page.

**The numbers exist in the missions.** Reading the mission-scoped reader
archives: `ZBD/C1C/M01/zrdr.zbd`'s `objectives.zrd` declares `OBJECTIVE1` …
`OBJECTIVE58` (58 contiguous blocks) and `ZBD/C5/M02/zrdr.zbd` (mission 22)
declares `OBJECTIVE1` … `OBJECTIVE65`. Every positive position the table uses
(`1 ..= 31`) is one of those blocks, so no item names an objective its mission
does not declare. The objective reading is by identifier only; no mission text
is reproduced.

**What stays unknown.** Whether the *engine* gates an item's visibility on the
objective it names — i.e. whether `Objective` is an unlock rule — is native
behavior: the scrapbook screen scripts build their spread through native
callbacks (see below) and no script reads the table's `Objective` position. So
the binding records *what the position names*, at `ClaimStatus::ObservedTool`,
and `UNLOCK_BACKING` still reports a declared `Known` unlock rule as
not-original-backed. The 14 special positions (`0` and the negatives) name no
numbered objective; the four negative rows have an empty `ResourceID` and their
meaning is unmeasured.

### 2. The table-of-contents replay control launches a native-selected mission

`ASSETS/SCRIPTS/SCRAPBOOK_TOC.SCRIPT` (member of the same archive; decoded
4 316 bytes, decoded SHA-256
`f26f48e2cdfd84db8012f53fd4e070e6d9d6962c885b591b70772bd5f1d45d01`) declares
the button `sbtoc_b_replay`. Its handler:

* reads the selected table-of-contents list entry index (`UT`) and, at press,
  calls native command **2104** with that index (`callback($$E$$, 2104, (UT))`),
  then ends the script and fires the application's own follow-ups;
* fills the list through native command **2409** (one call per row), reads the
  "current" mission index through **2405**, and enables the replay button only
  when **2411** answers that the entry is replayable.

So the **mechanism** is measured end to end: the control hands a
table-of-contents **list index** to native command 2104. The **target** of a
replay is native — the script names no mission or variant, and the table
documents no mission field ([`ORIGINAL_REPLAY_FIELDS`]) — so it stays an
explicit unknown. This is F38's boundary (native behaviour bindings), not
something the table or the script can answer.

The constants `TOC_REPLAY_COMMAND` (2104), `TOC_LIST_COMMAND` (2409),
`TOC_CURRENT_COMMAND` (2405) and `TOC_REPLAYABLE_COMMAND` (2411) record the
measured command numbers; `REPLAY_CONTROL_CLAIM` is their claim id.

### 3. The memento list, its order and its unlock state are native

`ASSETS/SCRIPTS/MOMENTOSELECTION.SCRIPT` (member of the same archive; decoded
1 453 bytes, decoded SHA-256
`39fca3e788162c1c69effe76beb12861997e4d402da1bdb15004492ba49155a0`) gets and
sets its picture through native command **2150** alone:

* `callback($$E$$, 2150, 0, <index>, <name>)` returns the picture **name** at
  an index — index `0` for the entry the cabin shows, `100` for the next,
  `101` for the previous;
* `callback($$E$$, 2150, 1, <name>)` selects the memento the name identifies;
* the script then builds the artwork path `assets\graphics\scrapbook\<name>` and
  special-cases one name (`ms_p_initialpinup1.jpg`), which is not offered as a
  choice.

The **set**, its **order** and its **unlock state** live entirely in the native
state command 2150 reads and writes; the script carries none of them, and the
table holds no memento record (`ORIGINAL_MEMENTO_RECORDS` is 0 — every
schema-covered record is a `Mission_Spread_Item`). They therefore stay explicit
unknowns. Only the command number is measured, recorded as `MEMENTO_COMMAND`
(2150) with `MEMENTO_SOURCE_CLAIM`. The 15 `MS_P_*`-shaped table images remain a
**prefix count**, not a memento ranking.

## What this stage binds (and does not)

Owner paths only (`crates/cs_content/src/scrapbook.rs`, `tests/`,
`docs/findings/`); no `cs_app` or `cs_app::ui::scrapbook` change was needed, so
no wiring edit to list.

* `crates/cs_content/src/scrapbook.rs`:
  * `ScrapbookObjectiveRule` (`mission` + `objective`) and
    `DiscoveredScrapbook::objective_rules(&progression) -> ObjectiveCensus` —
    the binding. Every item's position is read as a numbered objective of the
    campaign mission its page names, mapped through the progression's mission
    rows **in canonical id order** (the installation's campaign order). A
    position of `0`, a negative position, a position the schema-whole text can
    hold but no objective can (an out-of-range integer) and a page the
    progression holds no mission for all become an explicit
    `Resolved::Unknown`, never a guessed rule.
  * `ObjectiveRule` / `ObjectiveCensus` — the per-item binding and the census
    (numbered / special / unparsed / bound counts, the pages without a mission,
    and the snapshot census).
  * `DiscoveredScrapbook::member_span` — the table member's own `SourceSpan`,
    new provenance for every binding (`OBJECTIVE_RULE_CLAIM` at
    `ClaimStatus::ObservedTool`, over the member's decoded digest).
  * `OBJECTIVE_RULE_CLAIM`, `REPLAY_CONTROL_CLAIM`, `MEMENTO_SOURCE_CLAIM` and
    the measured command constants `TOC_REPLAY_COMMAND`, `TOC_LIST_COMMAND`,
    `TOC_CURRENT_COMMAND`, `TOC_REPLAYABLE_COMMAND`, `MEMENTO_COMMAND`.
  * `AuditReport::objective` (the census) and `OBJECTIVE_BACKING` in the audit
    JSON; `is_complete()` now also refuses while a numbered page names no
    mission. `UNLOCK_BACKING` was corrected: the position's meaning is measured
    but the gating stays native.
* `crates/cs_content/tests/accept_f47_d2_scrapbook_objective.rs` (new): two
  synthetic tests and one `#[ignore = "requires CS_GAME_DIR"]` retail test.

**Not bound.** The replay target and the memento set/order/unlock stay explicit
unknowns: they are native (F38's boundary), and the table/script carry no
mission identity or memento state to bind.

## Test selection and sensitivity

`cargo test --workspace --locked -- accept_f47_d2_ --include-ignored` discovers
and runs **3** tests, all passing:

| test | what it fails without |
| --- | --- |
| `accept_f47_d2_the_objective_position_binds_a_snapshot_to_its_missions_objective` | the page→mission mapping, the numbered/special split, the snapshot agreement, the explicit unknowns, the provenance span, or the audit's objective section |
| `accept_f47_d2_a_position_with_no_objective_and_a_page_with_no_mission_are_unknowns` | the out-of-range position or the page-without-a-mission unknown, or their effect on `is_complete()` |
| `accept_f47_d2_retail_the_installation_objective_census_is_measured` (`#[ignore = "requires CS_GAME_DIR"]`) | the measured retail census: 461 items, 447 numbered, 14 special, 0 unparsed, 447 bound, 167 snapshot rows, 165 agreeing, 2 disagreeing, and the first Hawaii snapshot binding to `ch1-m01` objective 18 |

Sensitivity was checked by mutation, then reverted (each run:
`cargo test -p cs_content --test accept_f47_d2_scrapbook_objective`):

| mutation | result |
| --- | --- |
| the page→mission index off by one (`page.page` instead of `page.page - 1`) | the synthetic binding test fails on `bound` (`1` vs `3`) and the unknown test on `bound` (`0` vs `1`) |
| the snapshot agreement always true | the binding test fails on `snapshot_agree` (`3` vs `2`) |
| `snapshot_name_components` returns `None` | the binding test fails on `snapshot_items` (`0` vs `3`) |

The retail test is the same production code over the real table, so a change to
the mapping or the census fails there too.

## Design decisions a reviewer should check

1. **The binding is a measurement, not an invented unlock.** `ScrapbookObjectiveRule`
   says "this position names objective `n` of mission `m`". It is **not** wired
   into `Unlock`/`UnlockFact`, whose vocabulary has no mission-objective leaf:
   `UnlockFactKind` is `MissionSucceeded`/`StuntCompleted`/`AceDefeated` and
   `UnlockFact` carries a kind and a subject, so adding an objective leaf would
   change `cs_sim::records` (`FactKind`), outside this task's owner paths. It
   would also claim the native gating the engine has not been shown to do. The
   declared catalog (#773 / F47-C2) owns turning a measured binding into a
   runtime rule.
2. **The mission is the campaign order, taken from the progression the audit
   already receives.** `mission/ch1-m01` … `mission/ch5-m04` sort in campaign
   order, so page `p` is the `p`-th mission row. A page the progression cannot
   answer is a named gap (`page_without_mission`), never silently dropped, and
   now keeps `is_complete()` false.
3. **"Unparsed" is real, not dead code.** `RecordSchema::for_entry` accepts any
   whole-number first field, and `is_whole_number` accepts arbitrarily long
   digits, so a value beyond `i64::MAX` passes the schema and still names no
   objective. The branch is exercised by the second synthetic test.
4. **The snapshot census pins the evidence, not just the rule.** Counting
   `Snap_<page>_<n>` items and how many agree with their position is what makes
   the binding falsifiable: a corpus that changed the relation would fail the
   retail test rather than be believed.
5. **The commands are recorded as numbers, with a claim id each.** The replay
   and memento mechanisms are only ever stated at the level the bytes show
   (command numbers and their arguments); no handler, address or disassembly is
   committed, and no mission or memento is named.

## What remains unknown (recorded, not guessed)

* **Whether the engine gates an item on the objective it names.** The position
  is measured to name a numbered objective; whether completion unlocks or
  reveals the item is native. **Affected content:** every unlock path, hidden
  page and reward claim in F47 and F42. **Resolving tasks:** F38 (native
  behaviour bindings) and F13/F07-D (mission language).
* **What a replay launches.** The control passes a table-of-contents list index
  to native command 2104; the mission and variant are native. **Affected
  content:** every replay link's target in F47-C's launch. **Resolving task:**
  F38.
* **The memento set, order and unlock state.** Read and written by native
  command 2150; the table holds no memento record. **Affected content:** the
  cabin memento choice (F47 non-negotiable 5) and any `EntryKind::Memento`.
  **Resolving tasks:** F38 and F45's cabin screen work.
* **The meaning of the 14 special positions.** `0` is the initial page; the four
  negative positions (`-11` ×3, `-12` ×1, all with an empty `ResourceID`) name
  no numbered objective and their effect is unmeasured. **Affected content:**
  those four items' visibility (`6_1_4`, `11_2_3`, `13_2_2`, `20_1_3`).
  **Resolving task:** F38.
* **The 167 snapshot items the installation does not hold** (`Snap_*`, no
  artwork member) — F47-D's recorded gap; the binding now also identifies the
  objective each names. **Resolving task:** #773 / F47-C2.
* **No runtime consumes a `ScrapbookObjectiveRule` yet.** It is data a declared
  catalog can project; the runtime declares no scrapbook entry at all (F47-D's
  recorded gap). **Affected content:** the whole scrapbook UI's data.
  **Resolving tasks:** #773 / F47-C2 on top of #760 / F47-C1.
* **No original executable ran.** `retail` is file access; the engine's
  snapshot naming is read from the image as strings and offsets only. Nothing
  here is `verified_original`, and the `ObservedTool` class says so.

## Sources used

* `specs/F47-scrapbook-records-mementos-and-mission-replay.md` (stage
  `### F47-D`, AC04, non-negotiable 1 and 5).
* `docs/contracts/STATE-TRANSACTIONS.md`, `docs/contracts/CLI-EVIDENCE.md`.
* `docs/findings/2026-10-08-f47-d-retail-scrapbook-audit.md` (the discovery,
  the audit, the three backing constants and the unknowns this stage answers).
* `docs/findings/2026-09-29-f12-i-record-kind-schemas.md` (the sixteen
  documented positions and the `Objective` position's kind),
  `docs/findings/2026-10-03-f14-d-8-stunt-scrapbook-collections.md` (the 461
  rows and the member digest).
* `docs/findings/2026-10-15-f45-b-1-original-layout.md` (the front-end screen
  sections, including `ScrapBook_TOC`, and the script members this stage reads).
* `docs/findings/2026-10-03-f39-d-branching-optional-and-failure-validation.md`
  (the mission-scoped `objectives.zrd` reader and the numbered `OBJECTIVE<N>`
  blocks).
* `crates/cs_content::campaign_bindings` (`campaign_layout`,
  `CampaignMission`), `crates/cs_content::catalog::baseline`
  (`retail_baseline`) and the production `cs_assets::rof` mount.
* The owner's installation, read-only: `GOSDATA/ASSETS/crimson.rof` and its
  `ASSETS/SCRAPBOOK.CSV`, `ASSETS/SCRIPTS/SCRAPBOOK_TOC.SCRIPT` and
  `ASSETS/SCRIPTS/MOMENTOSELECTION.SCRIPT` members, and the mission-scoped
  `ZBD/*/M*/zrdr.zbd` `objectives.zrd` members. **No script line, record line or
  picture is committed**, and every claim taken from them is stated as a
  measurement or an explicit unknown.
* The owner-supplied decrypted engine image, read-only: the `Snap_` format
  strings at the offsets cited above (numbers only; no disassembly or
  decompiled code).
