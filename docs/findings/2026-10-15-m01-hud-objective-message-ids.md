# M01-HUD-OBJECTIVE: the four M01 objective message ids resolve through `strings.dll`'s own `{name, id}` table, and the HUD projection that draws them

Date: 2026-10-15. Task: #1462 `M01-HUD-OBJECTIVE` (follow-up of #1438
`M01-PLAY-RADIO`, whose list included "show the current objective on a minimal
HUD"). Test prefix `accept_m01_hud_objective_`. Capabilities used: `retail`
(read of `$CS_GAME_DIR`) and ordinary build/test. **No original executable was
run**; nothing here is `verified_original`, and every statement below is a
reading of shipped files or of this project's own code. Addresses are file
offsets in `strings.dll` or virtual addresses in the decrypted image, as
labeled.

## The question, and the short answer

M01's four HUD objectives are the four blocks whose `IDENTITY` carries a
message id — O9 `PRIMARY 1` + `MSG_BRF_NWM1_OBJ1`, O11 `PRIMARY 2` +
`MSG_BRF_NWM1_OBJ2`, O15 `PRIMARY 3` + `MSG_BRF_NWM1_OBJ3`, O18 `PRIMARY 4` +
`MSG_BRF_NWM1_OBJ4` (M01-B-OBJ-2). The task asked whether those ids correlate
with the `RT_STRING` images **through the resource headers** (F12's
`account_string_ids` / `ResourceHeader::defines`), to measure that correlation
for the four ids before building anything, and to say so rather than invent
captions if the installation holds no text for them.

Short answers:

1. **The installation does hold the text** — all four ids resolve to
   non-empty `RT_STRING` rows of `strings.dll`, in language 1033.
2. **Not through the headers.** Neither shipped header defines a single
   `MSG_*` name, so `ResourceHeader::define` cannot pair a message name with
   an id; the task's header correlation does not exist on that path.
3. The pairing that *does* exist is `strings.dll`'s own `.data` table of
   `{name pointer, string id}` records — measured by F22-H as a vocabulary
   table, promoted here to production code, and pinned for these four ids at
   **16 100, 16 101, 16 102 and 16 103**.

## Fingerprints

| Item | Value |
| --- | --- |
| `install_sha256` (production `cs-inspect inventory`) | `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` |
| `content_sha256` (same run) | `a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d` |
| `strings.dll` | `7582fecaca42d21dd44eb95f896dcb415af790c7057ad81c8e1600ac0b445c21`, 131 072 bytes (F12-G, F22-H) |
| `GOSDATA/ASSETS/crimson.rof` | 846 members (F22-H) |

No original string text is reproduced in this file: ids, counts, byte
offsets, lengths and digests only.

## 1. The headers do not pair message names with ids

The production account of the whole installation
(`cs-inspect config-account --cs-path "$CS_GAME_DIR" --out …`, exit 0) carries
the `#define` names of both shipped headers — 521 `IDS_`, 185 `SB_`, 6 `STR_`
and 52 `FONT_` name occurrences — and **zero** occurrences of `MSG_` anywhere
in its report. F39-E1 measured the same pair of files directly: "both shipped
generated headers … define **no** `MSG_*` id at all — 0 of their 820 defines".

So `ResourceHeader::define(b"MSG_BRF_NWM1_OBJ1")` answers nothing for these
names, `account_string_ids` has no such id to cross, and a reader that tried
to reach the text that way would find no id rather than a wrong one. That is a
**correction of the task's premise**, recorded here as evidence rather than
treated as missing data: the header path is real, and message names are not on
it.

F39-E1 also recorded, out of band, "nothing shipped pairs a name with a row
id" — that half is **corrected below**: the pairing exists, in `strings.dll`'s
data section, and the counts F39-E1 compared (5 161 spellings against 1 792
rows) were comparing the name *block* with all *rows* instead of with the
table's own ids.

## 2. The table that does pair them

`strings.dll` is PE32 (`magic 0x010b`), `ImageBase 0x10000000`, five sections,
`.data` at `VirtualAddress 0x6000` with `SizeOfRawData 0xC000`. Inside it:

