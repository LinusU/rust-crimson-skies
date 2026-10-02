# M01-A: the first source-derived mission binding

Date: 2026-09-29. Task: M01-A "Bind original mission data and branches"
(#258, `missions/M01.md`, work order `M01-A`). Shared contract:
`docs/contracts/SCRIPT-MISSION.md`; identity rules
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: `retail`
(`$CS_GAME_DIR` read-only, never written), `synthetic`. Implementer:
**opencode-1** (session of 05:06Z). Reviewer: **opencode-1**, a separate
session with fresh context that took no part in the implementation — same
agent name, different context, so this review is *not* independent
original-reference evidence and no agent review replaces the owner's human
approval. The reviewer added the two `the_retail_title_block…` /
`…outside_the_retail_title_block…` tests, corrected the string-id numbering
below and recorded it as unknown.

The stage's minimum acceptance scenario is *"Source-derived binding has no
unresolved critical dependencies."* This note records what was read, how the
binding was derived, what "critical" is taken to mean and why, what stays
unknown, and which parts of the chain are inference rather than observation.

## Files and the one observable failure

- `crates/cs_content/src/campaign_bindings.rs` (owner path) adds:
  `CriticalDependency` (`ALL`, `label`, the private `claim`),
  `SourceDependency`, `SourceSpanRecord`, `CampaignMission`,
  `SourceBindingError`, `SourceContext` (`read`, `install_sha256`,
  `campaign`, `string_rows`, `bind`, private `campaign_position`),
  `SourceBinding` (`dependency`, `unresolved_critical`, `is_verified`,
  `validate`, `to_json`, `to_mission_binding`), the private helpers
  `mission_key`, `program_key`, `strip_font_tag`, `read_file`,
  `map_provenance`, `scan_campaign`, `find_container_dir`, `chapter_number`,
  `mission_number`, `json_string`, `array_of_objects`, `array_of_strings`,
  and the constants `SOURCE_BINDING_UNKNOWNS`, `UNBOUND_CATEGORY_REASON`,
  `UNBOUND_SUBSYSTEM_REASON`, `UNBOUND_PROGRESSION_REASON`.
- `missions/bindings/M01.json` (owner path): the generated binding record.
- `missions/bindings/README.md` (owner path): what the directory holds now.
- `crates/cs_app/tests/campaign/m01_a.rs` (owner path): the
  `accept_m01_a_*` tests — eight in all: four record/identity tests, the
  two retail-title-block join tests added in review, and two synthetic
  predicate tests.
  `crates/cs_app/tests/campaign/evidence.rs` (owner path): the evidence
  harness, deliberately not prefixed `accept_m01_a_`.
- Wiring only (AGENTS rule 1): `crates/cs_content/src/lib.rs` (one doc
  paragraph), `crates/cs_app/tests/campaign/main.rs` (two `mod`
  declarations and a doc paragraph), `crates/cs_app/Cargo.toml`
  (`cs_assets` dev-dependency), root `Cargo.lock` (the new
  `cs_app -> cs_assets` edge).

**Observable failures (measured by mutation, applied and reverted):**

- resolving `CriticalDependency::MissionId` without a `catalog_id` fails
  `accept_m01_a_a_title_the_local_strings_do_not_carry_is_unresolved`,
  through `SourceBinding::validate`'s "resolved but carries no value" rule;
- answering a constant campaign position, or a constant mission identity,
  fails `accept_m01_a_the_retail_title_block_binds_every_campaign_position`
  at campaign position 1. **Before review no test caught this**: M01 *is*
  position 0, the record carries no position, and the committed JSON carries
  only `mission/ch1-m01`, so every earlier test passed with the join
  replaced by a constant;
- removing the campaign block-length check fails
  `accept_m01_a_a_title_outside_the_retail_title_block_resolves_no_position`
  (every retail row would then select some position, so the witness that
  test needs would not exist). The stage's headline test does *not* catch
  this mutation, contrary to what this note first claimed.

That is the failure the stage exists to prevent — a binding that reads as
resolved while pointing at nothing.

## What was read from the installation

`$CS_GAME_DIR` was opened read-only. Nothing original is committed: the
record holds ids, hashes and byte ranges, and the evidence report holds the
same. Measured on this installation:

| Fact | Value | Where it comes from |
| --- | --- | --- |
| `install_sha256` | `b4e780ab…c631978` | `cs_assets::install::discover` + `fingerprint`, re-measured independently by the test |
| Campaign layout | `ZBD/C1`, `C1B`, `C1C`, `C2`, `C2B`, `C3`, `C4`, `C5`, each holding `M<nn>` directories | directory walk |
| Campaign size | 24 missions (5/5/5/5/4 per chapter) | directory walk, sorted by `(chapter, mission number)` |
| Localized title | string id **3480**, language **1033**, text `The Lost Treasure` behind the display tag `[AB14I]` | `GOSDATA/ASSETS/BINARIES/langui.dll` read through `cs_content::config::StringCatalog` |
| Localized title block | ids **3480 … 3503**, contiguous, 24 rows | the maximal consecutive run of non-empty rows containing 3480 |
| Mission id | `mission/ch1-m01` | campaign position 0 → chapter 1, mission 1 |
| World group | `world/c1c` | the only chapter-1 group holding `M01`: `ZBD/C1C/M01/` |
| Program | `script/c1c-m01-zrdr` | `ZBD/C1C/M01/zrdr.zbd`, the reader archive F13-B classifies as holding mission programs |
| Source spans | `langui.dll` block `92088..92698` (610 bytes) and `ZBD/C1C/M01/zrdr.zbd` `0..91171` | `SourceSpan` of each asset, with each asset's SHA-256 |

`langui.dll` is in the F02 inventory (228 files), so its bytes are covered
by `install_sha256`.

**String-id numbering (corrected in review).** The ids above are the ones
`cs_formats::string_id` produces, `(block - 1) * 16 + index`: a read-only
walk of the PE resource directory during review located the `RT_STRING`
leaf at file offset 92088 (610 bytes) at path
`[type 6, name 218, language 1033]`, and `(218 - 1) * 16 + 8 = 3480`. The
documented Win32 rule for that leaf is `name * 16 + index = 3496`, so the
engine's string ids sit one block (16) below the Win32 ids of the same
text — this document originally recorded 3496 while production reports
3480. The shift is uniform, the join below uses only contiguity and index
inside the run, and `missions/bindings/M01.json` carries no string id, so
M01-A is unaffected either way. Which numbering the project intends is
**not** established here: recorded as an unknown below and filed as #374,
not guessed.

**Corrected by #374 (2026-10-02).** The claim just above that the documented
Win32 rule is `name * 16 + index = 3496` is wrong. Microsoft documents the
rule in terms of the string *identifier*, and the one-based section entry
`218` carries identifier `(218 - 1) * 16 + 8 = 3480`; the production
numbering *is* the Win32 numbering. The id 3480 recorded in the table above
is unchanged. Evidence and the rejected reading:
`docs/findings/2026-10-02-t374-string-id-numbering.md`.

## How the work order was matched to a retail mission

`missions/M01.md` says *"Find the corresponding original catalog/program/
world identities. Confirm the title against local strings; do not key
runtime logic by this discovery label."* No file in the installation names
the campaign order in plain text — a full read-only scan for
`The Lost Treasure` finds it only inside `langui.dll`, encoded UTF-16. The
derivation therefore uses two independent structures and one join:

1. **Localized strings.** Exactly one row of the UI string table equals the
   declared discovery title after stripping its display tag. The row's id
   sits in a contiguous run of non-empty rows of length 24.
2. **Directory layout.** `ZBD/C<chapter><variant>/M<nn>` yields 24 missions,
   ordered by `(chapter, mission number)`; mission numbers are unique inside
   a chapter (two groups of one chapter holding the same mission number are
   refused as ambiguous, not resolved by picking one).
3. **The join.** The title block's length must equal the campaign's length;
   the title's index inside the block is the campaign position.

**The join is an inference, not an observation.** It is recorded as
`ClaimStatus::Inferred` for the mission and world dependencies and
`ObservedTool` for the installation hash, the localized title and the
program archive. Nothing here is `verified_original` (AGENTS.md rule 8), and
no agent review can make it so. A later stage that finds an authoritative
campaign-definition record should re-derive the same mapping and, if it
differs, this record must be corrected rather than defended.

Supporting observation, not used as a premise: the sound archive names the
same campaign as `c1-HA-m1 … c5-MH-m4` (5/5/5/5/4), matching both the
directory counts and the five region prefixes in the localized long-name
block.

## What "critical" means here, and why

The word appears only in the acceptance scenario; neither the sheet nor a
contract defines it. It is taken from the **data-binding checklist** in
`missions/M01.md`, whose first five entries are *canonical mission id;
installation/rules hash; title string; program and source map; world
group/variant*. Those five are exactly what
`CriticalDependency::ALL` enumerates, in checklist order, and
`accept_m01_a_the_critical_dependency_set_is_the_checklists_first_five`
pins the set so it cannot shrink silently.

They are the anchors every later checklist entry is read *through*: with no
mission id, installation hash, confirmed title, program source map or world
group there is nothing for actors, objectives, media, rewards or difficulty
branches to hang on. The remaining checklist entries are **content the
binding records, not dependencies it needs**. They are kept, with a reason
each, in `SourceBinding::unknowns` and in `missions/bindings/M01.json`, and
they keep `verified` false and the campaign unready. The tests assert both
halves: no critical dependency unresolved *and* every unbound checklist
entry still present, so the definition cannot be narrowed to hide work.

The distinction is what the 2026-09-28 owner directive asks for — a working
record may legitimately report incomplete product support as long as the
states stay separate. Two states, two fields:

- **Source-derived** — `unresolved_critical().is_empty()`. This is the M01-A
  acceptance scenario, and it holds.
- **Verified** — `is_verified()`: no critical dependency unresolved, no
  unknown left, `closure_sha256` present, at least one evidence claim. It is
  **false**, because `closure_sha256` needs the retail content catalog
  (F14-D) and decoded mission programs (F37/F38), and the checklist entries
  below are unbound.

## What the record deliberately does not claim

- `verified` is `false`; `closure_sha256` is `null`; `evidence_ids` is
  empty. The evidence *report* for this task carries `unknowns: []` because
  `tools/validate_evidence.py --require-pass` rejects a report with
  unresolved task issues — the task's own acceptance is complete. The
  product-incompleteness state is not deleted anywhere: it lives in
  `missions/bindings/M01.json` (`unknowns`), in this file and in the
  evidence report's `review.method`.
- Only the `mission_identity` cell of M01's campaign record is `Complete`.
  The other six cells are `Unknown`, all 23 subsystem rows per mission are
  `Unresolved`, all 24 progressions are `Unknown`, and
  `CoverageReport::is_ready()` is false. The test asserts every count.
- No objective, actor, spawn, route, reward, difficulty or failure/success
  value is bound or guessed. No opcode, timing, count or coordinate was
  read from a walkthrough.
- Which reader member is M01's control program is **not** established: the
  program identity names the mission's reader archive as a whole, and the
  mission opcode table remains unmeasured (F13-C ships an empty signature
  table).

## Test inventory (`accept_m01_a_*`, 8 tests)

| Test | What it pins |
| --- | --- |
| `accept_m01_a_source_derived_binding_has_no_unresolved_critical_dependencies` (retail) | the scenario: all five dependencies resolved with the right evidence class, the installation hash re-measured independently, the ids carry the kinds their roles mean, the cited directory exists, every span re-hashed and in range, and `verified` false with the unbound checklist entries still named |
| `accept_m01_a_the_committed_record_is_what_the_installation_derives` (retail) | `missions/bindings/M01.json` is byte-identical to what production code derives, and carries the schema's fields |
| `accept_m01_a_a_title_the_local_strings_do_not_carry_is_unresolved` (retail) | a title miss never resolves: title and the identities that depend on it stay unresolved, the installation hash stays resolved, the identity cell reads `Unknown`, and no span is cited |
| `accept_m01_a_the_campaign_keeps_everything_else_unresolved_and_unready` (retail) | 24 missions, 168 cells, exactly 1 complete, 552 subsystem rows all unresolved, 24 unknown progressions, `is_ready()` false, and M01's closure counts 7 cells / 1 complete / 23 unresolved subsystems |
| `accept_m01_a_the_retail_title_block_binds_every_campaign_position` (retail, **added in review**) | the join at all 24 positions: every row of the retail title block binds the campaign position its index names, the mission/world/program ids are the ones that campaign entry declares (never a constant), and every selected mission directory and reader archive exists on disk |
| `accept_m01_a_a_title_outside_the_retail_title_block_resolves_no_position` (retail, **added in review**) | a retail title the strings carry but that sits in no campaign-length run resolves the title and the installation hash and **nothing** else: no position, no mission id, no world group, no program, and the identity cell stays `Unknown` |
| `accept_m01_a_verified_and_unresolved_are_two_distinct_states` (synthetic) | both predicates are real: a complete authored record reads verified, one unresolved dependency stops it and is named, a resolved value with no value is refused, and clearing `unknowns` while a dependency is unresolved still does not verify |
| `accept_m01_a_the_critical_dependency_set_is_the_checklists_first_five` (synthetic) | the critical set is exactly the checklist's five labels, unique and in order |

Every test calls production code (`SourceContext`, `SourceBinding`,
`CampaignBindings`); none repeats an expected value read from the record it
checks — the retail assertions are re-measured from `$CS_GAME_DIR`, from
the string table the installation holds, or from the committed inventory.
The two join tests read their titles out of the retail string table at
runtime, so no retail title string is repeated in the test source.

## Mutation probes (applied, then reverted; results are the observed runs)

| Mutation | Observed result |
| --- | --- |
| `SourceBinding::unresolved_critical` returns an empty vector | 2 tests fail: `accept_m01_a_a_title_the_local_strings_do_not_carry_is_unresolved` (a title the local strings do not carry was reported resolved) and `accept_m01_a_verified_and_unresolved_are_two_distinct_states` |
| `SourceBinding::is_verified` always returns `false` | 1 test fails: `accept_m01_a_verified_and_unresolved_are_two_distinct_states` (the complete authored record must read as verified, so the predicate cannot be a constant) |
| `SOURCE_BINDING_UNKNOWNS` emptied (no checklist entry recorded as unknown) | 2 tests fail: `accept_m01_a_source_derived_binding_has_no_unresolved_critical_dependencies` (the named checklist entries are gone) and `accept_m01_a_the_committed_record_is_what_the_installation_derives` (the committed record no longer matches) |
| `CriticalDependency::MissionId` reported resolved even when `catalog_id` is `None` | 1 test fails: `accept_m01_a_a_title_the_local_strings_do_not_carry_is_unresolved`, through `SourceBinding::validate`'s "resolved but carries no value" rule |
| `SourceContext::campaign_position` answers `Some(0)` for every title (**review probe**) | 2 tests fail: `accept_m01_a_the_retail_title_block_binds_every_campaign_position` (position 1 does not bind position 1) and `accept_m01_a_a_title_outside_the_retail_title_block_resolves_no_position` (nothing is left to witness). Before review **no** test failed this mutation |
| the campaign block-length check removed, position computed for any run (**review probe**) | 1 test fails: `accept_m01_a_a_title_outside_the_retail_title_block_resolves_no_position` (every row resolves a position, so its witness does not exist). The stage's headline test passes this mutation |
| the mission id fixed to `mission/ch1-m01` for every position (**review probe**) | 1 test fails: `accept_m01_a_the_retail_title_block_binds_every_campaign_position` (position 1 must not be M01). Before review **no** test failed this mutation either |

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m01_a_ --include-ignored` | 0 (8 tests: 6 retail, 2 synthetic) |
| each of the 8 tests alone with `--exact --include-ignored` | 0 (1 passed each) |
| `python3 tools/validate_evidence.py private/evidence/M01-A/acceptance.json --artifact-root private/evidence/M01-A --require-pass` | 0 |
| schema check of `missions/bindings/M01.json` against `schemas/mission-binding.schema.json` | conforms, including the `verified`-implies clause (not committed as a tool: the workspace ships no JSON-schema validator) |

## Recorded unknowns (not guessed)

- **`langui.dll` is not routed by `cs_formats::text`.** `cs-inspect config`
  refuses it ("an extension alone routes nothing"), so the localized
  mission-title table is reachable only through `StringCatalog` today.
  Filed as #372 (`F12-D.langui`); this stage calls the same production
  reader directly rather than adding a dialect rule outside its owner paths.
- **String-id numbering is not established.** The engine numbers a row
  `(block - 1) * 16 + index` (`cs_formats::string_id`, marked `Documented`),
  while the documented Win32 rule for the same `RT_STRING` leaf is
  `block * 16 + index`: for the title block at `langui.dll` offset 92088
  (resource name 218) that is 3480 against 3496, one block apart, uniform
  for every row. M01-A is unaffected (the join uses only contiguity and
  index, and the record carries no string id), but any later stage that
  addresses a string by id must settle which numbering it means. Filed as
  #374, not guessed.
- **No campaign-definition record was found.** The work-order ↔ retail-mission
  mapping rests on the join described above. `crimson.exe` and
  `crimson.icd` hold no plaintext mission list, and none was decompiled.
- **The chapter ↔ region naming is not established from a definition
  file**; it is consistent with the sound names, the region prefixes of the
  localized long-name block and the chapter cutscenes, and is recorded as
  supporting observation only.
- **The mission program is the reader archive as a whole.** Which member is
  M01's control program, and every opcode in it, is unmeasured (F13).
- **No original run was observed.** Nothing here is a behaviour claim.

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`cs_content::config::StringCatalog` and the platform filesystem;
`schemas/mission-binding.schema.json`; `missions/M01.md`;
`docs/contracts/SCRIPT-MISSION.md`; `docs/contracts/IDENTITY-CONTENT.md`;
`docs/findings/2026-09-29-f13-b-locate-and-classify-programs.md`;
`docs/findings/2026-09-29-f50-a-binding-coverage-records.md`;
`docs/findings/2026-09-29-f12-c-typed-tuning-and-localized-ids.md`.
