# F12-D: the localized-installation acceptance case has no data on this machine

**Task:** #48 `F12-D` (stage D of
`specs/F12-text-configuration-strings-and-pe-resources.md`).
**Date:** 2026-10-02. **Agent:** bunny-alpha-2. **Status:** blocked, with the
evidence that blocks it and the exact owner input that unblocks it.
**Capabilities used:** `retail` (read-only access to `$CS_GAME_DIR`). No original
executable was run, so nothing here raises any claim above `ObservedTool`.

## The claim under test

Stage D's minimum acceptance scenario is one line of the sheet:

> **AC04:** A localized installation preserves stable ids while changing display
> text.

It is an assertion about **two** text sets: an id set and the display text that
belongs to it in a *second*, differently localized installation. It cannot be
observed inside a single installation, because inside one installation every
`(id, language)` pair is unique — there is nothing for a translation to differ
from.

## What this machine has

`CS_CAPABILITIES=retail,gpu,audio`; `CS_GAME_DIR` is one English installation
(`install_sha256 b4e780ab84cf31d8…`, the value the project's own
`cs-inspect inventory` records in the committed evidence reports). Every routed
string image was read through the production path,
`cs-inspect config --cs-path "$CS_GAME_DIR" --file <image>`:

| image | sha256 | `RT_STRING` blocks | strings | undecodable | duplicate ids | languages |
| --- | --- | --- | --- | --- | --- | --- |
| `strings.dll` | `7582fecaca42d21d…` | 112 | 1792 | 0 | 0 | 1033 |
| `GOSDATA/ASSETS/BINARIES/langui.dll` | `357e6bb05f1d2872a…` | 101 | 1616 | 0 | 0 | 1033 |
| `GOSDATA/ASSETS/BINARIES/language.dll` | `2d7c904bb2d7b14a…` | 3 | 48 | 0 | 0 | 1033 |

3456 strings, one language, no duplicate `(id, language)` pair in any image:
there is nothing inside this installation for AC04 to compare.

To rule out a second localization hiding in a file the three routed rules do not
name, every PE image in the installation was walked with a read-only
PE/COFF resource-directory reader (DOS `e_lfanew` -> COFF -> optional header
data directory 2 -> the three resource levels). 228 files, 18 PE images with a
resource directory, 11 of them carrying `RT_STRING`:

| image | `RT_STRING` leaves | language ids | whose text |
| --- | --- | --- | --- |
| `strings.dll` | 112 | 1033 | the game |
| `GOSDATA/ASSETS/BINARIES/langui.dll` | 101 | 1033 | the game |
| `GOSDATA/ASSETS/BINARIES/language.dll` | 3 | 1033 | the game |
| `crimson.icd` | 2 | 1033 | the game (DirectX ICD, not text resources) |
| `SETUPENU.DLL` | 37 | 1033 | Microsoft setup |
| `UNINSTAL.EXE` | 17 | 1033 | Microsoft uninstaller |
| `clokspl.exe` | 19 | 1033 | Microsoft clock control |
| `ebueula.dll` | 8 | 1033 | Microsoft EBU EULA |
| `mfc42.dll` | 43 | 1033 | Microsoft C++ runtime |
| `dsetup32.dll` | 119 | 1028, 1029, 1031, 1033, 1034, 1036, 1040, 1041, 1042, 1043, 1045, 1046, 1049, 1053, 2052, 2058, 3082 | Microsoft DirectSetup |
| `mcp.dll` | 5 | 0 (neutral) | Microsoft Borland runtime |

**No installation member carries a game string in any language other than
1033.** The only multi-language string tables belong to Microsoft's own setup
and runtime binaries, which is exactly what task #467
(`F51-LOCALE-SET`, blocked) measured with the production census, and what this
independent walk reproduces. The resource types present across the installation
are 1, 2, 3, 4, 5, 6, 9, 10, 12, 14, 16, 240 and 255; no other type carries
localizable text.

