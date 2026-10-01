# M02-A: the second source-derived mission binding

Date: 2026-10-01. Task: M02-A "Bind original mission data and branches"
(#261, `missions/M02.md`, work order `M02-A`). Shared contract:
`docs/contracts/SCRIPT-MISSION.md`; identity rules
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: `retail`
(`$CS_GAME_DIR` read-only, never written), `synthetic`. Implementer:
**bunny-alpha-2** (session of 04:31Z). Reviewer: **bunny-alpha-2** (Rally
review claim on the same task, 05:25Z), which is the *same agent identity*, so
this is **not independent review** — the reviewer found two inaccurate rows in
the implementer's own mutation table, which is exactly the kind of thing a same
identity reviewing its own work is bad at. The reviewer's context was fresh (a
new session with no memory of the implementation; everything it checked was
re-read from the tree, the installation and the task history), but a fresh
context does not make it an independent reviewer. No agent review replaces the
owner's human approval. `evidence.rs`'s M02-A report method still records that
no reviewer has run yet, because that string is written by the acceptance
harness at the moment the implementer generated the report; the reviewer
identity belongs here and in the Rally `complete_review` notes.

Reviewer's changes, all inside owner paths:

- `campaign_position_for` now refuses through `JoinAgreement::establishes()`
  rather than re-reading `agreement.state` itself. The guard a binding obeys and
  the predicate callers read were the same rule written twice; they are now one.
- `accept_m02_a_the_join_is_corroborated_by_the_long_name_rows` asserts that
  every listed row block is exactly as long as the campaign. Without it the
  corroboration test passed even with the campaign-length rule deleted (see the
  mutation table).
- This note's mutation table, corrected against re-observed runs, plus the
  judgement on the unreachable contradiction arm below.

The stage's minimum acceptance scenario is *"Source-derived binding has no
unresolved critical dependencies."* It holds. This note records what was read,
how the binding was derived, **what is new at this mission** beyond M01-A, and
which parts of the chain remain inference rather than observation.

## Reviewer's judgement on the unreachable contradiction arm

The implementer asked whether the contradiction arm — the one no installation
produces, so it is proved only on authored values — is an acceptable limit.
It is, for this merge, for three reasons and with one caveat:

1. The retail installation *agrees with itself*, so `classify_join` returns
   `Agreed` on real data and the arm is not the difference between passing and
   failing here.
2. The rule is a pure function, `campaign_position_for`, over a confirmed row
   and a `JoinAgreement`. Every arm of it is therefore reachable without an
   installation, and all of them are asserted. That is what makes the arm a real
   guard rather than dead code: deleting it fails a real test.
3. `SourceContext` is only constructible by reading a real installation root,
   so there is no supported seam through which an *authored* disagreement could
   be fed to `bind`. The pure split is the right way to test it; adding a seam
   just to make the arm reachable from `bind` would add production surface for
   no new evidence.

The caveat: the guard has therefore never been observed end to end, so a future
installation whose localized table contradicts its directory layout would be
refused *wholesale* — no mission, world or program identity at all, with the
contradiction named as the reason. That is the intended conservative behaviour,
but it is unverified on real data and is recorded as an unknown below. It also
means the guard is **all-or-nothing**: one disagreeing block outvotes every
agreeing one. That is deliberate (a single structure that contradicts the layout
is evidence *against* the join, not for it) and is asserted in the synthetic
test, but an installation that disagrees for a benign reason — say one campaign
length run of unrelated strings that happens to carry `" - "` on every row —
would lose every binding. A future stage that reads a second or third
installation should watch for that.

## Files

