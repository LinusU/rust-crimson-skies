# F12-D (`#372`, `F12-D.langui`): routing the localized UI string images through the `config` command

**Owner paths used:** `tools/cs_inspect/src/config.rs` (the routing and its
tests), `tools/cs_inspect/src/main.rs` (help text only).
`crates/cs_formats/src/text/dialect.rs` needed **no** change — see
"What was already true".

**Observable failure, before this change** (English installation, `retail`):

```text
$ cs-inspect config --file $CS_GAME_DIR/GOSDATA/ASSETS/BINARIES/langui.dll
cs-inspect: no observed dialect covers "langui.dll"; an extension alone routes nothing
$ echo $?
3
```

and the same file with the spelling the inventory writes:

```text
$ cs-inspect config --file $CS_GAME_DIR/GOSDATA/ASSETS/BINARIES/langui.dll \
    --container GOSDATA/ASSETS/BINARIES/langui.dll --string 3466:1033
$ echo $?
0
```

So the bytes were read fine; only the **spelling routing was tried** was
wrong. `string_id` block numbering is not involved in either run.

## What was already true (and therefore not changed)

The dialect inventory already carries the evidence-backed rule this task
asked for. `TEXT_DIALECT_INVENTORY`'s `pe.resources` row lists three
`MemberRule::Loose` entries, and `MemberRule::Loose`'s own definition spells
what such a rule is:

> /// A loose installation file.
> /// Path relative to the installation root.

| rule path | length | observed |
| --- | --- | --- |
| `strings.dll` | 131 072 | PE32, `machine 0x014c`, 112 `RT_STRING` blocks, 1792 counted units, language 1033, code page 1252 |
| `GOSDATA/ASSETS/BINARIES/language.dll` | 32 768 | PE32, 3 blocks, 48 units, language 1033, code page 0 |
| `GOSDATA/ASSETS/BINARIES/langui.dll` | 282 624 | PE32, 101 blocks, 1616 units, language 1033, code page 0 |

