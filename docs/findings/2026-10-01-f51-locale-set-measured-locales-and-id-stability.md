# F51-LOCALE-SET: the supported-locale set measured from the installation, and what one installation still cannot answer

Date: 2026-10-01. Task: F51-LOCALE-SET "Measure the original supported-locale
set and localized-installation id stability" (Rally #467). Spec:
`specs/F51-localization-fonts-text-layout-and-original-media-ids.md` (deliverable,
AC03 and AC04) and `specs/F12-text-configuration-strings-and-pe-resources.md`
AC04. Shared contracts: `docs/contracts/UI-NETWORK.md` and
`docs/contracts/CLI-EVIDENCE.md`. Capabilities used: `retail` (read access to the
original installation at `$CS_GAME_DIR`) and `synthetic` for the unignored half
of the suite.

Implemented by `bunny-alpha-2/bunny-alpha-2`. This file is the implementer's
record; it is not an independent review and it awards no `verified_original` or
`release_approved`. The original executable was not run: `retail` here means read
access to the installation files, and it is never evidence of how the original
behaves.

## Observable failure (listed before editing)

F51-A made the supported-locale set and the resource-language map
caller-declared, because the original release's locale list was unmeasured, and
F51-D then audited the owner's installation against a set **the test author
wrote**: `crates/cs_app/tests/text/common.rs` held
`retail_declared_locales() = ["en-us"]` and `retail_language_map() =
{1033 -> "en-us"}`. Concretely:

- The declared locale set was not a measurement. Nothing read the resource
  language ids out of the original files, so a wrong or invented list would have
  produced a green audit, and the label `"en-us"` asserted a language name that
  no original file records.
- Nothing in the tree could answer "which languages do the original's own files
  carry?", so the claim could not be replaced by a measurement even in
  principle: there was no measurement path to remove or to trust.
- F12 AC04 — *a localized installation preserves stable ids while changing
  display text* — had no production comparison at all. `TextCatalog` could
  resolve a row per locale but nothing compared two locales' id sets, and a
  one-installation run had no way to say "not comparable" instead of implying
  stability.

## Files and what changed

- `crates/cs_content/src/localization.rs` (owner path): the measured declaration
  and the id-numbering comparison.
  - `measured_locale_label(language) -> "resource-<id>"` — the only label a
    measured locale may carry (see "What a measured label may not say").
  - `LanguageObservation` (`source: SourceSpan`, `language`, `rows`) and
    `ResourceLanguageTable` (`observe`, `observations`, `languages`, `rows_for`,
    `containers`, `is_empty`): the measured resource language table, one
    occurrence per container and language, each with the span that proves it.
  - `MeasuredLocalesError::{NothingMeasured, TooManyLanguages}` and
    `MeasuredLocales` — `from_table` **derives** the `SupportedLocales` set and
    the `LanguageMap` from the table; `table`, `supported`, `language_map`,
    `languages`, `locale_for`, `len`. An empty measurement and a table over
    `MAX_LANGUAGE_MAP_LEN` ids are refused, never truncated into a plausible
    list.
  - `IdNumbering` (`shared`, `only_first`, `only_second`, `identical_text`,
    `changed_text`; `is_stable`, `compared`, `renumbered`, `changed`),
    `TextCatalog::ids_for`, `TextCatalog::compare_locale_ids` and
    `TextCatalog::compare_installation_ids` — the F12 AC04 comparison. Stability
    is a property of the **id sets**; the text counts say whether the display
    text changed.
  - `IdStability::{SingleLocale, Compared}` and `measure_id_stability` — the
    honest one-installation answer. `IdStability::is_stable` is `false` for
    `SingleLocale`, so a single installation can never be reported as evidence
    of id stability.
  - Module docs: a new "Declaring the locale set from measurement" section, and
    the F51-A/F51-B "unmeasured" notes now point at the measurement instead of
    claiming the list is unknowable in principle.