- `crates/cs_content/src/campaign_bindings.rs` (owner path) adds:
  `TitleBlock` (`new`, `first_id`, `last_id`, `len`, `is_empty`, `contains`),
  the free function `title_blocks`, `GroupedTitleBlock`, `JoinCorroboration`,
  `JoinAgreement` (`establishes`), the free functions `classify_join` and
  `campaign_position_for` with the refusal constants
  `NO_CONFIRMED_ROW_REFUSAL`, `CONTRADICTED_JOIN_REFUSAL` and
  `SHORT_ROW_BLOCK_REFUSAL`, the free helper `region_prefix`, and the
  `SourceContext` methods `chapter_sizes`, `campaign_title_blocks`,
  `present_string_ids`, `join_agreement`, `region_group_sizes`.
  `campaign_position` now reads its answer from `campaign_position_for`
  instead of walking the row runs itself, so M01-A's join and M02-A's guard
  cannot disagree.
- `missions/bindings/M02.json` (owner path): the generated binding record.
- `missions/bindings/README.md` (owner path): what the directory holds now.
- `crates/cs_app/tests/campaign/m02_a.rs` (owner path): the eight
  `accept_m02_a_*` tests. `crates/cs_app/tests/campaign/evidence.rs` (owner
  path): the `evidence_report_m02_a_writes_the_acceptance_report` harness plus
  the M02-A test list. It is deliberately not prefixed `accept_m02_a_`.
- Wiring only (AGENTS rule 1): `crates/cs_app/tests/campaign/main.rs` (one
  `mod m02_a;` and a doc paragraph).
- No `Cargo.toml` or `Cargo.lock` change: this stage adds no dependency and no
  crate edge.

## What was read from the installation

`$CS_GAME_DIR` was opened read-only. Nothing original is committed. Measured
on this installation, all of it re-derived by the tests:

| Fact | Value | Where it comes from |
| --- | --- | --- |
| `install_sha256` | `b4e780ab…c631978` | `cs_assets::install::discover` + `fingerprint`, re-measured independently by the test |
| Campaign layout | `ZBD/C1`, `C1B`, `C1C`, `C2`, `C2B`, `C3`, `C4`, `C5` | directory walk |
| Campaign size | 24 missions (5/5/5/5/4 per chapter) | `SourceContext::campaign()`; `chapter_sizes()` = `[5,5,5,5,4]` |
| Localized title | exactly one row equals `The Bomber Heist` behind the display tag `[AB14I]` | `GOSDATA/ASSETS/BINARIES/langui.dll` read through `cs_content::config::StringCatalog` |
| Title row block | one of two campaign-length runs, both 24 rows | `campaign_title_blocks()` |
| Mission id | `mission/ch1-m02` | campaign position 1 → chapter 1, mission 2 |
| World group | `world/c1` | `ZBD/C1/M02/` |
| Program | `script/c1-m02-zrdr` | `ZBD/C1/M02/zrdr.zbd` (117 252 bytes, SHA-256 `89f27219…5423ad9`) |
| World-group siblings | `ZBD/C1/M02`, `ZBD/C1/M04`, `ZBD/C1/M05` | `SourceContext::campaign()` filtered by `world_group` |

`M02` is at campaign position 1, not 0 — the first mission M01-A bound could
not distinguish a correct join from a constant, because M01 *is* the first
row of the block *and* the first mission of the campaign. Every constant-answer
mutation below is caught here for exactly that reason.

### Reviewer's independent re-measurement

The reviewer re-derived the load-bearing numbers **without** the binding path,
so the acceptance tests are not the only thing standing behind them:

- `shasum -a 256` over `ZBD/C1/M02/zrdr.zbd` (117 252 bytes) and
  `GOSDATA/ASSETS/BINARIES/langui.dll` gives the two digests `M02.json` records,
  byte for byte.
- A walk of `ZBD/<chapter><variant>/<mission>/zrdr.zbd` gives 24 missions per
  chapter `5, 5, 5, 5, 4` — so `chapter_sizes()` is the directory layout's own
  accounting. Chapter 1 is `C1C/M01`, `C1/M02`, `C1B/M03`, `C1/M04`, `C1/M05`
  once ordered by mission number, which is why position 1 is `mission/ch1-m02`
  with world group `C1` and program `ZBD/C1/M02/zrdr.zbd`.
