# F12-G: the two non-string resources in `strings.dll`, and the `.H` ↔ `RT_STRING` id correlation

Date: 2026-09-29. Task: #368 / F12-G "Measure the two non-string resources in
strings.dll and the string/header id correlation", a follow-up to F12-B.
Shared contract: `docs/contracts/IDENTITY-CONTENT.md` (its "Required catalog
collections" paragraph is why the two unconsumed resources are a readiness
matter, not a curiosity). Required capability: `retail`, used **read-only**.

**Owner paths used:** `docs/findings/` (this file) and
`docs/findings/evidence/F12-G.json` (the acceptance report; see
[Evidence](#evidence)). No production code changed — the task states "no reader
change, and no guessed parse", and the reader did not need one.

Installation fingerprint, from the production `cs-inspect inventory` run whose
report is the evidence artifact:

| Fingerprint | Value |
| --- | --- |
| `install_sha256` | `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` |
| `content_sha256` | `a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d` |

Nothing from the installation is committed beyond paths, byte spans, sizes,
SHA-256 digests, resource ids, language ids, code pages, counts and structure
descriptions. **No string text and no `#define` name is reproduced in this
file**; where a name would have identified a row, the row is described by its
id and by a name *category* instead.

## The two questions, and what each one turned out to be

1. **What are resource types 16 and 255 in `strings.dll`?**
   Type 16 **is** a `VS_VERSIONINFO` block — established, complete, with every
   one of its 944 bytes accounted for. Type 255 is **four bytes whose meaning
   is not established**; the value they carry is recorded, and so is exactly
   what the installation does *not* settle about it.
2. **Does a `.H` resource id correlate with a `RT_STRING` block id?**
   **Yes, and decisively — but with `langui.dll`, not with `strings.dll`.**
   This corrects the framing of the F12-B unknown: the two `.H` members are the
   resource-compiler headers of the `.rc` scripts that built `langui.dll`, and
   the correlation is a *build-provenance* fact, not a second runtime lookup
   path. The task's hypothesis ("the game resolves its strings through its own
   headers rather than the Win32 resource API") is therefore **not** established
   by the correlation, and the reason it is not is recorded below.

## Method

Everything below was measured with a single read-only walk of the resource
directories and of the two `.H` members. The walk is independent of the
project's own reader on purpose: a correlation that only the production reader
could see would not be evidence about the file, it would be evidence about the
reader. The production reader was used as a **cross-check** (see
[Cross-checks](#cross-checks-against-the-production-code-path)).

- `strings.dll`, `langui.dll` and `language.dll` are read directly from
  `$CS_GAME_DIR`; no byte is written, and nothing inside the installation is
  opened for writing.
- The two `.H` members are exported into `private/` (git-ignored) through the
  **production** `cs-inspect rof --export-dir` path, and the exported bytes'
  digests match the digests the same command reports for the member
  (`61ec2327…` and `5d9c896d…`). The export is a convenience so the probe can
  hash exactly what the production reader decoded; the analysis itself is over
  those bytes.
- Every PE image in the installation is walked for a type-16 or a type-255 leaf,
  which is what turns "one image" into "the installation".
- The probe is `private/evidence/F12-G/measure.py` and its output is
  `private/evidence/F12-G/measure.json`. The probe is an artifact, not a
  deliverable: it lives in the private evidence directory and is not committed.

### What is committed as evidence, and what is not

`docs/findings/evidence/F12-G.json` carries only the report the CLI-EVIDENCE
contract asks for. The **measured numbers live in this document**, because a
reviewer has to be able to re-derive them without the private artifacts, and
because the acceptance criterion of this task is a property of the document.

## Part 1 — resource type 16: a `VS_VERSIONINFO` block

**Container path:** `strings.dll` (installation root).
**Image:** 131 072 bytes, SHA-256
`7582fecaca42d21dd44eb95f896dcb415af790c7057ad81c8e1600ac0b445c21`, PE32,
5 sections, resource directory at RVA `0x12000` / 46 712 bytes, 118
directories and 114 leaves over three types (6, 16, 255).

| Property | Value |
| --- | --- |
| Leaf path | `[type 16, name 1, language 1033]` |
| `IMAGE_RESOURCE_DATA_ENTRY` at | `0x12000` + `0x1598` = file offset `0x13598` |
| Data RVA | `0x1d2c4`; file offset `0x1d2c4` (inside `.rsrc`, whose raw data starts at file `0x12000`) |
| Size | **944 bytes** |
| `CodePage` / `Reserved` | `1252` / `0` |
| SHA-256 of the 944 bytes | `2d9ed5039fedf0cacaa7a9b84732921863ad7e6b7dfec339c8236c4ac5f705cd` |

**Structure established.** The payload is a complete Win32 version resource:
the root node's key is the UTF-16 string `VS_VERSION_INFO`, `wLength` is **944 —
equal to the leaf's declared `Size`**, `wValueLength` is 52 and `wType` is `0`
(binary), which is exactly the length of a `VS_FIXEDFILEINFO`. A member walk
consumes **944 of 944 bytes with 0 trailing bytes** over **17 nodes nested at
most three deep**, and the 52-byte fixed record's `dwSignature` is
`0xFEEF04BD`, the documented magic.

| depth | rel | file offset | `wLength` | `wValueLength` | `wType` | key |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | 0 | `0x1d2c4` | 944 | 52 | 0 | `VS_VERSION_INFO` |
| 1 | 92 | `0x1d320` | 784 | 0 | 1 | `StringFileInfo` |
| 2 | 128 | `0x1d344` | 748 | 0 | 1 | `040904b0` |
| 3 | 152 | `0x1d35c` | 26 | 1 | 1 | `Comments` |
| 3 | 180 | `0x1d378` | 76 | 22 | 1 | `CompanyName` |
| 3 | 256 | `0x1d3c4` | 94 | 27 | 1 | `FileDescription` |
| 3 | 352 | `0x1d424` | 40 | 4 | 1 | `FileVersion` |
| 3 | 392 | `0x1d44c` | 48 | 8 | 1 | `InternalName` |
| 3 | 440 | `0x1d47c` | 114 | 39 | 1 | `LegalCopyright` |
| 3 | 556 | `0x1d4f0` | 42 | 1 | 1 | `LegalTrademarks` |
| 3 | 600 | `0x1d51c` | 64 | 12 | 1 | `OriginalFilename` |
| 3 | 664 | `0x1d55c` | 34 | 1 | 1 | `PrivateBuild` |
| 3 | 700 | `0x1d580` | 80 | 24 | 1 | `ProductName` |
| 3 | 780 | `0x1d5d0` | 58 | 11 | 1 | `ProductVersion` |
| 3 | 840 | `0x1d60c` | 34 | 1 | 1 | `SpecialBuild` |
| 1 | 876 | `0x1d630` | 68 | 0 | 1 | `VarFileInfo` |
| 2 | 908 | `0x1d650` | 36 | 4 | 0 | `Translation` |

Key names and lengths are recorded; the string *values* of the 12 text members
are not, because the acceptance criterion for this task commits structure
descriptions and not content. Their lengths in characters — 22, 27, 4, 8, 39,
12, 24 and 11 for the eight non-empty ones, and four more with a length of 1,
which is an empty string — are recorded because the sizes are part of the
structure.

The 52 fixed bytes, which are numbers and therefore recorded in full:

| field | value | | field | value |
| --- | --- | --- | --- | --- |
| `dwSignature` | `0xFEEF04BD` | | `dwFileFlagsMask` | `0x0000003F` |
| `dwStrucVersion` | `0x00010000` | | `dwFileFlags` | `0x00000000` |
| `dwFileVersionMS` | `0x0001000D` (1.13) | | `dwFileOS` | `0x00040004` |
| `dwFileVersionLS` | `0x00080A93` (8.2707) | | `dwFileType` | `0x00000002` |
| `dwProductVersionMS` | `0x00010000` (1.0) | | `dwFileSubtype` | `0x00000000` |
| `dwProductVersionLS` | `0x00000001` (0.1) | | `dwFileDateMS` / `LS` | `0x00000000` / `0x00000000` |

`Translation`'s 4 bytes are `0x0409` followed by `0x04B0`, i.e. language 1033
and code page 1200.

**Evidence class.** The *structure* is `Documented` — this is Microsoft's own
version-resource layout, and the fixed record's `dwSignature` is its documented
magic. The *specific values* are `ObservedTool` (a read of the installed
bytes). No claim here is `verified_original`: the original program was not run.

**Cross-check inside the installation.** Every type-16 leaf of every PE image
in the installation was walked the same way: **30 leaves across 14 images**
(including `crimson.exe`, `crimson.icd`, `mfc42.dll`, `msvcrt.dll` and
`SETUPENU.DLL`). All 30 root to `VS_VERSION_INFO`, all 30 carry
`dwSignature = 0xFEEF04BD`, and all 30 parse. Sixteen consume their leaf
exactly; the other fourteen leave 4 (four leaves), 8 (seven) or 12 (three)
**zero** bytes of trailing padding, and all fourteen are leaves of
`dsetup32.dll` carrying a non-English language id — that image is the only
one with 17 languages, and its English leaf consumes its resource exactly. So
"a version resource whose declared size may exceed its `wLength` by a small
zero tail" is an observed property of the corpus, and `strings.dll`'s leaf is
in the exactly-consumed group.

**What remains unknown for type 16.** The 12 text values and the *meaning* of
the type for this project: nothing in the workspace consumes a version
resource, and a version resource is a Microsoft build-toolchain artifact, so
whether the original game reads it is not established by the file. One
observable but unresolved lead is recorded in
[the unknowns section](#recorded-unknowns).

## Part 2 — resource type 255: four bytes, meaning not established

**Container path:** `strings.dll` (installation root).

| Property | Value |
| --- | --- |
| Leaf path | `[type 255, name 1, language 1033]` |
| `IMAGE_RESOURCE_DATA_ENTRY` at | `0x12000` + `0x15a8` = file offset `0x135a8` |
| Data RVA | `0x1d674`; file offset `0x1d674` |
| Size | **4 bytes** |
| `CodePage` / `Reserved` | `1252` / `0` |
| SHA-256 of the 4 bytes | `641c2b20cfae89ad63861b5b6a0142bd371f17d9a4002e2983baa7aca9f062a6` |
| Payload | `09 04 00 00` |
| Decodings | `u32` little-endian = **1033**; as two `u16` little-endian = (1033, 0) |

**Structure established:** the leaf's extent is 4 bytes and its whole payload
is recorded above. The payload sits immediately after the type-16 payload
(`0x1d2c4 + 944 = 0x1d674`), and in the two other carriers of the resource the
same adjacency does not hold, so the adjacency is an accident of this image's
layout, not a structure.

**What the installation says about it, and what it does not.** Type 255 is not
one of the `RT_*` types Microsoft assigns. Exactly **three** PE images in the
installation carry a type-255 leaf, and all three are byte-identical:

| File | path | size | `CodePage` | payload | `u32` |
| --- | --- | --- | --- | --- | --- |
| `SETUPENU.DLL` | `[255, 1, 1033]` | 4 | 1252 | `09040000` | 1033 |
| `ebueula.dll` | `[255, 1, 1033]` | 4 | 1252 | `09040000` | 1033 |
| `strings.dll` | `[255, 1, 1033]` | 4 | 1252 | `09040000` | 1033 |

Two of the three are Microsoft packaging binaries that have nothing to do with
the game (`ebueula.dll` is a licence-agreement DLL, `SETUPENU.DLL` is the
installer UI). That is a **measured** fact and it bounds the question: the
resource is a Microsoft build/packaging convention that `strings.dll` inherited,
not a Crimson Skies data structure.

The value `1033` equals the leaf's own third-level language id in all three
carriers, and `strings.dll`'s version resource declares the same `0x0409` in its
`Translation` record. That is a correlation over **three samples whose value is
the same in every sample**, so it cannot distinguish "this field is the
language" from "this field is a constant". One counterexample in the
installation settles part of it anyway: `dsetup32.dll` carries 17 languages
(1028, 1029, 1031, 1033, 1034, 1036, 1040, 1041, 1042, 1043, 1045, 1046, 1049,
1053, 2052, 2058, 3082) and carries **no** type-255 leaf at all, so the resource
is not a per-language marker in this corpus.

**Evidence class.** The bytes, their size, their position and the three-carrier
correlation are `ObservedTool`. The **meaning is `Unknown`**, and it is recorded
as such with the span and the reason, exactly as the task requires: four bytes
at `strings.dll` `0x1d674`, payload `09040000`, and the reason is that no
second sample with a different value exists anywhere in the installation, so no
reading of the field is distinguishable from any other.

**Not guessed:** this finding does not call the payload a LANGID, a version, a
build number or a flag, and no code reads it.

## Part 3 — the `.H` ↔ `RT_STRING` correlation

### The two headers

Both members were exported through the production reader
(`cs-inspect rof --container GOSDATA/ASSETS/crimson.rof --member
ASSETS/SCRIPTS/<name> --export-dir`); the exported digests equal the digests
the same command reports for the member.

| Member | Bytes | SHA-256 | `#define`s | distinct values | min | max | maximal consecutive runs |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `ASSETS/SCRIPTS/RESOURCE.H` | 29 579 | `61ec23270fdf1dc484085db93c936af4e4bb5bb177e3d8a3dd513a6bc4eefb78` | 635 | 612 | 9 | 40 001 | 69 |
| `ASSETS/SCRIPTS/RESRC1.H` | 8 922 | `5d9c896d7532a022a40c1e733eb5d21b7ca85219be4633e23ae69d88cb649c52` | 185 | 173 | 101 | 40 170 | 3 |

The name categories are structural and are recorded as counts only: 351 of
`RESOURCE.H`'s 635 defines and 181 of `RESRC1.H`'s 185 carry a string-table
name (`IDS_` / `SB_` / `STR_`); the rest are font ids, multiplayer-panel
control ids, resource-compiler bookkeeping (`_APS_*`) and a handful of other
prefixes. The two members' header comments name the `.rc` each was generated
for — `LangUI.rc` and `ScrapBook.Rc`.

Every PE image in the installation that has an `RT_STRING` block was scored
against the headers' 782 distinct values (both members, numbering B, "does
this value name a block this image has"):

| Image | `RT_STRING` blocks | header values naming one of them |
| --- | --- | --- |
| `GOSDATA/ASSETS/BINARIES/langui.dll` | 101 | **775 / 782** |
| `SETUPENU.DLL` | 37 | 169 |
| `strings.dll` | 112 | 123 |
| `ebueula.dll` | 8 | 28 |
| `crimson.icd` | 2 | 15 |
| `clokspl.exe` | 19 | 14 |
| `UNINSTAL.EXE` | 17 | 12 |
| `dsetup32.dll` | 7 | 10 |
| `GOSDATA/ASSETS/BINARIES/language.dll` | 3 | 2 |
| `mcp.dll`, `mfc42.dll` | 5, 43 | 0 |

`langui.dll` is the image the two headers describe; the next highest is 169,
and the gap to 775 is what makes that a measurement rather than an inference
from the file names.

### What the string-table ids cover

Restricting to the 532 string-table names (351 + 181):

| Header | string ids | span | maximal consecutive runs |
| --- | --- | --- | --- |
| `RESOURCE.H` | 351 defines, 346 distinct | 100 … 30 002 | 65 |
| `RESRC1.H` | 181 defines, 171 distinct | 40 000 … 40 170 | **1** (fully contiguous) |

`RESRC1.H`'s single run of 171 contiguous ids is the sharpest object in the
whole measurement, and the next two sections are built on it.

### The three PE string tables

| Image | `RT_STRING` blocks | block id range | counted units | non-empty units | smallest / largest documented string id |
| --- | --- | --- | --- | --- | --- |
| `langui.dll` | 101 | 1 … 2 511 (not contiguous) | 1 616 | 1 247 | 0 … 40 175 |
| `strings.dll` | 112 | 7 … 1 072 (not contiguous) | 1 792 | 1 023 | 96 … 17 151 |
| `language.dll` | 3 | 1 … 3 (contiguous) | 48 | 36 | 0 … 47 |

### The correlation, and which of two numberings it selects

A `.H` value `v` is a *string* id; the block it lives in follows from the
numbering. Two numberings are in play in this repository:

- **A** — `block = v / 16` (integer division), i.e. `id = block * 16 + index`;
- **B** — `block = v / 16 + 1`, i.e. `id = (block - 1) * 16 + index`, which is
  exactly what `cs_formats::string_id` implements and what the F12-B finding
  labels `Documented`.

For each numbering, how many of a header's values name a block the image
actually has, in two scopes — every define, and only the 532 string-table
names:

| Image | blocks | A, all 820 defines | A, 532 string names | B, all 820 defines | B, 532 string names |
| --- | --- | --- | --- | --- | --- |
| `langui.dll` | 101 | 705 | 463 | **813** | **525** |
| `strings.dll` | 112 | 105 | 72 | 128 | 78 |
| `language.dll` | 3 | 0 | 0 | 2 | 0 |

B beats A on every row, and the margin is enormous on `langui.dll` and
negligible-to-none on the other two images.

**No permutation test backs this, and none is reported.** The score is a sum of
a per-value predicate over the headers' value multiset, so it is *invariant*
under any permutation of that multiset: every draw of a shuffle of the 820
values returns the observed count (705 for A, 813 for B on `langui.dll`).
A test that can only return the number it is testing carries no information,
which is why there is no `p` value here. (A resampling null, drawing 820 fresh
values from the observed id range instead of shuffling the real ones, does
separate the two — 20 000 draws never exceed 59 — but that is not what decides
this either, because the deciding measurement below is deterministic and has
no sampling step at all.) The decisive evidence is the boundary test.

### The boundary test: `RESRC1.H`'s contiguous run, exactly

`RESRC1.H` names ids **40 000 … 40 170**, contiguous. Under B those ids are
blocks **2 501 … 2 511**; under A they are blocks **2 500 … 2 510**.

| | blocks the run requires | present in `langui.dll`? |
| --- | --- | --- |
| A (`block = v/16`) | 2 500 … 2 510 | **no — block 2 500 does not exist** |
| B (`block = v/16 + 1`) | 2 501 … 2 511 | **yes — all eleven, and 2 511 is `langui.dll`'s highest block** |

Each of those eleven blocks holds exactly 16 counted units, so the eleven
blocks cover ids 40 000 … 40 175, and the header names 171 of those 176. The
five ids the header does *not* name — 40 171 … 40 175 — are **precisely the
five units of the last block that are empty**, and every other empty unit in
those eleven blocks is one the header *does* name. The header's name set and
the image's empty-unit set agree on the tail exactly and are disjoint on the
interior, which is what a resource compiler's output and the `.rc` that
produced it look like.

The same asymmetry appears in `RESOURCE.H` independently: its largest value,
40 001, is a block that exists under B (2 501) and does not exist under A
(2 500). Its 65 consecutive string-id runs are wholly inside `langui.dll`'s
block set **58 times under B and 43 times under A**, and B's 7 failures are
each a *single*-id run, so B's misses are ids with no block at all rather than
runs that straddle a boundary.

This is measured evidence bearing on task #374 (which of the two numberings the
project intends). It is **not** a resolution of #374, which owns the code and
the `Documented` claim; a note with these numbers is attached there. What is
established here is narrower and is about the installation: the `.H` ids and
`langui.dll`'s blocks are the *same* numbering, and that numbering is
`block = v/16 + 1`.

### What the correlation establishes — and what it does not

**Established (`ObservedTool`):** the two `.H` members and `langui.dll`'s
`RT_STRING` tree describe the same ids under one numbering. `RESOURCE.H`'s and
`RESRC1.H`'s own comments name the `.rc` scripts they were generated for, and
the ids they name are the ids the resource compiler wrote into `langui.dll`.
The headers are therefore the **build-time source** of the resource tree, not a
second copy of it that a runtime could consult.

**Not established, and specifically not the task's hypothesis.** The task
framed the correlation as something that "would establish that the game
resolves its strings through its own headers rather than the Win32 resource
API". The measurement does not establish that, and the reason is structural: a
`.H` file is a *pre-compilation* artifact. It is in the installation inside
`crimson.rof` as data, but the runtime artifact is the compiled resource tree
in `langui.dll`. The correlation proves the two numberings are one thing seen
before and after compilation; it says nothing about which of them a running
game reads, and there is no path by which a compiled game would read a `.H`.

The two readings remain open and are recorded as such:

- **Whether the original engine calls the Win32 resource API.** `crimson.icd`'s
  import directory holds 21 module descriptors; 19 of them list 461 named
  symbols between them, and none of those 461 is `LoadString`,
  `FindResource`, `LoadResource`, `LockResource` or `SizeofResource`. The
  remaining two — `KERNEL32.dll` and `USER32.dll` — both name a thunk array
  whose **first entry is zero**, so those two modules list no symbols in the
  file at all. The executable is documented elsewhere as protected, and this
  task did not unpack, decrypt or decompile anything. So the import table is
  silent about exactly the two modules that would answer the question, and it
  is recorded as a lead, not as evidence either way.
  `crimson.exe` — the launcher — *does* import `VERSION.dll`
  (`GetFileVersionInfoA`, `VerQueryValueA`, `GetFileVersionInfoSizeA`), which
  is the documented consumer of the type-16 structure. That does not
  establish that anything reads *this* image's type-16 leaf; `GetFileVersionInfoA`
  is also how a program reads its own version block.
- **Whether `strings.dll` is reached at all.** `strings.dll` is the only image
  in the group that carries types 16 and 255, and it exports exactly one
  symbol and imports only `KERNEL32.dll` — so it does not import the string
  resource API. Whether the engine loads it, and by which entry point, is not
  measured here.

## Cross-checks against the production code path

The numbers above come from an independent walk. They were checked against the
project's own readers, which is what makes them evidence about the file rather
than about the probe:

| Production path | Result | Agrees with this finding |
| --- | --- | --- |
| `cs-inspect config --file "$CS_GAME_DIR/strings.dll" --cs-path "$CS_GAME_DIR"` | `pe_resources.accounting = {"strings":1792,"undecodable":0,"other_leaves":2,"duplicate_ids":0}` | yes — the two unconsumed leaves are type 16 and type 255, and 1 792 is `strings.dll`'s counted `RT_STRING` units |
| `cs-inspect rof --container GOSDATA/ASSETS/crimson.rof --member ASSETS/SCRIPTS/RESOURCE.H` | 29 579 bytes decoded, SHA-256 `61ec2327…` | yes |
| `cs-inspect rof --container GOSDATA/ASSETS/crimson.rof --member ASSETS/SCRIPTS/RESRC1.H` | 8 922 bytes decoded, SHA-256 `5d9c896d…` | yes |
| `cs-inspect inventory --cs-path "$CS_GAME_DIR"` | the two fingerprints at the top of this file | yes |

`other_leaves: 2` is the F12-B reader's own accounting of "leaves that are not
three-level `RT_STRING` leaves". It is the gap this task was filed to close:
the reader counted the two resources, and this finding is what they are.

## Recorded unknowns

Each names the affected content and what would resolve it, per the owner
directive on follow-up limitations.

1. **The meaning of the four bytes of type 255.** Affected content: one
   resource in each of three images, one of which (`strings.dll`) is
   gameplay-relevant. Resolved by: nothing in the workspace can resolve it; a
   second, differently-valued sample would, and no localized or
   differently-built image is available. Until then the span stays recorded and
   unread, and a readiness claim must name it. Affected releases: any
   `verified_original` claim about `strings.dll`'s resource tree.
2. **Which resource API the original engine uses** for the `RT_STRING` tree.
   Affected content: every UI/dialogue string (all 532 `.H`-named ids and the
   rest of `langui.dll`'s 1 616 units). Resolved by: an original-run capture
   or a file-access trace from the owner — `#341`-style evidence, which agents
   cannot produce. Not resolved by this task, and the import-table silence of
   `crimson.icd` recorded above is why.
3. **Whether anything reads `strings.dll`'s type-16 leaf.** Affected content:
   that one resource. Resolved by: the same owner-supplied original-run
   evidence. The structure itself is established; only its consumption is open.
4. **The `Documented` claim on `string_id`.** Affected content: every id this
   project reports for a `RT_STRING` unit. This task's boundary test is
   evidence for the `(block - 1) * 16 + index` form and against the
   `block * 16 + index` form, on the installation, from the `.rc`/compiled
   pair. It does not settle which numbering the project should report, and it
   does not verify the Microsoft documentation, which this task did not obtain.
   Resolved by: #374, which owns `crates/cs_formats/src/pe_resources.rs` and
   the `Documented` claim; a note with these numbers is attached to it.
5. **Whether a localized installation keeps these ids.** Only one English
   installation was available, so the correlation and the two non-string
   resources are single-installation observations. F12-D AC04 stays open.
6. **Seven string-table ids in `RESOURCE.H` name no `langui.dll` block**
   (values 600, 620, 2 002, 2 050, 2 054, 3 510, 3 540, each a single-id run).
   One of them, 2 002, lands on a block `strings.dll` does have and
   `langui.dll` does not. Affected content: those seven ids. Not guessed:
   whether they are dead defines, ids for a different image, or evidence that
   the two headers do not belong to one `.rc` set. Recorded, unresolved.
7. **Eighteen of `langui.dll`'s 101 blocks are addressed by neither header**
   (2, 3, 4, 5, 190, 195, 196, 197, 198, 200, 201, 202, 204, 205, 206, 217,
   219, 227). A block can be reached by a name the headers do not carry, by
   a resource the `.rc` referenced but did not define, or by a second `.rc`
   not in the installation. Affected content: those blocks' 288 units.
   Recorded, unresolved; the F12-D accounting pass is where it belongs.

## Tests

**There is no `accept_f12_g_` test, and this is stated rather than worked
around.** The task's owner paths are `docs/findings/` alone and it says
explicitly "This is a measurement task: no reader change"; a test that pinned
this measurement would have to live in a crate and would call a reader the task
forbids changing. Adding a Rust test outside the owner path, or a test that
merely re-reads this document, would be exactly the shortcut the project rules
forbid, so neither was done.

What *is* pinned, and how a reviewer re-derives the numbers here:

- the full workspace test suite is green on this commit (see
  [Commands](#commands)), and the two production readers whose output this
  finding cross-checks against (`accept_f12_b_retail_pe_resource_structure_matches_the_survey`
  and `accept_f12_b_retail_resource_headers_read_within_the_id_space`) are part
  of it and run with `--include-ignored`;
- the measurement itself is a script, not prose: it was run twice in a row and
  the two runs produced byte-identical JSON
  (`sha256 ece7c8d379ff1f6904cd6f46b78f29a6e10fce5b5c9b5f4c1edc1c117aa6c8ef`),
  and that digest is the `measure.json` artifact of the committed evidence
  report, so the probe's entire output is pinned there;
- every number in this document is either a hash, a byte span, a size, a count,
  an id, a key name or a structure field — never original text and never a
  `#define` name.

**What is not claimed:** the four production commands in
[Cross-checks](#cross-checks-against-the-production-code-path) cover the
installation fingerprint, the two header digests and lengths, `strings.dll`'s
`RT_STRING` unit count and its `other_leaves: 2`. They do **not** produce the
per-image scoring table, the block id lists, the boundary test, the
empty-unit agreement, the unaddressed-block list or the import-table
measurements. Those come from the probe, which is an artifact in the private
evidence directory and is not committed, so re-deriving them needs that script
(or an equivalent walk) and not the four commands alone.

A follow-up should promote this measurement to a `#[ignore = "requires
CS_GAME_DIR"]` regression test once a crate owner path is granted for it; until
then the finding is documentation-grade, and the level a merge can award is
`checked`.

## Evidence

This task used `retail` (read-only access to the original installation), so
`docs/contracts/CLI-EVIDENCE.md` requires an acceptance report. It is
`docs/findings/evidence/F12-G.json`, generated on this tree by
`private/evidence/F12-G/harness.py` from real runs: the full workspace test
suite, the production `cs-inspect inventory` fingerprint, the two production
`cs-inspect rof` decodes, and the production `cs-inspect config` read of
`strings.dll`. The report carries digests, counts, lengths and exit codes only;
the artifacts stay in `private/evidence/F12-G/`.

It is validated with `tools/validate_evidence.py` **without** `--require-pass`,
for the reason the F12-H report gives and that reason still applies here: the
flag rejects a report that lists unresolved issues, and this task's
deliverable is largely that seven limitations stay recorded with the content
they affect. Deleting the `unknowns` to turn the flag green would be the thing
the contract forbids. The report's `claim` is `implemented`: a merge awards
`checked` at most, and nothing here observed the original game running.

## Commands

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 (734 passed, 0 failed, 62 ignored over 101 test binaries) |
| `cargo test --workspace --locked -- accept_f12_b_ --include-ignored` | 0 (16 tests, 16 passed, 0 failed — includes the two retail readers this finding cross-checks against) |
| `cargo test --workspace --locked -- accept_f12_g_ --include-ignored` | **0 tests** — see [Tests](#tests) |
| `python3 tools/validate_evidence.py private/evidence/F12-G/acceptance.json --artifact-root private/evidence/F12-G` | 0 (`structurally_valid: true`, 8 artifacts) |

## What is not claimed

- No behaviour of the original game is claimed or implied. The original
  executable was not run; `retail` here means read access to the installation's
  files.
- The type-255 payload is not called a LANGID, a version or a flag.
- The correlation is not presented as evidence that the engine reads its `.H`
  files, because a `.H` file is a pre-compilation artifact and cannot be a
  runtime lookup path.
- This finding does not change `string_id`, the `Documented` claim, the dialect
  inventory or the catalog. #374 owns the numbering; F12-D owns the accounting.

**Identities.** Implementer: `bunny-2` (Space Bunny Free). Reviewer: `bunny-2`
again, in a fresh session — the same agent instance, so **this review is not
independent evidence** and nothing here may be read as an original-reference
confirmation. See [Review](#review) for what it did and did not establish.

## Review

Reviewer: `bunny-2` in a fresh session (same agent instance as the
implementation, so not independent). The review re-derived the measurement
with a PE/COFF resource walk and a string-table walk written from the
Microsoft layouts, independently of the probe, and compared the result with
this document number by number. Every image digest, byte size, offset, member
span, id range, unit count, non-empty count, per-image score, boundary-test
fact, empty-unit agreement, unaddressed-block list and import-table count in
this file reproduced exactly; the implementer's probe was also re-run and
produced byte-identical output (same `measure.json` digest).

Two claims were wrong as written and were corrected here:

- the 20 000-draw permutation test reported in
  [the correlation section](#the-correlation-and-which-of-two-numberings-it-selects)
  cannot produce the `p = 0/20 000` it claimed: the score is invariant under a
  permutation of the headers' value multiset, so every draw returns the
  observed count. That paragraph now states the invariance instead of a
  significance value, and the measurement itself is untouched.
- the claim that every number here can be recomputed "from the commands …
  without the private artifacts" overstated what the four production commands
  emit; [Tests](#tests) now says which measurements they cover and which come
  from the probe.

No production code was touched, and no measured value was changed.

**Still outstanding, and the only reason this branch is not merged:**
`accept_f12_g_` resolves to zero tests. `AGENTS.md` rule 6 and
`docs/contracts/CLI-EVIDENCE.md` both require the task prefix to resolve to at
least one real test, and this task's owner path is `docs/findings/` alone, so
no agent can add one inside its scope — the natural home,
`crates/cs_formats/src/pe_resources.rs`, is #374's. F12-K / #377 already spells
out the test to write, with the exact assertions. Resolving that needs the
owner: grant a test owner path, or rule that a `docs/findings/`-only
measurement task is exempt from the prefix rule.

## Sources

`$CS_GAME_DIR` read-only (the PE images, and `ASSETS/SCRIPTS/RESOURCE.H` and
`ASSETS/SCRIPTS/RESRC1.H` decoded through the production `cs-inspect rof`
reader); `specs/F12-text-configuration-strings-and-pe-resources.md`
(non-negotiable #1 "extract grammar from real samples before implementing a
parser", and #5 "unknown configuration keys are retained and counted");
`docs/contracts/IDENTITY-CONTENT.md` ("Required catalog collections": a catalog
element cannot be excluded, and an opaque unparsed member is still an inventory
row); `docs/contracts/CLI-EVIDENCE.md`; the F12-A, F12-B and F12-D.langui
findings; `crates/cs_formats/src/pe_resources.rs` (`string_id`, `RT_STRING`,
the `other_leaves` accounting) and `crates/cs_formats/src/text/resource_header.rs`;
the Microsoft PE/COFF resource-directory layout and the Win32 version-resource
layout (`Documented`); and Microsoft's Win32 API reference page for
`LoadStringA`, which this task read and which does **not** itself state the
block-index formula — that part of the `Documented` claim is left to #374.