- `crates/cs_app/src/text/locale_measure.rs` (owner path, new): reading the
  original files.
  - `string_image_languages(rows) -> Vec<(u32, usize)>` — the distinct
    third-level resource language ids of one image with each id's **row** count.
  - `StringImageMeasurement` and `measure_string_image_languages` — the
    installation's localization surface measured into a `ResourceLanguageTable`.
  - `ImageLanguages` (`path`, `install`, `bytes`, `languages`, `leaves`,
    `string_blocks`, `carries_only`), `ImageMeasure::{Resources, NoResources,
    NotPe}`, `ResourceLessImage`,
    `InstallationLanguages` (`install`, `files`, `images`, `without_resources`,
    `not_pe`; `languages`, `image`),
    `measure_image_languages` and `measure_installation_languages` — the
    installation-wide PE resource language census, over the production
    discovery's file set and the production PE reader. Every inventoried file
    lands in exactly one of the three lists, so nothing is skipped.
  - `LocaleMeasureError::{Discovery, Read, Layout}`: a file that cannot be read,
    or a PE image this project cannot parse, fails the run **by name**. A census
    that quietly dropped an unreadable image would not be a census.
- `crates/cs_app/src/text/mod.rs` (wiring): the module declaration, its
  re-exports and the module docs' new bullet.
- `crates/cs_app/tests/text/locale_set.rs` (owner path, new): the 5
  `accept_f51_locale_set_*` tests.
- `crates/cs_app/tests/text/common.rs` (owner path): `retail_declared_locales`
  and `retail_language_map` are **gone**. `RetailLocaleMeasurement`
  (`install`, `table`, `declared`, `rows`, `spans`, `catalog`) and
  `retail_locale_measurement` measure the installation, and `retail_audit` now
  takes its `SupportedLocales` and `LanguageMap` from
  `MeasuredLocales`. `measured_locale(language)` is the production label
  spelling, so a test looks a locale up with the label the declaration built.
- `crates/cs_app/tests/text/audit.rs` (owner path): the F51-D retail test
  asserts the **measured** language set and the measured label instead of
  `"en-us"`. Nothing else in that test changed.
- `crates/cs_app/tests/text/evidence.rs` (owner path): the F51-D evidence
  harness's census reports the measured declaration, so the committed F51-D
  report's `declared` field is no longer transcribed from a caller's list.
- `crates/cs_app/tests/text/locale_evidence.rs` (owner path, new): this task's
  evidence harness and the derived installation language census.
- `crates/cs_app/tests/text/main.rs` (wiring): `mod locale_evidence;` and
  `mod locale_set;`.
- `docs/findings/evidence/F51-LOCALE-SET.json` and this file.

## What was measured

Installation fingerprint (production discovery):
`install_sha256 = b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`
— the same digest F12-A/F12-D/F51-D recorded.

### 1. The localization surface: one measured resource language

Measured through `cs_content::config::StringCatalog::read` (production F12) and
`measure_string_image_languages` (production):

| image | container span | measured resource language | rows | span offset/length |
| --- | --- | --- | --- | --- |
| `strings.dll` | `strings.dll` | 1033 | 1 792 | 0 / 131 072 |
| `GOSDATA/ASSETS/BINARIES/langui.dll` | that path | 1033 | 1 616 | 0 / 282 624 |
| `GOSDATA/ASSETS/BINARIES/language.dll` | that path | 1033 | 48 | 0 / 32 768 |

One distinct language id (1033) across the three routed string images; 3 456
rows; each occurrence's `SourceSpan` carries the installation digest above and
the whole-container byte range. The F12-D reading of `[1033]` per image is
confirmed by a different production path and is now the *input* to the
declaration rather than a hand-written map.

### 2. The declaration derived from it

`MeasuredLocales::from_table` on that table yields exactly one declared locale,
labelled `resource-1033`, and a `LanguageMap` of `{1033 -> resource-1033}`. The
F51-D audit of the installation, run against that measured declaration, has no
`unmapped_language` and no `undeclared_locale` blocker: every measured row
decodes under the measured map, and the single measured locale answers every id
of every routed image (`strings.dll` 1 792, `langui.dll` 1 616, `language.dll`
48, all with 0 missing).

### 3. The whole-installation census, and why it is **not** the locale list