- `cs-inspect config --file …langui.dll --string <id>:1033` over the whole table
  gives the two campaign-length runs directly: rows `3450…3473` are the
  region-prefixed long names and `3480…3503` the short names, both 24 rows and
  no others that long. The short run's first two rows are `The Lost Treasure`
  and `The Bomber Heist`, so the declared M02 title is row `3481`, index 1 —
  the position the record claims.
- The same dump puts `The Bomber Heist` and
  `Hawaii - The Great British Bomber Heist` each exactly once in the file, and
  shows the long run's prefixes grouping `Hawaii`×5, `Northwest`×5,
  `Hollywood`×5, `Rocky Mountains`×5, `Manhattan`×4 — `[5,5,5,5,4]`, the
  layout's chapter sizes. That is the corroboration, measured without
  `join_agreement`.
- Comparing `missions/bindings/campaign-inventory.tsv` with the short run row by
  row confirms the eight mismatches below are exactly M05, M09, M11, M14, M15,
  M20, M22 and M23.

## What is new at M02

M01-A's join rests on **one** structure: a run of exactly `campaign.len()`
localized rows, whose length must equal the campaign the directory layout
declares. That is weak. Any 24 unrelated strings would form such a run, and
the length agreement is a coincidence the join cannot distinguish from a real
campaign list. M01-A's finding recorded this as supporting observation only.

M02-A turns it into a check the code performs:

1. **The installation offers a second account of the campaign.** The
   localized table holds **two** campaign-length runs of 24 rows: the short
   mission names and the region-prefixed long names
   (`Hawaii - The Great British Bomber Heist`, `Manhattan - Battle over
   Broadway`, …). `campaign_title_blocks()` returns *both*, not only the one
   the confirmed title sits in.
2. **The long names group like the layout's chapters.** Taking the part of
   each long name before its first `" - "` gives five regions in groups of
   **5, 5, 5, 5, 4** — which is exactly `chapter_sizes()`, the per-chapter
   mission counts the directory walk declares. Only the *grouping* is used.
   **No region name is bound to a chapter**: nothing in the installation
   establishes what "Hawaii" means, and `region_group_sizes` only counts
   consecutive rows that share a prefix. The M01-A finding's standing unknown
   ("the chapter ↔ region naming is not established from a definition file")
   is unchanged.
3. **A contradiction now yields no campaign position at all.**
   `campaign_position_for` refuses when the grouping disagrees with the
   layout, and names the cause. Before M02-A, `bind` would have derived a
   mission identity anyway and the record would have read as resolved.

M02's own title sits in the **ungrouped** block, so the corroboration
genuinely comes from elsewhere — asserted by
`accept_m02_a_the_join_is_corroborated_by_the_long_name_rows`, which fails if
M02's own block ever carries prefixes and corroborates itself.

**The join is still an inference.** It is recorded as
`ClaimStatus::Inferred` for the mission and world dependencies and
`ObservedTool` for the installation hash, the localized title and the program
archive. The corroboration narrows what a disagreement would look like; it does
not turn the mapping into an observation, and nothing here is
`verified_original` (AGENTS rule 8).

## What M02-A found that a reader of the record would otherwise miss

- **The world group is not a mission identity.** `ZBD/C1` holds three retail
  missions (M02, M04, M05), so `world/c1` alone cannot identify this mission.
  M01-A could not see this: `ZBD/C1C` holds only M01. A consumer keying
  mission content by the world row would conflate three missions. The mission
  id and the program id do single it out. Follow-up filed as #449 (`M02-T2`).
- **The original spellings disagree.** The short block says
  `The Bomber Heist`; the long block says
  `Hawaii - The Great British Bomber Heist`. Neither is "the" name; each is
  one localized row. `accept_m02_a_the_original_name_is_confirmed_against_the_local_strings`
  pins that the confirmation is a fact about *one* row.
