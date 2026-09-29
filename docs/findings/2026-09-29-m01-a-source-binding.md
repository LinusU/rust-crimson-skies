# M01-A: the first source-derived mission binding

Date: 2026-09-29. Task: M01-A "Bind original mission data and branches"
(#258, `missions/M01.md`, work order `M01-A`). Shared contract:
`docs/contracts/SCRIPT-MISSION.md`; identity rules
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: `retail`
(`$CS_GAME_DIR` read-only, never written), `synthetic`. Implementer:
**opencode-1**.

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
  `accept_m01_a_*` tests. `crates/cs_app/tests/campaign/evidence.rs` (owner
  path): the evidence harness, deliberately not prefixed `accept_m01_a_`.
- Wiring only (AGENTS rule 1): `crates/cs_content/src/lib.rs` (one doc
  paragraph), `crates/cs_app/tests/campaign/main.rs` (two `mod`
  declarations and a doc paragraph), `crates/cs_app/Cargo.toml`
  (`cs_assets` dev-dependency), root `Cargo.lock` (the new
  `cs_app -> cs_assets` edge).

**One observable failure:** if `SourceContext::bind` resolved
`CriticalDependency::MissionId` without producing a `catalog_id`, or if the
campaign block check that ties the localized-title block to the declared
campaign length were removed, then
`accept_m01_a_source_derived_binding_has_no_unresolved_critical_dependencies`
fails: it re-reads the installation, re-measures the installation hash,
re-reads every cited span and asserts that the resolved identity selects a
mission directory that really exists. That is the failure the stage exists
to prevent — a binding that reads as resolved while pointing at nothing.

## What was read from the installation

`$CS_GAME_DIR` was opened read-only. Nothing original is committed: the
record holds ids, hashes and byte ranges, and the evidence report holds the
same. Measured on this installation:

| Fact | Value | Where it comes from |
| --- | --- | --- |
| `install_sha256` | `b4e780ab…c631978` | `cs_assets::install::discover` + `fingerprint`, re-measured independently by the test |
| Campaign layout | `ZBD/C1`, `C1B`, `C1C`, `C2`, `C2B`, `C3`, `C4`, `C5`, each holding `M<nn>` directories | directory walk |
| Campaign size | 24 missions (5/5/5/5/4 per chapter) | directory walk, sorted by `(chapter, mission number)` |
| Localized title | RT_STRING id **3496**, language **1033**, text `The Lost Treasure` behind the display tag `[AB14I]` | `GOSDATA/ASSETS/BINARIES/langui.dll` read through `cs_content::config::StringCatalog` |
| Localized title block | ids **3496 … 3519**, contiguous, 24 rows | the maximal consecutive run of non-empty rows containing 3496 |
| Mission id | `mission/ch1-m01` | campaign position 0 → chapter 1, mission 1 |
| World group | `world/c1c` | the only chapter-1 group holding `M01`: `ZBD/C1C/M01/` |
| Program | `script/c1c-m01-zrdr` | `ZBD/C1C/M01/zrdr.zbd`, the reader archive F13-B classifies as holding mission programs |
| Source spans | `langui.dll` block `92088..92698` (610 bytes) and `ZBD/C1C/M01/zrdr.zbd` `0..91171` | `SourceSpan` of each asset, with each asset's SHA-256 |

`langui.dll` is in the F02 inventory (228 files), so its bytes are covered
by `install_sha256`.

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

## Test inventory (`accept_m01_a_*`, 6 tests)

| Test | What it pins |
| --- | --- |
| `accept_m01_a_source_derived_binding_has_no_unresolved_critical_dependencies` (retail) | the scenario: all five dependencies resolved with the right evidence class, the installation hash re-measured independently, the ids carry the kinds their roles mean, the cited directory exists, every span re-hashed and in range, and `verified` false with the unbound checklist entries still named |
| `accept_m01_a_the_committed_record_is_what_the_installation_derives` (retail) | `missions/bindings/M01.json` is byte-identical to what production code derives, and carries the schema's fields |
| `accept_m01_a_a_title_the_local_strings_do_not_carry_is_unresolved` (retail) | a title miss never resolves: title and the identities that depend on it stay unresolved, the installation hash stays resolved, the identity cell reads `Unknown`, and no span is cited |
| `accept_m01_a_the_campaign_keeps_everything_else_unresolved_and_unready` (retail) | 24 missions, 168 cells, exactly 1 complete, 552 subsystem rows all unresolved, 24 unknown progressions, `is_ready()` false, and M01's closure counts 7 cells / 1 complete / 23 unresolved subsystems |
| `accept_m01_a_verified_and_unresolved_are_two_distinct_states` (synthetic) | both predicates are real: a complete authored record reads verified, one unresolved dependency stops it and is named, a resolved value with no value is refused, and clearing `unknowns` while a dependency is unresolved still does not verify |
| `accept_m01_a_the_critical_dependency_set_is_the_checklists_first_five` (synthetic) | the critical set is exactly the checklist's five labels, unique and in order |

Every test calls production code (`SourceContext`, `SourceBinding`,
`CampaignBindings`); none repeats an expected value read from the record it
checks — the retail assertions are re-measured from `$CS_GAME_DIR` or from
the committed inventory.

## Mutation probes (applied, then reverted; results are the observed runs)

| Mutation | Observed result |
| --- | --- |
| `SourceBinding::unresolved_critical` returns an empty vector | 2 tests fail: `accept_m01_a_a_title_the_local_strings_do_not_carry_is_unresolved` (a title the local strings do not carry was reported resolved) and `accept_m01_a_verified_and_unresolved_are_two_distinct_states` |
| `SourceBinding::is_verified` always returns `false` | 1 test fails: `accept_m01_a_verified_and_unresolved_are_two_distinct_states` (the complete authored record must read as verified, so the predicate cannot be a constant) |
| `SOURCE_BINDING_UNKNOWNS` emptied (no checklist entry recorded as unknown) | 2 tests fail: `accept_m01_a_source_derived_binding_has_no_unresolved_critical_dependencies` (the named checklist entries are gone) and `accept_m01_a_the_committed_record_is_what_the_installation_derives` (the committed record no longer matches) |
| `CriticalDependency::MissionId` reported resolved even when `catalog_id` is `None` | 1 test fails: `accept_m01_a_a_title_the_local_strings_do_not_carry_is_unresolved`, through `SourceBinding::validate`'s "resolved but carries no value" rule |

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m01_a_ --include-ignored` | 0 (6 tests: 4 retail, 2 synthetic) |
| `python3 tools/validate_evidence.py private/evidence/M01-A/acceptance.json --artifact-root private/evidence/M01-A --require-pass` | 0 |
| schema check of `missions/bindings/M01.json` against `schemas/mission-binding.schema.json` | conforms, including the `verified`-implies clause (not committed as a tool: the workspace ships no JSON-schema validator) |

## Recorded unknowns (not guessed)

- **`langui.dll` is not routed by `cs_formats::text`.** `cs-inspect config`
  refuses it ("an extension alone routes nothing"), so the localized
  mission-title table is reachable only through `StringCatalog` today.
  Filed as a follow-up task; this stage calls the same production reader
  directly rather than adding a dialect rule outside its owner paths.
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