A second, independent check over the same installation found no language-named
data at all: the only occurrences of `English`, `French`, `German` and `Spanish`
as data strings are in `msvcrt.dll`, `msvcp60.dll`, `dplayerx.dll`,
`dsetup32.dll`, `clokspl.exe` and `mcp.dll` (C runtime locale tables and
DirectSetup's language list). `crimson.icd` names `Assets\Binaries\Language.dll`
and carries one `English` string of the same MSVC provenance. Nothing in the
installation declares a release locale set, and nothing in it is a second
localization.

## Why this blocks the stage rather than narrowing it

1. **AC04 cannot be produced from this data.** Any report that called it
   satisfied would be comparing the installation with itself. Writing a
   synthetic second image (renumbering a language id, re-encoding text) would
   test this engine against this engine and is explicitly excluded: the sheet
   says synthetic fixtures alone cannot certify original-data behaviour, and a
   developer placeholder never satisfies a retail acceptance case.
2. **The evidence report could not honestly pass.** `--require-pass` rejects a
   report whose `unknowns` is non-empty. With AC04 open the report must carry
   it, so stage D's own "Done when" (a report that passes
   `tools/validate_evidence.py --require-pass`) is unreachable while the data is
   missing. Emptying `unknowns` to make the flag pass is the one thing the
   owner directive forbids.
3. **The rest of stage D is already measured or is a separate slice.** The
   accounting of the two keyed-list members is pinned by merged work:
   `ASSETS/LAYOUT.CSV` 822 entries and `ASSETS/SCRAPBOOK.CSV` 461 sixteen-field
   items with 13 typed positions each, in #371 (`F12-I`); the reading rules R1-R4
   in #351; placeholder expansion in #370 (`F12-E`); the id correlation between
   the `.rc` headers and `langui.dll` in #368 (`F12-G`) and #377 (`F12-K`). What
   stage D adds on top is a whole-installation account of *referenced* fields
   and ids — a production feature that does not exist yet, and that cannot be
   validated end to end while AC04 is open. It is filed as its own task below
   rather than smuggled into this one.

## What the owner must provide (either one unblocks the stage)

* **A second, localized Crimson Skies installation** exposed to this machine as
  a read-only root — the retail comparison test in #467 already reads
  `CS_LOCALIZED_GAME_DIR`, so with a localized tree the comparison runs through
  the production path with no code change. Its `strings.dll`,
  `GOSDATA/ASSETS/BINARIES/langui.dll` and `language.dll` are the three images
  this stage reads.
* **Or owner-supplied reference material** for a localized string set: the ids
  and their display text for at least one non-English locale, as the owner's own
  reference evidence. That is enough to answer AC04's id-stability question but
  not enough to certify a release locale set, which needs the packaging.

`human_play` and `human_review` are not needed for AC04 as written: the claim is
about what a second installation *contains*, not about how the running game
behaves. That is why this stage is blocked on data and not on the owner
playing the game.

## Recorded unknowns (kept, not closed)

* **U1** Whether a localized `langui.dll` / `strings.dll` keeps the block ids,
  the language ids and the code pages measured here. Unresolvable from one
  installation. Gates F12-D's AC04, the release's supported-locale set
  (#467), and the title confirmations that follow the campaign inventory.
* **U2** Which `RT_STRING` numbering the original addresses strings with:
  `cs_formats::string_id`'s `(block - 1) * 16 + index` versus the
  `name * 16 + index` rule the #374 task assumed. #368 measured 775 of 782
  header values naming blocks `langui.dll` actually has under the engine's
  numbering and 705 under the other, with a deterministic boundary case (block
  2500 absent, blocks 2501..2511 present). Open in #374; AC04 compares id
  *sets*, so this task must not assume either answer.
* **U3** Whether the game reaches these strings through the Win32 resource API,
  through its own `RESOURCE.H`/`RESRC1.H`, or through both. Recorded on the
  `pe.resources` inventory row since F12-A; unchanged.

## Reproducing the census

Read-only, from `$CS_GAME_DIR`; nothing was written into the installation and no
original bytes are committed here.

```sh
for f in strings.dll GOSDATA/ASSETS/BINARIES/langui.dll GOSDATA/ASSETS/BINARIES/language.dll; do
  cs-inspect config --cs-path "$CS_GAME_DIR" --file "$CS_GAME_DIR/$f" \
      --out "private/evidence/F12-D/$(basename "$f").json"
done
```

The resource-directory walk is described in prose above rather than committed as
a script, because the production reader (`cs_formats::read_pe_resources`) is the
one that must own this measurement; this walk only established that no second
locale exists anywhere in the installation, which is a negative result about
*presence* and not a claim about any reader's correctness. The three images were
additionally read through the production reader (`cs-inspect config`), whose
per-image accounting is the table above.