- **Several work-order titles are not the retail strings.** Comparing the
  declared inventory with the localized block, eight of twenty-four declared
  titles differ from the original string: M05, M09, M11, M14, M15, M20, M22,
  M23. M02 is not one of them, which is *why* this stage's binding resolves at
  all. The eight are recorded as unknowns below and filed as #448
  (`M02-T1`); they are **not** corrected here, because the inventory is the
  frozen denominator whose titles come from `missions/README.md`
  (`missions/bindings/README.md`) and the retail spelling is not this task's to
  substitute. Their bindings will leave `CriticalDependency::TitleString`
  unresolved until the owner decides.
- **The retail table carries duplicated display texts** (66 of them, e.g.
  `OK`, `Cancel`, `Manhattan`). A title matching several rows names no single
  row and therefore no single campaign position; `bind` refuses it, and the
  refusal is tested with a duplicated retail string rather than an authored
  one.

## What "critical" means here, and why

Unchanged from M01-A and not re-litigated: `CriticalDependency::ALL` is the
data-binding checklist's first five entries of `missions/M02.md` — *canonical
mission id; installation/rules hash; title string; program and source map;
world group/variant*. They are the anchors the rest of the checklist is read
through. `accept_m02_a_…` asserts all five resolve **and** that the entries
this stage does not bind are still named in `unknowns`, so the definition
cannot be narrowed to hide work.

Two states stay apart, as the 2026-09-28 owner directive requires:

- **Source-derived** — `unresolved_critical().is_empty()`. **Holds** for M02.
- **Verified** — `is_verified()`. **False**: `closure_sha256` needs the retail
  content catalog (F14-D) and decoded mission programs (F37/F38), and every
  checklist entry below is unbound.

## What the record deliberately does not claim

- `verified` is `false`, `closure_sha256` is `null`, `evidence_ids` is empty.
  The evidence *report*'s `unknowns` is empty because
  `tools/validate_evidence.py --require-pass` rejects a report with unresolved
  task issues — the product-incompleteness state lives in
  `missions/bindings/M02.json` and here, not in the report.
- Only the `mission_identity` cell of M02's campaign record is `Complete`. The
  other six are `Unknown`, all 23 subsystem rows per mission are
  `Unresolved`, all 24 progressions are `Unknown`, and
  `CoverageReport::is_ready()` is false. The test asserts every count.
- No objective, actor, spawn, route, reward, difficulty or failure/success
  value is bound or guessed. No opcode, timing, count or coordinate was read
  from a walkthrough.
- Which reader member is M02's control program is **not** established: the
  program identity names the mission's reader archive as a whole, and the
  mission opcode table remains unmeasured (F13-C ships an empty signature
  table).
- Nothing here is a behaviour claim. No original run was observed.

## Test inventory (`accept_m02_a_*`, 8 tests)