These are the F12-A survey rows and the F12-B retail structure test
(`accept_f12_b_retail_pe_resource_structure_matches_the_survey`), which
already asserts that the inventory routes exactly these three paths and that
`dialect_for_member("GOSDATA/ASSETS/BINARIES/langui.dll", None) ==
Some(TextDialect::PeResources)`. **Adding a rule, or a second rule by file
name, would have been a second, weaker alias for the same member** — the
guess the task explicitly rules out ("Not an extension guess: the rule must
be justified by what the file actually is", and IDENTITY-CONTENT
§Lookup contract: "Evidence-backed aliases are records with scope and test
coverage").

The gap was one layer up: `config_command_result` derived the routing
spelling from `path.file_name()` alone, so a rule written as an
installation-relative path could never match a file inside an installation.
`langui.dll` happened to be the only loose rule whose spelling was not also
its file name, which is why `strings.dll` always worked and the other two
never did.

## The change

`cs-inspect config` now tries the member spellings in a fixed, documented
order and keeps the one the observed rules routed
(`routing_candidates`, `installation_relative_spelling`):

1. the explicit `--container` spelling, and nothing else — an override names
   the member, so no inference may contradict it (unchanged behaviour);
2. the file's **installation-relative** spelling, when the file lies under the
   selected root (`--cs-path`, which wins over `CS_GAME_DIR`);
3. the file name, which is what a loose export carries (the previous
   behaviour, kept).

The spelling that routed is what the report's `source.container` records, so
provenance names the member the inventory describes. Nothing is mounted, no
other file of the installation is read, and no dynamic loading is introduced:
`installation_relative_spelling` is a pure path computation
(case-insensitive root match, `/`-joined remainder, validated through
`cs_types::install::RelativePath`, mirroring
`cs_assets::install::relative_spelling`).

A root that does not contain the file contributes no candidate. That is
deliberate: `--cs-path` is a **routing hint**, not a precondition for reading
the file, so a stale `CS_GAME_DIR` can never turn a working run into a
failure, and no new failure mode or exit code was introduced. When nothing
routes, the refusal now names every spelling that was tried, which is what
makes a wrong root diagnosable from the diagnostic alone.

`--install-sha256` is unchanged: without it the report still carries
`UNAFFILIATED_INSTALL_SHA256`. The routed spelling says *which member* the
bytes are; it is not a statement that the installation was fingerprinted.

## The exact command M01-A needed, on the real installation

```text
$ cs-inspect config --file $CS_GAME_DIR/GOSDATA/ASSETS/BINARIES/langui.dll \
    --string 3480:1033 --out private/…
cs-inspect: wrote config report to private/…
$ echo $?
0
```

| reported | value |
| --- | --- |
| `source.container` | `GOSDATA/ASSETS/BINARIES/langui.dll` |
| `pe_resources.accounting` | `{"strings":1616,"undecodable":0,"other_leaves":0,"duplicate_ids":0}` |
| `pe_resources.languages` | `[1033]` |
| lookup `3480:1033` | `found`, code page 0, span `{"offset":92088,"length":610}` |

`language.dll` (48 units) and `strings.dll` (1792 units, code page 1252, two
non-string leaves) route the same way, and the retail test asserts all three
plus the recorded lengths, accounting, language and code pages. The id 3480
row and its block extent are M01-A's recorded facts for the M01 localized
title, re-read through the CLI rather than through `StringCatalog` directly.
No original text is asserted, printed or kept by any test.

## Recorded unknowns (not guessed, not resolved here)

- **Which numbering the original addresses strings with is still open
  (#374).** `cs_formats::string_id` is `(block - 1) * 16 + index`, marked
  `Documented`, and the retail test's id 3480 is that engine's own numbering;
  the documented Win32 rule for the same leaf is one block apart, uniformly,
  for every row. This task makes ids *resolvable* through the CLI; it does not
  settle which id the original game would print, and no test claims it does.
  One lead for #374, found while confirming the rows: the original's own
  `ASSETS/SCRIPTS/RESOURCE.H` (comment: "Used by LangUI.rc") defines
  `IDS_MISSIONAREA 1220`, and `langui.dll`'s engine-numbered id 1220 is a
  region string. The block *name* of that leaf decides the question, and it
  was not measured here.
- **F12-D AC04 stays open.** Only one (English) installation was available,
  so "a localized installation preserves stable ids while changing display
  text" remains unproven; `langui.dll`'s language, code page 0 and block
  layout are single-installation observations.
- **Which resource API the game reaches these strings through** is unchanged
  and still listed on the `pe.resources` row (Win32 resource API, the
  project's own `RESOURCE.H`, or both).
- **`--string` still reports the engine's own id space**, not a verified
  original id space (see #374 above).

## Mutation probes

| Mutation | Failing tests |
| --- | --- |
| the installation-relative spelling is not tried | the routing test, `--cs-path` precedence, the refusal-naming test, the retail test (4 of 7) |
| the explicit `--container` is ignored | `…keeps_an_explicit_container_over_the_installation_path` |
| the refusal names only the first spelling | `…refuses_a_file_no_rule_covers_and_says_what_it_tried` |
| `CS_GAME_DIR` is ignored | `…cs_path_wins_over_the_environment`, the retail test |

The reviewer (`bunny-2`, a different agent instance with fresh context)
re-ran every one of these against the branch and reproduced the failing set
above, and added two more:

| Mutation | Failing tests |
| --- | --- |
| the root's components are matched case-sensitively | `…routes_a_file_whose_root_disagrees_in_case` |
| the joined spelling skips the `RelativePath` validation | `…an_empty_root_leaves_the_file_name_routing` |

## Review changes (bunny-2)

The reviewer added two acceptance tests, corrected one doc claim and stated
two rules in the module documentation, all inside
`tools/cs_inspect/src/config.rs`:

* `accept_f12_d_config_routes_a_file_whose_root_disagrees_in_case` pins the
  case-insensitive root match that `installation_relative_spelling` documents
  as the reason it exists, and that no test covered. A root spelled
  `/VAR/FOLDERS/…` routes a file spelled `/var/folders/…`, and the routed
  spelling keeps the file's own case.
* `accept_f12_d_config_an_empty_root_leaves_the_file_name_routing` pins the
  empty-root case: a root with no components is the empty prefix, so a
  relative `--file` is its own installation-relative spelling and an absolute
  one is rejected by `RelativePath` for being absolute — either way routing
  degrades to the file name, and an exported `CS_GAME_DIR=` lands in the same
  place. The relative half is asserted against the production function rather
  than by changing the process's working directory, which would race the other
  tests in the binary.
* `routing_candidates`' documentation claimed "two spellings that route to the
  same dialect are not two candidates". The code collapses only *identical*
  spellings; a case-differing pair stays two. The comment now says what the
  code does, and why the extra candidate is unreachable
  (`MemberRule::matches` compares ASCII case-insensitively).
* The module documentation now states the case-insensitive root match and the
  empty-root rule, and that the root is never looked up on disk.
* `…keeps_an_explicit_container_over_the_installation_path` now asserts the
  two-block accounting its fixture documents, so the image it read is
  distinguishable from the one-block image the routing test writes at the same
  path.

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f12_d_ --include-ignored` | 0 (7 tests: 6 synthetic, 1 retail; 5 before the review added two) |
| each of the 7 alone, `--exact --include-ignored` | 0 |
| `python3 tools/validate_evidence.py private/evidence/F12-D.langui/acceptance.json --artifact-root private/evidence/F12-D.langui` | 0 (see **Evidence**) |

## Evidence

The report is `docs/findings/evidence/F12-D.langui.json`, **regenerated by
the reviewer's own harness** (`private/evidence/F12-D.langui/harness.py`) on
the rebased review tree, as `docs/contracts/CLI-EVIDENCE.md` requires of the
reviewing agent: the task selection with `--include-ignored`, then each
discovered acceptance test again with `--exact --include-ignored` (both in
`cargo-test.log`), one `cs-inspect config` report per surveyed PE image with no
`--container` override, and one `cs-inspect inventory` run whose production
fingerprints supply `install_sha256` and `content_sha256`. 7 tests, 7 passed, 0
failed, 0 ignored. The installation and content fingerprints are byte-identical
to the implementer's earlier run, and the reviewer separately re-ran all three
`config` reports by hand and got the same containers, accounting, languages,
code pages and block extents, so the two reports describe the same
installation. The artifacts stay in `private/`; the three config reports
contain the installation's strings and are never committed.

It is validated **without** `--require-pass`, because that flag rejects a
report that still lists unresolved issues and this task's own acceptance is
partly that they stay recorded: #374 (which numbering the original addresses
strings with) and F12-D AC04 (a localized installation's stable ids) are open
and affect M02-A..M24-A. Dropping them to turn the flag green is exactly the
shortcut the owner directive forbids, so the flag exits 3 with "Unresolved
issues" and this file, the report's `unknowns` and its `review.method` say
why. The claim is `implemented`: a merge awards `checked` at most, and
nothing here observes the original game running.

## What is not claimed

No behaviour claim about the original game is made or implied. The original
executable was not run; `retail` here means read access to the installation
files. The change is a CLI routing fix on top of an already surveyed dialect
rule, so the level a merge can award is `checked`.

**Identities.** Implementer: `bunny-alpha-1` (Space Bunny Alpha), which wrote
the routing change, the tests and the first copy of this report. Reviewer:
`bunny-2` (Space Bunny Free), a different agent instance whose context was
fresh — it read the task, the F12 sheet section, `IDENTITY-CONTENT.md` and
`CLI-EVIDENCE.md`, then the branch. The review is an agent review, not
independent original-reference evidence, and it does not replace the owner's
approval.

## Sources

`$CS_GAME_DIR` read-only (the three PE images, and `ASSETS/SCRIPTS/RESOURCE.H`
read out through the production `cs-inspect rof` reader);
`specs/F12-text-configuration-strings-and-pe-resources.md` (non-negotiable
#1, AC04); `docs/contracts/IDENTITY-CONTENT.md` (lookup contract);
`docs/contracts/CLI-EVIDENCE.md`; the F12-A and F12-B findings; M01-A's
finding `docs/findings/2026-09-29-m01-a-source-binding.md`; and `docs/findings/2026-09-29-t351-keyed-list-reading-rules.md` for
the precedent on validating an evidence report whose unknowns stay recorded.
