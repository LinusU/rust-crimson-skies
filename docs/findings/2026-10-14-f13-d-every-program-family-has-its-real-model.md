# F13-D: every program family has its real model — there is no bytecode mission VM

Date: 2026-10-14. Task: **F13-D** (#640), re-scoped by the owner on 2026-10-05
from "approve a complete mission-language implementation plan" to "locate the
bytecode mission VM the F13 census never found" — answered by the owner's
static analysis: **there is none**. Feature sheet:
`specs/F13-mission-language-discovery-and-compatibility-closure.md` (the
`### F13-D` section; the owner's re-scoping supersedes its
compare-two-original-runs minimum scenario for this task, which stays the
spec's goal for a later stage). Shared contract:
`docs/contracts/SCRIPT-MISSION.md`. Capabilities used: **`retail`** (read-only
access to `$CS_GAME_DIR`) and ordinary build/test. Nothing was rendered,
played or run from the original executable; `retail` is file access, not
evidence of how the 2000 engine behaved, and nothing here is
`verified_original`.

## The premise that was wrong

F13-B/C walked **every** located program as a stream of numeric opcode words
against an empty ledger and stopped each one at its first counter with
`unknown_opcode` — 1452 stops on retail. The owner's static analysis of the
owner-supplied decrypted executable (`crimson.decrypted.exe`, sha256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, owner note
on #640 dated 2026-10-05; the image lives outside `$CS_GAME_DIR` since
2026-10-08) established that **none of the three families F13 walked is an
opcode stream**:

| Family | Real model | Owner-note evidence (address only, never bytes) |
| --- | --- | --- |
| mission reader members (`*.zrd`) | `.zrd` typed trees read **by keyword** by native consumers; tags 1 = int, 2 = float, 3 = string, 4 = list (count + 1), opened at VA `0x579c60` (`zrdr_main.cpp`); `objectives.zrd` is interpreted natively by `CZMission` (`0x466b70` parse, `0x46a490` update, vocabulary at `0x625eac`–`0x626b2c`) | data + a native interpreter, not instructions |
| `interp.zbd` script bodies (98) | a **named text-command language**: each line is a NUL-separated argument vector whose first token is the command name, matched by string compare in `zinterp_parse.cpp` (`0x5b845c`, `0x5b9a98`, `0x5baf58`; e.g. `GameGenSwapYZAxis` at `0x5b9841`, `SetBFETolerance` at `0x5bb980`) | the "instruction set" is the set of command names; there is no numeric opcode |
| animation payloads (`mis_anim.zbd` / `cam_anim.zbd`, magic `0x08170616`, version `0x35`) | streams of **typed event records** run over time by the animation runtime (`zeff_anim_run.c`, `0x4ecbb0`–`0x4ed2a0`), loaded by `0x522ef0` → `0x521b80`; the text twin's 41 event keywords live at `0x63341c`–`0x633718` | the "opcode" is the event type: a closed set with structured control flow (`LOOP`/`IF`…`ENDIF`/`CALLBACK`) |

So the F13 numeric opcode ledger applies to **none** of them, and a stopped
walk over a `.zrd` body, a command script or an event payload was the census
**misreading text and data**, never "unknown opcode" evidence. #630's rule
(numbered `OBJECTIVE<N>` blocks through the `.zrd` grammar) is the correct
model for the mission control member, and it generalises: every mission
member is data plus a native interpreter.

Static code evidence, accepted by the owner for this task; addresses and
summaries only, never decompiled code and never executable bytes (spec F13
non-negotiable #2).

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/script_raw/model.rs` (new): `ProgramModel`
  (`TypedDataTree`, `NamedCommands`, `EventRecords`), `ModelledProgramProbe`,
  `ModelError`, `NamedCommandsError`, `reads_as_named_commands`,
  `model_basis`, the `ANALYSIS_MODULE` / `*_RVA` constants. The typed place a
  family's real model and its executable-static-analysis basis are recorded.
- `crates/cs_formats/src/script_raw/probe.rs`: `ProbeSession::record_modelled_program`
  (beside `record_directive_program`), the `ProbeError::Model` refusal, and
  `modelled_programs` / `modelled()` / `modelled_by_model()` on the session
  and `ProbeReport`.
- `crates/cs_formats/src/script_raw/mod.rs`: module declaration, re-exports
  and one doc paragraph (wiring only).
- `tools/cs_inspect/src/script_discovery.rs`: the `scripts` command
  classifies each located program with its model before deciding to walk it
  (`model_selection`, `interp_lines_by_offset`, `structural_evidence`,
  `modelled_program_json`), report version `cs-inspect-scripts/5` (per-program
  `model`, `probe.modelled*`, `probe.model_basis`, `probe.modelled_program_rows`),
  the extended `signatures_complete`, the `accept_f13_d_*` tests and the
  `evidence_report_f13_d_*` harness; the two M01-B-OPC fixtures whose `.zrd`
  members now classify as typed trees, and the F13-C retail pin, updated with
  their new honest counts.
- `crates/cs_formats/tests/script_raw/main.rs`: the `accept_f13_d_*` machinery
  tests.
- This file, plus `docs/findings/evidence/F13-D.json` (the CLI-EVIDENCE
  report) and the correction banners in the F13-B and F13-C findings.

**One observable failure:** with `model_selection` returning `None` for every
family (the classification removed), the `scripts` report walks all 1452
retail programs as binary words again and
`accept_f13_d_retail_every_program_family_has_its_real_model` plus
`accept_f13_d_cli_classifies_every_family_with_its_real_model` fail: the
modelled counts drop to zero and `stops` regains `unknown_opcode`.

## What changed

**Producer → model → consumer.** The discovery (F13-B) still locates the same
1452 programs; before the probe session walks a program, the command
classifies it:

| Rule | Model | Evidence per program |
| --- | --- | --- |
| a reader-archive member that **decodes** as a `.zrd` typed tree (`cs_content::stunts::decode_zrd`, refusing any trailing byte) | `typed_data_tree` | `structural_decode` at `observed_tool`, at the program's own span |
| an INTERP script body whose every line heads with a printable ASCII token in the F07-validated line layout — or an empty body, which is only the validated zero terminator | `named_commands` | `structural_decode` at `observed_tool`, at the program's own span |
| an animation container's payload behind its **validated** header | `event_records` | `executable_static_analysis` at `documented`, at the loader VA `0x521b80` (owner note #640) |

A modelled program is filed with `record_modelled_program` — never walked —
and the session refuses evidence that cannot establish a meaning
(`weak_evidence`: a scan lead or anything below `documented`), mirroring
`SignatureClaim`. A program no rule classifies (a member that does not
decode, a body whose head is binary, an animation container whose header
dispatch could not validate) **keeps the binary walk and its honest stop**:
the census's failure was confidence without meaning, so classification here
never guesses on a name.

`signatures_complete` now counts a modelled program as accounted for, so on
retail every program is accounted (40 directive + 1412 modelled) and the
empty table has nothing left to walk. The fail-closed gate itself is pinned
on corpora that still hold binary programs by
`accept_f13_d_cli_keeps_an_incomplete_table_failing_closed` and
`accept_f13_c_cli_fails_an_incomplete_table_and_refuses_a_malformed_file`:
with `--signatures` supplied and a binary program remaining, the run exits 3.

The binary machinery is untouched: `walk_program`, `OpcodeLedger`, the
`accept_f13_b_*` and `accept_f13_c_*` session tests and the F13-B retail
minimum scenario (the empty ledger refusing `objectives.zrd`'s first counter
when walked directly) all still pass unchanged.

## Retail corpus result (`cs-inspect scripts --coverage`, report v5)

`$CS_GAME_DIR` read-only; no file written inside it:

| Count | Value |
| --- | --- |
| `.zbd` containers routed / script containers | 184 / 124 (1 + 62 + 61) |
| located programs | 1452 (unchanged from F13-B) |
| directive programs (M01-B-OPC #1356) | 40 |
| **modelled programs** | **1412** |
| → `typed_data_tree` | 1253 (every reader member outside the 40 control members decodes as a `.zrd` tree) |
| → `named_commands` | 98 (94 text bodies + 4 empty scripts with only the zero terminator) |
| → `event_records` | 61 (every animation container's header validated) |
| programs walked as binary words | **0** (was 1412 before this task) |
| `unknown_opcode` stops | **0** (was 1412) |
| programs resolved by a signature claim | 0 (the mission opcode table stays unmeasured) |
| inventory records / unused unknown | 320 / 0 (unchanged) |

The four empty INTERP scripts (bodies of exactly the 4-byte zero terminator,
at container offsets 20415, 46486, 78897 and 84086) are classified
`named_commands` with the evidence note "the body holds only the
F07-validated zero terminator": walking their terminator word as an opcode is
exactly the misreading this task corrects.

## The three questions the owner answered on 2026-10-14

1. **Are the compiled anim event-type ids resolved?** Yes — by F20 (#690),
   which measured the compiled stream's record layout over all 61 carriers
   (56 994 blocks, 242 391 events, 16 132 948 bytes) and resolved the event
   ids by joining them, statement for statement, to the installation's own
   event declarations — which are the owner's keyword spellings. This task
   therefore files no measurement follow-up for the anim payload layout; the
   `event_records` model cites F20's table as the resolved event-type source
   alongside the owner note.
2. **The two keyword tables cross-check.** The owner's note (41 keywords at
   `0x63341c`–`0x633718`, 43 entries after removing 2 duplicates) and F20's
   event-stream table describe the **same family in two vocabularies**, and
   the owner confirmed the cross-check that the keywords with no recorded
   compiled mention (`ACTIVATION`, `ON_CALL`, `ANIM_VERBOSE`,
   `GLOBAL_ANIM_VERBOSE`) correspond exactly to the four event ids that join
   no keyword (13, 17, 26, 28). This task records the owner's statement; the
   full per-pair reconciliation is follow-up **#1415 (F13-D-FU1)** — each
   pair (owner keyword, F20 spelling), whether the owner keyword is a
   compiled-event name, a reader-text record-vocabulary field or neither,
   the extra/missing names and one citation per pair, with **no model claimed
   until every name is placed**. The four unjoined ids stay unjoined until an
   original run or the original source measures them: follow-up
   **#1416 (F13-D-FU2)**, id → keyword name, reader-text twin, parameter
   list, timing/role and per-id evidence, over every id the table stores —
   a prerequisite to advance VS-M01-RUNTIME.

   **Owner question of 2026-10-14: is the reader-text-twin cross-check
   decidable?** Assessed: **yes in principle, per event, and it is the right
   first method** — with a measured premise and explicit refutation rules:
   the evidence chain is (1) #678's measured twin join (a carrier record's
   blocks are one declaration's sequence blocks, in order), (2) F20's
   measured statement↔event alignment (statement *k* ↔ event *k* once the
   leading `ACTIVATION`, which is never stored as an event, is dropped;
   zero conflicts over 477 pairs / 1440 sequences), and (3) the spelling at
   the unknown id's aligned position, cross-checked against the owner's
   elimination correspondence — the id value itself is the compiled event's
   own low byte, so the join is a value join, not a name borrowed from
   another game. It is **decided against** whenever a position joins an
   unknown id to an already-resolved spelling, when two occurrences of one
   id join to different spellings, or when a twin declaration is missing —
   and those outcomes are recorded, never forced. One sharpening for the
   implementing task: `ACTIVATION` is consumed by the alignment rule itself,
   so it cannot sit at an event position; if the four-way correspondence
   holds, the unknown ids should join the *remaining* no-mention keywords,
   and `ACTIVATION`'s no-mention status needs its own recorded explanation.
   The **premise is not yet measured**: whether the 389 unknown-carrying
   blocks (347 records, 16 carriers) all have declarations that align. F20's
   refusal was the duration gap, not proof of a missing twin. Measuring twin
   existence per block is #1416's first step; id **28** (exactly one event
   in the census) is the cheapest decisive probe.
3. **The report's record-level keys must be machine-readable.** Agreed and
   filed as **#1417 (F13-D-FU3)** (depends on #1415): every record-level key
   status becomes `measured` (with its citations) or `outside_vocabulary`
   (with the vocabulary name and the resolving task), replacing the
   `shape_measured`/`effect_measured` labels and the free-text `unmeasured`
   reason, with the owner's gate on the `scripts` command: *"Every measured
   key carries a citation; every unmeasured key names the vocabulary it is
   outside and the task that would resolve it."*

Nothing here advances VS-M01-RUNTIME or M01-B: the directive-program work and
its vocabularies are untouched, and the four unjoined event ids plus the two
keyword vocabularies stay open.

## Test inventory (`accept_f13_d_*`)

`crates/cs_formats/tests/script_raw/main.rs` (3 unignored + 1 retail) and
`tools/cs_inspect/src/script_discovery.rs` (2 unignored + 1 retail + the
evidence harness):

| Test | Covers |
| --- | --- |
| `modelled_programs_are_filed_not_walked` | the session path: a text body filed with its structural evidence, a scan lead refused `weak_evidence`, the binary probe intact beside it, the report rows and counts, `session_closed` after teardown |
| `reads_as_named_commands_checks_the_command_head` | two printable-headed lines → `Ok(2)`; an empty body → `EmptyBody`; a binary head → `HeadNotText`; an empty head → `EmptyHead` |
| `model_basis_is_the_owner_static_analysis` | every model's basis is `executable_static_analysis` at `documented`, at an executable address of `crimson.decrypted.exe`, citing #640; labels round-trip, an unknown label is refused |
| `retail_every_interp_body_reads_as_named_commands` | `$CS_GAME_DIR`: all 98 retail INTERP bodies read as named commands, four of them empty |
| `cli_classifies_every_family_with_its_real_model` | the command end to end: 4 modelled (incl. the empty script), 2 binary programs keeping their `unknown_opcode` stop, the per-program `model` field, the row evidence mix and the three `model_basis` addresses |
| `cli_keeps_an_incomplete_table_failing_closed` | with `--signatures` supplied and binary programs remaining: exit 3, `passes: false` — the F13-C gate still bites |
| `retail_every_program_family_has_its_real_model` | `$CS_GAME_DIR`: the measured split (40 / 1412 / 98 / 1253 / 61), zero binary walks, `stops: {}`, 1412 evidence rows with the exact method/confidence mix |
| `evidence_report_f13_d_writes_the_acceptance_report` | the CLI-EVIDENCE harness (`docs/contracts/CLI-EVIDENCE.md`), not an acceptance test |

Updated with their new honest counts (never weakened): the M01-B-OPC fixture
whose non-control `.zrd` members now classify as typed trees, and the F13-C
retail pin (retryable 1412 → 0, `complete` false → true, and the supplied
partial table now passing because nothing remains to resolve).

`cargo test --workspace --locked -- accept_f13_d_ --include-ignored`
discovers and executes 7 tests (0 ignored at that point), all passing; each
also passes when run alone with `--exact`.

## Mutation probes (applied, run, restored)

- `model_selection` returning `None` for every family →
  `accept_f13_d_cli_classifies_every_family_with_its_real_model` and
  `accept_f13_d_cli_keeps_an_incomplete_table_failing_closed` fail (0 passed,
  2 failed);
- `reads_as_named_commands` returning `Ok` unconditionally →
  `accept_f13_d_reads_as_named_commands_checks_the_command_head` fails, and
  so does the CLI classification test (the binary body is misclassified);
- the `establishes_semantics` check removed from
  `ProbeSession::record_modelled_program` →
  `accept_f13_d_modelled_programs_are_filed_not_walked` fails.

## Recorded unknowns (not guessed)

- **The mission opcode table stays unmeasured.** No signature claim is
  resolved anywhere; the model axis says the corpus holds no numeric opcode
  stream, not that every key, command and event parameter is understood.
- **The four empty INTERP scripts** are named-command programs with zero
  commands (only the terminator); what the original does with an empty script
  is unmeasured.
- **The two anim keyword vocabularies are not yet reconciled** (owner note's
  41 keywords vs F20's table): follow-up #1415. **No model is claimed on the
  strength of the correspondence**; the `event_records` model rests on the
  owner note plus F20's measured stream, not on the cross-check.
- **Four compiled event ids (13, 17, 26, 28) join no keyword**: follow-up
  #1416; their 496 events in 389 blocks stay refused by F20's `PlaybackGap`,
  as F20 recorded.
- **Record-level key statuses are not machine-readable yet**: follow-up
  #1417.
- **No original run was observed.** The classification is structural decode
  plus owner-accepted static analysis; it is `documented`/`observed_tool`,
  never `verified_original`.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f13_d_ --include-ignored
```

## Sources

`specs/F13-mission-language-discovery-and-compatibility-closure.md`,
`docs/contracts/SCRIPT-MISSION.md`, `docs/contracts/CLI-EVIDENCE.md`, the
owner notes on #640 (2026-10-05 with the addresses and the image sha256;
2026-10-08 the image path change; 2026-10-14 the F20 cross-check and the
three follow-up requirements), the F13-A/F13-B/F13-C findings (corrected
below), the M01-B-OPC finding
(`docs/findings/scripts/2026-10-12-m01-b-opc-directive-programs-in-the-f13-c-probe.md`),
the F20 event-grammar finding
(`docs/findings/2026-10-06-f20-event-grammar.md`), and the read-only
`$CS_GAME_DIR` listing.

## Reviewer note

Implementer: `mimo-1` (fresh session). Independent review requested per the
2026-10-08 owner directive (format/mission semantics); the reviewing agent
regenerates `docs/findings/evidence/F13-D.json` on the reviewed commit, as
`docs/contracts/CLI-EVIDENCE.md` requires. A review by the same agent
identity is not independent evidence (AGENTS.md "Reviewing").