| Test | What it pins |
| --- | --- |
| `accept_m02_a_source_derived_binding_has_no_unresolved_critical_dependencies` (retail) | the scenario: all five dependencies resolved with the right evidence class, the installation hash re-measured through production discovery, the ids carry the kinds their roles mean, the selected directory and reader archive exist, the record is not M01's, every span re-hashed and in range, and `verified` false with the unbound checklist entries still named |
| `accept_m02_a_the_committed_record_is_what_the_installation_derives` (retail) | `missions/bindings/M02.json` is byte-identical to what production code derives, and carries the schema's fields |
| `accept_m02_a_the_original_name_is_confirmed_against_the_local_strings` (retail) | the declared title is carried by exactly one retail row in a campaign-length block; the second block's row at the same index says something **else** (region-prefixed), so the two blocks are independent; and a title the strings do not carry — and a title they carry **twice** — resolve neither the title nor the three identities that depend on it, while the installation hash stays resolved and no span is cited |
| `accept_m02_a_the_join_is_corroborated_by_the_long_name_rows` (retail) | the stage's new claim, measured: `Agreed`, at least two campaign-length blocks, **every listed block exactly as long as the campaign** (added by the reviewer), at least one grouped, the grouping equal to the layout's chapter sizes, the groups covering every row, M02's own block **not** the grouped one, and every row of the corroborating block selecting the campaign position and mission identity its index names |
| `accept_m02_a_the_world_group_holds_several_missions` (retail) | `ZBD/C1` holds more than one mission, every sibling's archive exists, the record binds only the mission its position selects, and the program identities of the siblings are distinct — so the world row cannot be a mission identity |
| `accept_m02_a_the_campaign_keeps_everything_else_unresolved_and_unready` (retail) | 24 missions, 168 cells, exactly 1 complete, 552 subsystem rows all unresolved, 24 unknown progressions, `is_ready()` false, and M02's closure counts 7 cells / 1 complete / 23 unresolved subsystems |
| `accept_m02_a_a_title_block_must_be_exactly_the_campaign_length` (synthetic) | `title_blocks` on authored id sets: a gap ends a run, a single row is a run of one, a run one short or one long is not the campaign, two campaign-length runs are both listed ascending, and `TitleBlock::new(5, 4)` is `None` |
| `accept_m02_a_a_contradicted_corroboration_establishes_no_position` (synthetic) | every arm of the guard on authored values: `Unavailable` / `Agreed` establish, `Disagreed` refuses, one contradicting block outvotes one agreeing, a same-total regrouping is still a contradiction, and `campaign_position_for` returns the index for a confirmed row / each of its three named refusals otherwise |

Every test calls production code (`SourceContext`, `SourceBinding`,
`CampaignBindings`, `title_blocks`, `classify_join`,
`campaign_position_for`). No test repeats an expected value read from the
record it checks: the retail assertions are re-measured from
`$CS_GAME_DIR`, from the string table the installation holds, or from the
committed inventory.

## Mutation probes

The implementer applied seven mutations, then reverted them. The reviewing
agent re-applied all seven independently and observed the results below; two
of the implementer's rows were corrected by that re-run (marked **corrected**),
because the mutation table is a claim about test strength and has to name the
test that actually fails. Every mutation was reverted again; the tree carries
none of them.