| Property | Measured value |
| --- | --- |
| Record | two little-endian `u32`s: an image virtual address, then an `RT_STRING` id |
| First run | file `0x6030`–`0x8027`, 1 023 records |
| Second run | file `0xcb88`–`0xeb87`, 1 023 records, byte-identical to the first |
| Terminator after the second run | `{0x10001246, 0xffffffff}` at `0xeb80` |
| Name block | file `0x8028`–`0xcb86`: 1 023 NUL-terminated ASCII names, each 4-byte aligned |
| Pointer order | strictly descending over the block in both runs (the runs walk the same block) |
| Id span | 100 … 17 142, all distinct (1 023 ids) |

F22-H measured this table first (both runs, the `PDT` pair with id `0` that the
scan drops, and the control vocabulary it labels); this task re-derived it and
measured what F22-H did not need: **how the table relates to the string rows**.

### The correlation the task asked for

Set of the table's 1 023 ids and set of `strings.dll`'s non-empty `RT_STRING`
units are **equal** — both differences are empty (0 ids named but empty, 0
non-empty rows unnamed). The image holds 1 792 units of which 1 023 are
non-empty (F12-G's counts, re-derived here with the same numbering
`(block - 1) * 16 + index` the catalog uses). So for this image the table is a
**bijection between names and non-empty rows**: every name labels exactly one
non-empty row, and every non-empty row has exactly one name.

That is what makes a message name a usable key rather than a hint, and it is
why the production reader can refuse a contradiction instead of picking a row:
a name that answered two ids, or an id with two names, cannot occur in this
image, and the reader reports it if it ever appears in another one.

## 3. The four ids

| Message name (element 2 of `IDENTITY`) | id | `RT_STRING` block | index | language | container | row length (UTF-16 code units) |
| --- | --- | --- | --- | --- | --- | --- |
| `MSG_BRF_NWM1_OBJ1` (O9, `PRIMARY 1`) | 16 100 | 1007 | 4 | 1033 | `strings.dll` | 63 |
| `MSG_BRF_NWM1_OBJ2` (O11, `PRIMARY 2`) | 16 101 | 1007 | 5 | 1033 | `strings.dll` | 33 |
| `MSG_BRF_NWM1_OBJ3` (O15, `PRIMARY 3`) | 16 102 | 1007 | 6 | 1033 | `strings.dll` | 64 |
| `MSG_BRF_NWM1_OBJ4` (O18, `PRIMARY 4`) | 16 103 | 1007 | 7 | 1033 | `strings.dll` | 48 |

Four consecutive rows of one block, four different lengths — a row that was
empty or undecodable would have broken the retail test rather than been filled
in. The lengths are structure; the text itself is read at run time through the
production readers and asserted only against what the same catalog resolves,
so no original string is written into any test.

## 4. What the code does now