Every one of the 228 inventoried files was read: 18 are PE images with a
resource directory, 210 are not (a game archive, audio banks, video, two
bitmaps, a `rof` pair, the empty `EBUSetup.sem` marker, and
`GOSDATA/ASSETS/BINARIES/roffile.dll`, which is a PE image whose resource data
directory declares **zero** bytes) and 205 are not PE images at all. The 18
measured images carry these resource language ids:

| image | leaves | `RT_STRING` blocks | measured resource languages |
| --- | --- | --- | --- |
| `GOSDATA/ASSETS/BINARIES/ijl10.dll` | 1 | 0 | 1033 |
| `GOSDATA/ASSETS/BINARIES/language.dll` | 3 | 3 | 1033 |
| `GOSDATA/ASSETS/BINARIES/langui.dll` | 101 | 101 | 1033 |
| `SETUPENU.DLL` | 71 | 37 | 1033 |
| `UNINSTAL.EXE` | 23 | 17 | 1033 |
| `clokspl.exe` | 38 | 19 | 1033, **2057** |
| `crimson.exe` | 3 | 0 | 1033 |
| `crimson.icd` | 32 | 2 | 1033 |
| `cszoneregister.exe` | 0 | 0 | (none) |
| `dsetup.dll` | 1 | 0 | 1033 |
| `dsetup32.dll` | 153 | 119 | **1028, 1029, 1031, 1033, 1034, 1036, 1040, 1041, 1042, 1043, 1045, 1046, 1049, 1053, 2052, 2058, 3082** |
| `ebueula.dll` | 16 | 8 | 1033 |
| `ifc21.dll` | 1 | 0 | 1033 |
| `mcp.dll` | 10 | 5 | **0**, 1033 |
| `mfc42.dll` | 93 | 43 | 1033 |
| `msvcp60.dll` | 1 | 0 | 1033 |
| `msvcrt.dll` | 1 | 0 | 1033 |
| `strings.dll` | 114 | 112 | 1033 |

This is the most important measured result of the task, and it is a **negative**
one: the installation's *game* binaries (`strings.dll`, `langui.dll`,
`language.dll`, `crimson.exe`, `SETUPENU.DLL`, `UNINSTAL.EXE`) are single
language, and the 19 distinct resource language ids in the tree belong to
Microsoft's runtime, setup and EULA binaries (`dsetup32.dll` alone carries 17,
`mcp.dll` carries the neutral id 0, `clokspl.exe` carries Indonesian). Those ids
describe *Microsoft's* localized resources, not the game's supported locales, so
an installation-wide PE language census **cannot** be used to declare the
release's locale set. The declaration must come from the localization surface,
which is what the production path does.

Two further measured facts from the same census, recorded because they bound what
can be claimed later:

- `crimson.exe` imports `GetSystemDefaultLCID` and its 3 resource leaves carry no
  `RT_STRING` block, so the executable itself holds no language table. The
  original selects a language at runtime from the **operating system's** locale
  id, not from a table in the game. (Measured with a byte-level search of the
  executable, not by disassembling it; the import name is the evidence.)
- There is no locale *label* string anywhere in the installation: a
  whole-tree search for `en-us`, `en_us`, `fr-fr`, `de-de`, `es-es` and `it-it`
  matches nothing, in the archives and the executables alike. The only
  locale-name vocabulary present (`spanish-mexican`, `english-nz`,
  `swedish-finland`, `english-trinidad y tobago`, …) is Microsoft C runtime data:
  the same strings, typos included, are in `msvcrt.dll`, `clokspl.exe` and
  `dplayerx.dll`, so it is the CRT's locale vocabulary and not an original
  language table. **A language-name label therefore cannot be measured from this
  installation at all.**

### 4. Id stability across locales (F12 AC04): not comparable here

The comparison is implemented and runs, but one installation has one language, so
the measured result is `IdStability::SingleLocale { measured: resource-1033,
ids: 1792 }` for `strings.dll` — `is_compared() == false`, `is_stable() == false`.
That is the honest answer and it is a *typed* one: `IdStability::is_stable`
returns `false` for `SingleLocale` by construction, so no future one-installation
run can report stability by accident.