| Mutation | Observed result (re-run by the reviewer) |
| --- | --- |
| `campaign_position_for` answers position 0 for every row that sits in a block | **corrected: 5 of 8 fail**, not 4: `…_the_committed_record_is_what_the_installation_derives`, `…_the_join_is_corroborated_by_the_long_name_rows`, `…_source_derived_binding_has_no_unresolved_critical_dependencies`, `…_the_world_group_holds_several_missions` and `…_a_contradicted_corroboration_establishes_no_position` |
| `campaign_title_blocks` returns every run, dropping the campaign-length rule | **corrected: 1 fails — `…_the_original_name_is_confirmed_against_the_local_strings`, not `…_the_join_is_corroborated_by_the_long_name_rows`**. With the rule gone, 66 rows of unrelated runs land at the same index, so that test's "exactly one second row at campaign position 1" assertion breaks. The corroboration test did **not** catch it: no other run in the retail table carries a region prefix on every row, so `grouped` is unchanged and every corroboration assertion still holds. The reviewer therefore added an assertion that every listed block is exactly as long as the campaign, so a repeat of this mutation now fails the corroboration test as well |
| `region_group_sizes` treats a missing prefix as a group of its own instead of reporting the block ungrouped | 6 of 8 fail |
| `chapter_sizes` returns a constant | 6 of 8 fail |
| `classify_join` uses `any()` instead of `all()` | 1 fails: `…_a_contradicted_corroboration_establishes_no_position` (one agreeing block would outvote a contradicting one) |
| the contradiction arm of `campaign_position_for` removed | 1 fails: the same synthetic test |
| `title_blocks` merges runs across a gap | 7 of 8 fail |

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m02_a_ --include-ignored` | 0 (8 tests: 6 retail, 2 synthetic) |
| `cargo test --locked -p cs_app --test campaign -- accept_m01_a_ --include-ignored` | 0 (8 tests) — M01-A's suite is unaffected by the `campaign_position` change |
| `python3 tools/validate_evidence.py private/evidence/M02-A/acceptance.json --artifact-root private/evidence/M02-A --require-pass` | 0 (`structurally_valid: true`) |

The reviewer re-ran every one of these on the rebased tree, added
`cargo doc --locked -p cs_content --no-deps` (0; the change adds no new rustdoc
warning — the two `campaign_bindings.rs` warnings are pre-existing F14-E links
to the private `scan_campaign`), and ran each of the eight `accept_m02_a_*`
tests alone with `--exact` as `docs/contracts/CLI-EVIDENCE.md` requires. Because
the reviewer's fix touches production code, the evidence report was regenerated
on the corrected commit per the same contract, so its `candidate_tree` is that
commit's tree.

## Recorded unknowns (not guessed)

- **The join remains an inference**, now with a second structure checking its
  *shape*. Nothing establishes the mapping itself; a campaign-definition record
  was not found and no original run was observed.
- **What a region name means is not established.** Only the grouping is used.
  Which region corresponds to which chapter, and whether the five regions are
  the chapters at all, stays open.
- **`" - "` is a display convention, not a format.** `region_prefix` splits on
  the first `" - "` of a row's display text, which the long-name rows happen to
  use (`Hawaii - The Lost Treasure of Sir Francis Drake`). Nothing declares
  that every row carrying one names a region, and a short mission name that
  contains `" - "` would make its block *grouped*. On this installation it does
  not: M02's own block is asserted ungrouped, and the only fully prefixed block
  is the long-name one. But a mission whose short name contains `" - "` and
  groups into anything other than the chapter sizes would flip
  `classify_join` to `Disagreed` and refuse every binding at once, because the
  guard is all-or-nothing. If M05 … M24-A ever bind a row like that, this is
  the first thing to look at.
- **Eight of twenty-four declared work-order titles are not the original
  localized strings** (M05, M09, M11, M14, M15, M20, M22, M23). Their
  bindings will leave `CriticalDependency::TitleString` unresolved until the
  discrepancy is resolved by the owner. Recorded and filed as #448
  (`M02-T1`); **not** silently corrected.
- **String-id numbering is not established** (`cs_formats::string_id` numbers
  rows `(block - 1) * 16 + index`, one block below the documented Win32 rule
  for the same `RT_STRING` leaf). Filed as #374 by M01-A. M02-A joins only on
  contiguity and index within a run, so it is unaffected either way.
- **Which reader member is M02's control program, and every opcode in it, is
  unmeasured** (F13 ships an empty signature table).
- **The contradiction arm of the guard is not exercised by any installation.**
  This installation agrees with itself, so that arm is proved on authored
  values. An installation whose localized table contradicted its directory
  layout would be refused wholesale — which is the intended behaviour, but it
  has not been observed on real data.
- **No original run was observed.** Nothing here is a behaviour claim.

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`cs_content::config::StringCatalog` and the platform filesystem;
`schemas/mission-binding.schema.json`; `missions/M02.md`;
`docs/contracts/SCRIPT-MISSION.md`; `docs/contracts/IDENTITY-CONTENT.md`;
`docs/findings/2026-09-29-m01-a-source-binding.md`;
`docs/findings/2026-09-29-f50-a-binding-coverage-records.md`;
`docs/findings/2026-09-29-f13-b-locate-and-classify-programs.md`;
`docs/findings/2026-09-29-f12-c-typed-tuning-and-localized-ids.md`.