| Piece | Where | What it is |
| --- | --- | --- |
| Message-table reader | `crates/cs_formats/src/text/message_ids.rs` | Scans an image for 8-byte `{virtual address, id}` records whose address maps through `PeLayout::map_rva` to a 4–60 byte NUL-terminated ASCII name of letters/digits/`_` and whose id lies in `1..=99_999`; collapses a repeated identical pair (the table is stored twice) and keeps a *differing* repeat so `resolve` reports `Ambiguous`. Bounded by the parse's allocation budget; content failures skip a record, never the read. |
| `ImageBase` | `crates/cs_formats/src/pe_resources.rs` | `PeLayout::image_base`, read from the documented optional-header field (PE32 +28 / PE32+ +24) so a caller holding a virtual address can subtract it before `map_rva`. |
| The pairing | `crates/cs_content/src/config.rs` | `MessageCatalog::read` reads a PE image's strings and its message table from the **same bytes**; `resolve(name, language)` answers `Found` / `NoString` / `Ambiguous` / `Missing`, never a choice between two rows. |
| HUD read model | `crates/cs_app/src/ui/hud/objectives.rs` | `ObjectiveHud::from_engine` projects `EngineObjectives` into identity rows (role code, ordinal, verbatim message, live/completed) and picks `current`. `MissionMessages` keeps the installation catalog *or the reason it could not be read*. |
| The widget | same file, `ObjectiveHudPlugin` | One absolutely positioned `Text` node in the windowed composition: caption `OBJECTIVE — DESIGNED LAYOUT` above the current line; hidden when nothing is current; the message **key** is drawn when the catalog is absent or the id does not answer (spec F12 non-negotiable #4's diagnostic key). |
| Wiring | `crates/cs_app/src/mission_session/compose.rs` | The windowed face reads `strings.dll` from the stage's installation root, inserts `MissionMessages` and adds the plugin; the headless face does neither, so acceptance tests drive the read model directly. |

### The projection rule, and why it is labelled designed

A row is **live** when the lifecycle model marks it present, not completed and
`Awake` — the state in which the engine evaluates an objective — and
`current` is the live row with the smallest (role code, ordinal, index).
Everything else about the original's choice is unmeasured: which of several
awake objectives it draws, whether a completed one stays on screen while the
record naps its successor (M01's nap hops — O10, O49, O50 — carry no
`IDENTITY` at all), and what it shows when nothing is awake. The measured
completion-time HUD pair carries only a role code and an ordinal (M01-B-OBJ-2),
so nothing here could be taken from the executable; the rule and the layout are
**designed**, stated in the module doc and printed on screen.

## 5. Tests

`cargo test --workspace --locked -- accept_m01_hud_objective_ --include-ignored`
discovers **5** tests (4 ordinary, 1 `#[ignore = "requires CS_GAME_DIR"]`):

| Test | What it pins | Runs in CI |
| --- | --- | --- |
| `…_read_model_lists_every_identity_in_objective_order` | five identity rows in objective order with role/ordinal/message verbatim; all dormant → no current row, no line | yes |
| `…_read_model_follows_the_lifecycle_model` | no line while only identity-less objectives are awake; first wake makes `PRIMARY 1` current; a woken secondary does not displace it; completion moves the line to the next primary and hides it when nothing is live; the no-catalog line is the message key | yes |
| `…_read_model_of_a_record_without_identity_is_empty` | "no identity rows" is distinguishable from "no current row" | yes |
| `…_messages_absent_keeps_why_it_is_absent` | an unreadable `strings.dll` keeps its reason in `MissionMessages` instead of vanishing | yes |
| `…_retail_installation_text_and_the_current_objective` | the four names resolve to ids 16 100–16 103 and to non-empty language-1033 rows of `strings.dll`; the table holds 1 023 records; the read model over M01's real recovery shows the five rows, nothing at launch, `PRIMARY 1` after its wake and `PRIMARY 2` after the record's own latch hops — each drawn line equal to what the same catalog resolves | no (`#[ignore]`) |

Mutation checks, applied to production code, run and reverted:

| Mutation | Caught by |
| --- | --- |
| `ObjectiveHud::from_engine`: `live` forced to `false` | `…_read_model_follows_the_lifecycle_model` failed (`OBJECTIVE9 is awake and uncompleted`) |
| `MessageCatalog::resolve`: always `MessageLookup::Missing` | the retail test failed (`MSG_BRF_NWM1_OBJ1 must resolve to exactly one string row: Missing`) |

## 6. Commands

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 (on the rebased head) |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 (pre-rebase head and rebased head) |
| `cargo test --workspace --locked` | 0 (pre-rebase head; see [Evidence](#evidence)) |
| `cargo test --workspace --locked -- accept_m01_hud_objective_ --include-ignored` | 0 (5 discovered, 5 executed, 5 passed; re-run on the rebased head) |
| `cs-inspect inventory --cs-path "$CS_GAME_DIR" --out …` | 0 (the two fingerprints above) |
| `cs-inspect config --file "$CS_GAME_DIR/strings.dll" --cs-path "$CS_GAME_DIR"` | 0 (`strings: 1792`, `other_leaves: 2`, `languages: [1033]`) |
| `cs-inspect config-account --cs-path "$CS_GAME_DIR" --out …` | 0 (zero `MSG_` occurrences; both headers present) |
| `python3 tools/validate_evidence.py private/evidence/M01-HUD-OBJECTIVE/acceptance.json --artifact-root private/evidence/M01-HUD-OBJECTIVE` | 0 (`structurally_valid: true`) |

## 7. Evidence

This task used `retail`, so `docs/contracts/CLI-EVIDENCE.md` applies:
`private/evidence/M01-HUD-OBJECTIVE/acceptance.json` records the task
selection's real run (argv, working directory, exit code, counts, the five
assertions and the log artifact's digest) plus the installation fingerprints
above, and a copy is committed as
`docs/findings/evidence/M01-HUD-OBJECTIVE.json`.

It is validated **without** `--require-pass`: that flag rejects any report
whose `unknowns` are non-empty (exit 3, `Unresolved issues`), and section 5 of
this file's [What stays unknown](#5-what-stays-unknown) list is the honest
content of those `unknowns` — deleting it to turn the flag green is exactly
what the contract forbids. Every other condition the flag checks is met: exit
code 0, 5 executed, 5 passed, every assertion `pass` with evidence.

**On the rebased head.** `origin/main` moved while this branch was built
(mod-root footprint work: `crates/cs_assets/src/mods.rs`,
`crates/cs_content/src/mods/{mod,mount}.rs` and two findings). The rebase
applied without conflicts, brought in no file this branch changes and no
`Cargo.toml`/`Cargo.lock` — the merge-race case of the owner's 2026-10-01
directive — so the plain workspace run was not repeated there. What *was*
re-run on the rebased head: `fmt`, `clippy`, the task selection (which is the
report's `command`), and therefore the report's `candidate_tree`. The
`workspace-test.log` artifact is the run on the **pre-rebase** head: same
branch code, and the only bytes the rebase added are listed above.

## 5. What stays unknown

Each names the affected content and what would resolve it.

1. **Which objective the original draws, and for how long.** Affected content:
   M01's four primary objective lines (and every later mission's). The choice
   here (live, lowest role/ordinal) is designed; the measured completion HUD
   pair carries only role and ordinal. Resolved by: an owner run of the
   original (REF-OWNER-FIRST-CAPTURE, #358) or a measurement of the briefing
   table builder's consumer (0x4acd1b) in `CS_ENGINE_IMAGE`.
2. **Whether a line persists between two identity objectives.** M01's chain
   reaches `OBJECTIVE11` through three identity-less latch hops, so the
   designed rule hides the line for that stretch. Affected content: the same
   four lines. Resolved by: the same original-run evidence.
3. **The layout.** Position, caption, font size and the hide-when-empty
   behaviour are designed and labelled on screen; rendering fidelity is the
   owner's review (spec F46, `AC04`/`F46-D`).
4. **Whether the original resolves an `IDENTITY` message name through *this*
   table at run time.** The table exists and the briefing/UI table builder is
   the message element's measured consumer (M01-B-OBJ-2), but the lookup path
   inside the executable was not re-measured for this task; F22-H measured the
   table's control-vocabulary side instead. Affected content: any claim that
   the pairing is the engine's own lookup. Resolved by: a static analysis of
   the message path around 0x4acd1b, or an original-run capture.
5. **A second installation.** Only one English build is available, so the
   table's position, the `1..=99_999` id bound and the 1 023-record count are
   single-installation observations (F12-G's unknown #5 applies to this too).

## What is not claimed

- No behaviour of the original game: the original executable was not run;
  `retail` here means read access to the installation's files.
- Nothing here is `verified_original` or `release_approved`; a Rally merge
  awards `checked` at most, and no agent review replaces the owner's approval.
- No original string text appears in this file, in the tests or in the
  committed evidence report; the texts are compared between two production
  readers instead.
- The widget does not claim F46's original HUD: F46-D's comparison is
  outstanding, and its `AC04` is untouched by this task.

## Sources

`$CS_GAME_DIR` read-only (`strings.dll`, `GOSDATA/ASSETS/crimson.rof` through
the production readers); `specs/F46-hud-instruments-mission-map-and-pause.md`
(F46's owner paths, `AC04`), `specs/F12-text-configuration-strings-and-pe-resources.md`
(non-negotiables #3, #4 and #5); `docs/contracts/IDENTITY-CONTENT.md`,
`docs/contracts/UI-NETWORK.md`, `docs/contracts/CLI-EVIDENCE.md`;
`docs/findings/2026-10-02-f22-h-original-control-vocabulary.md` (the table),
`docs/findings/2026-10-03-f39-e1-objective-dormant-reveal-lifecycle.md` (the
identities and the header negative),
`docs/findings/2026-10-12-m01-b-obj-2-objective-lifecycle-count-identity-cues.md`
(the four ids and the HUD pair),
`docs/findings/2026-10-12-m01-b-wake-world-facts-that-wake-m01-finish-blocks.md`
(the chain the retail test walks),
`docs/findings/2026-09-29-f12-g-strings-dll-resources-and-header-id-correlation.md`
(the id numbering and the image's counts); and the production code named in
section 4.

**Identities.** Implementer: `bunny-alpha-1` (Rally #1462 implement claim).
Reviewer: recorded by the Rally review claim that follows; a review by the
same agent instance is fresh-context re-derivation at best and is never
independent original-reference evidence.