The comparison itself is exercised on synthetic data through the same production
function: two catalogs of one image with the same two ids and different display
text give `Compared { shared: 2, renumbered: 0, changed: 2, identical: 0 }`; a
catalog that drops one id and adds another gives
`Compared { only_second: [string_resource/2], renumbered: 1 }`; an untranslated
duplicate gives `Compared { identical: 2, changed: 0 }`. The retail test performs
the *real* comparison as soon as a second, localized installation is reachable
through `CS_LOCALIZED_GAME_DIR`, with no code change: it measures that
installation through the same production path, asserts it is a different
installation carrying a language of its own, and then compares
`strings.dll`'s id sets.

## What a measured label may not say

The derived label is `resource-<id>`, not a language name. Mapping resource
language id 1033 to "en-US" is a Win32 convention, and using it inside a
*declaration* would import an assumption about the 2000 release that no
measurement supports — the installation contains no locale label string at all
(§ 3). The label therefore names what the files said. An operator or a future
owner-supplied reference may map ids to language names, but that mapping is then
its own evidence and not part of this measurement.

## The design decisions a reviewer should check

1. **The declaration is derived, not supplied.** `MeasuredLocales` can only be
   built from a `ResourceLanguageTable`; an empty table and an over-wide table
   are refused (`NothingMeasured`, `TooManyLanguages`). There is no constructor
   that takes a list of locale names, so the F51-D test's old
   `retail_declared_locales()` has no successor.
2. **The measurement surface is the localization surface, and the census is
   context.** Deriving from an installation-wide PE language census would have
   declared 19 locales, 18 of which belong to Microsoft's runtime binaries. The
   census is still run, and the retail test asserts *which* images carry a
   non-measured language, so the shortcut stays visibly wrong.
3. **Row counts, not decoded counts.** `string_image_languages` counts every F12
   row of a language, including a row whose code units did not decode: "the image
   was built with this language" is true even of a string that did not decode.
4. **Stability is about id sets.** `IdNumbering::is_stable` ignores the text
   counts, so an unchanged proper noun is not a renumbering and a changed string
   under a vanished id is not stability.
5. **Unparseable is not language-free.** A file that is not a PE image is
   recorded as `not_pe` (including the empty marker file and the 16-bit
   `clcd16.dll`, which begins with `MZ` and is not a PE image); a PE image with a
   zero-length resource directory is recorded as an image without resources; any
   other reader failure fails the run by name.

## Non-negotiable behaviour this stage encodes

1. **No guessed compatibility claim.** The supported-locale set is a
   measurement or it does not exist; a label may not name a language the files
   did not record. F51's AC04 ("audit all strings and media for each declared
   supported original locale") is now over a *measured* set, and the residual
   ("one language, so not the release's set") is stated rather than papered over.
2. **Locale cannot change identity.** The comparison is over `TextId`s
   (`string_resource` content ids), and it changed nothing about how a row is
   stored, resolved or audited.
3. **Unknown means unknown.** F12 AC04 stays open with a typed
   `SingleLocale` result, a named owner requirement below, and no `verified_original`
   claim anywhere.

## Test selection and sensitivity

`cargo test --workspace --locked -- accept_f51_locale_set_ --include-ignored`
discovers and runs **5** tests, all passing (4 synthetic, unignored so CI runs
them; 1 retail test marked `#[ignore]` and run here with `--include-ignored` on
the real installation):

- `accept_f51_locale_set_measured_resource_languages_derive_the_declared_locale_set`
- `accept_f51_locale_set_the_audit_follows_the_measurement_and_never_a_guessed_locale`
- `accept_f51_locale_set_id_numbering_is_compared_across_two_measured_locales`
- `accept_f51_locale_set_one_measured_locale_reports_no_comparison_rather_than_stability`
- `accept_f51_locale_set_retail_the_installation_declares_only_measured_locales`
  (`#[ignore = "requires CS_GAME_DIR"]`)

Each also passes alone with `--exact`. Sensitivity was checked by mutating
production code and reverting:

| mutation | tests that fail |
| --- | --- |
| the declaration labels the measured ids with a language name (`en-us`/`other`) instead of `resource-<id>` | the declaration test and the retail test (`["en-us"] != ["resource-1033"]`) |
| `measure_string_image_languages` records only an image's first language | the declaration test |
| `string_image_languages` counts only rows that decoded | the declaration test |
| `IdStability::is_stable` answers `true` for `SingleLocale` | 3 tests, including the retail one |
| `IdNumbering::is_stable` ignores the id sets | the two-locale comparison test |
| the census keeps only an image's first resource language | the retail test |

## Evidence

- Report: `private/evidence/F51-LOCALE-SET/acceptance.json`, validated with
  `python3 tools/validate_evidence.py private/evidence/F51-LOCALE-SET/acceptance.json
  --artifact-root private/evidence/F51-LOCALE-SET --require-pass` →
  `{"structurally_valid": true, "artifact_count": 2, "claims_semantically_verified": false}`
  (exit 0). A copy is committed as `docs/findings/evidence/F51-LOCALE-SET.json`.
- Artifacts (stay in `private/`): `cargo-test.log` and
  `installation-language-census.json` — per-image paths, sizes, leaf/block
  counts, the numeric resource language ids, the measured surface, the derived
  declaration and the `id_stability` verdict. Counts and ids only: no original
  text, no original byte, no screenshot of original content.
- Capabilities declared by the report: `retail` and `synthetic`. The report's
  `claim` is `implemented`; a Rally merge would award at most `checked`.

## What remains unknown (recorded, not guessed)

- **The original release's supported-locale list.** Measured: this installation
  carries resource language 1033 in its localization surface, and no other
  language in any game binary. Not measured: which other languages a
  *localized* build of the release shipped. No language-name label can be
  measured here at all (§ 3), and the third-party resource languages are not
  evidence (§ 3). **The affected content is the whole localization surface** —
  the three routed string images, `crimson.exe`'s runtime language selection,
  and therefore F51 AC04's "for each declared supported original locale" — plus
  every claim that depends on the release being single-language.
- **F12 AC04: id stability across locales.** Measured: one locale, so
  `SingleLocale`, `is_stable() == false`. Not measured: whether a localized
  installation keeps the id numbering while the display text changes. The
  production comparison exists and runs; it needs a second installation.
- **How a localized build is selected.** The engine reads
  `GetSystemDefaultLCID`; *which* file a different system locale would load, and
  whether a localized release ships a differently named string image, is not
  measured and was not guessed.

The owner requirement is therefore a **second localized installation of Crimson
Skies** exposed to this machine as a read-only path (a second `$CS_GAME_DIR`-style
root, e.g. through `CS_LOCALIZED_GAME_DIR`), or owner-supplied reference
material for the release's localized string images (the packaging's own language
list, or the localized `langui.dll`/`language.dll`/`strings.dll` set). With
either, the retail test performs the id-stability comparison unchanged and a
follow-up task can declare the release's locale set from the same production
measurement path. Two other copies of `crimson.exe` exist on this machine
outside `$CS_GAME_DIR` (`/Users/linus/games/crimson-skies-retail/` and a
CrossOver bottle under `~/Library/Application Support/CrossOver/Bottles/`); this
session did **not** read them, because `AGENTS.md` rule 10 permits reading only
`$CS_GAME_DIR` and capabilities are the owner's to grant. Their language content
is unknown; if either is a localized build, the owner can point a capability at
it and the comparison above runs as written.

## Identities and sources

Implementer: `bunny-alpha-2/bunny-alpha-2` (opencode, model
`stealth/space-bunny-alpha`, Rally #467). No independent reviewer is recorded
here; the Rally review claim must supply one and record whether its context was
fresh.

Sources: `$CS_GAME_DIR` read-only (the three routed string images, and all 228
inventoried files for the census, plus a byte-level search of the tree for locale
label strings); `specs/F51-...md` (deliverable, AC03/AC04, non-negotiable
behaviour 1 and 5); `specs/F12-...md` AC04; `docs/contracts/UI-NETWORK.md`;
`docs/contracts/CLI-EVIDENCE.md`; `schemas/evidence.schema.json`; the F51-A,
F51-B, F51-C and F51-D findings; and the F12-A/F12-D findings for the PE
resource tree and the routed images.
