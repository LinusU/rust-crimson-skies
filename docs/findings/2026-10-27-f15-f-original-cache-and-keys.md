# F15-F: what the original engine cached, and how it keyed it

Date: 2026-10-27. Task #403 `F15-F`, the follow-up recorded in
`docs/findings/2026-09-30-t64-f15-d-cold-warm-restart.md` ("What this stage does
**not** establish", second bullet). Stage F15 of
`specs/F15-asynchronous-asset-loading-and-private-cache.md`; the shared contract
that constrains the *project's* cache is `docs/contracts/IDENTITY-CONTENT.md`.

Capabilities used: `retail` — read access to the original installation at
`$CS_GAME_DIR` and to the owner-supplied decrypted executable named by
`$CS_ENGINE_IMAGE`, both treated read-only. The original executable was **not**
run; no `gpu`, `audio`, `human_play` or `human_review` claim is made or needed.
Every address below is a virtual address in the measured image; every string is
quoted only as evidence, never as code, and no disassembly, decompiled code,
executable bytes or original media is committed.

Probe: `tools/engine_file_io_probe.py` (read-only, standard library only).
Acceptance tests: `tools/tests/test_engine_file_io_probe.py`
(`accept_f15_f_` selection). Evidence report:
`docs/findings/evidence/F15-F.json`. All three are reproduced in
"Reproduction" below.

## The answer

1. **No persistent derived-asset store or cache was observed anywhere in the
   engine's file-creation surface.** The word "cache" occurs in exactly two
   strings in the whole 2,580,480-byte image, and both are sound-residency
   vocabulary. The engine imports no memory-mapping entry point, its one Win32
   `CreateDirectoryA` call is a writability probe in `%TEMP%` (the three
   `_mkdir` calls create only the user's `Planes` and `SavedGames\<profile>`
   trees and back a script `mkdir` command), and every file it opens for
   writing is user state, a debug dump or screenshot, an editor's text data, or
   part of the compiled-asset toolchain — never a decoded texture, sound or
   script keyed by content identity.
2. **The original does derive assets, but before shipping, not at load.** The
   retail reader container holds *compiled* reader node trees (tagged nodes:
   string tag `3`, list tag `4`), the loader reads exactly that node form, and
   the loader carries a source-versus-compiled timestamp check whose diagnostic
   names a "compiled version". The compiler that emits that form is linked into
   the same executable (`zipper\gamez\zclass\cls_zbd.c`,
   `zipper\gamez\zgamegen\gg_read.c`); at run time it is reached only through the
   `GameZWriteZBDFile` script command (`0x5b973a`), not from the resource load
   path, and no read-back of what it writes was observed.
3. **The one cache the original does implement that this measurement observed is
   in-memory sound residency, keyed by the sound's definition, not by content
   identity.** Retail sound sets carry a per-sound `PURGEABLE` child node
   (2,633 of them in `ZBD/zrdr.zbd` against 2,766 `.wav` references), and the
   sound-set parser turns its presence into bit 6 of the definition's flag word.
   The engine's own vocabulary also distinguishes `st_cached` from `st_streamed`.
   What the bit changes at run time is **not** established here.
4. **F15's cache layout is therefore a free design choice, not constrained by the
   original.** Nothing in F15's acceptance criteria (AC01–AC04) changes under
   either answer: they are internal consistency properties of *our* loader
   ("a late completion must not attach an old texture", "a killed write must not
   lose content", "warm equals cold"). AC04 is an equality between our own cold
   and warm loads, not an equality with an original layout. What the answer does
   bound is the claim *level*: no F15 stage may cite an original cache layout it
   has not measured, and this finding supplies no such layout.

The rest of this document is the evidence for those four sentences, including
the negative's boundary.

## Inputs

| Input | Value |
| --- | --- |
| `$CS_ENGINE_IMAGE` | `crimson.decrypted.exe`, 2,580,480 bytes, SHA-256 `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` |
| PE | PE32, i386, image base `0x400000`; sections `.text 0x1000`, `.rdata 0x203000`, `.data 0x219000`, `.idata 0x61f000`, `.rsrc 0x623000`, `.reloc 0x629000` |
| Retail container read | `$CS_GAME_DIR/ZBD/zrdr.zbd`, 3,988,394 bytes (read-only; the installation/content fingerprints are recorded in `docs/findings/evidence/F15-F.json`) |
| Linked DLLs relevant to file IO | `msvcrt.dll`, `kernel32.dll`, `shell32.dll`; the codec libraries the image names are read-only in what it imports — `zTiff.dll` exposes only `TIFFOpen`/`TIFFGetField`/`TIFFReadEncodedStrip`/… (no `TIFFWrite*`), `avifil32.dll` only `AVIStreamOpenFromFileA`/`AVIStreamRead`/…; `assets\binaries\roffile.dll` `0x61f4f4` and `assets\binaries\ijl10.dll` `0x60b57c` are loaded with `LoadLibraryA` |

The probe refuses to analyse anything whose SHA-256 is not the value above, so
this document cannot silently describe a different binary.

## What was searched

The search is an enumeration, not a keyword scan: it resolves the image's import
table and then **every** reference to every file-creating or file-writing entry
point the image actually imports. References to each IAT slot are found in the
`.text` bytes in three shapes — a direct `call dword ptr [slot]`, a `jmp` through
the slot (tail call), and a register load (`mov r32, [slot]`) that later calls the
register — and the containing function, plus the nearest preceding `push imm32`
naming a string, is reported for each. The pattern matcher is pinned by a
synthetic PE fixture in the acceptance test, so a change that silently stops
recognising one shape fails the test.

| Entry point | References | Sites |
| --- | --- | --- |
| `CreateFileA` | 14 | `0x406916`, `0x406c84`, `0x4a93dd`, `0x533fdb`, `0x5343bb`, `0x579d48`, `0x579e28`, `0x579eba`, `0x59d95b`, `0x59da0b`, `0x5a9a8d`, `0x5bfe8d`, `0x5bff3e`, `0x5d015d` |
| `WriteFile` | 6 | `0x406934`, `0x533ff9` (loaded), `0x59dba6` (loaded), `0x59dd6e`, `0x5a9aab` (loaded), `0x5bff5e` |
| `fopen` | 26 | of which the write modes are listed below |
| `fwrite` | 45 | the archive, model, material, texture-directory and animation-record writers |
| `CreateDirectoryA` | 1 | `0x406bf8` — the `%TEMP%\~RCSA~` probe only |
| `_mkdir` | 3 | `0x41a7d8` (`Planes`), `0x46b206` (`SavedGames\<profile>`), `0x5b7cc7` (the script `mkdir` command) |
| `GetTempPathA` | 3 | `0x406bdf` (writability probe), `0x59c9bc`, `0x59cab1` (`gamez.err` / `gamez.out` fallback) |
| `MoveFileA` | 1 | `0x4073a7` — screenshot rename under the configured pictures directory |
| `DeleteFileA` | 5 | `0x406965`, `0x406ca1` (probe cleanup), `0x407394`, `0x4073b4` (screenshot temp), `0x5b8e87` (the script `DeleteFile` command) |
| `RemoveDirectoryA` | 2 | `0x406c22` (probe cleanup), `0x46b3fc` (saved-game cleanup) |
| `tmpfile` | 2 | `0x5c0650` (jump), `0x5c06e1` — the ZAR writer's scratch file |
| `freopen` | 1 | `0x59c98f` — `stderr` redirected to `<name>.err` |
| `remove` / `rename` | 3 / 1 | saved game `%s\%s.sav`, saved-game trees, the `location.zrd` rewrite |
| `_chmod` / `SetFileAttributesA` | 1 / 4 | attribute fix-ups on the same paths |
| `_rmtmp` | 2 | End of the ZAR writer |

The image imports **none** of `CreateFileMappingA`, `CreateFileMappingW` or
`MapViewOfFile`, and none of `CreateFileW`, `CopyFileA`, `_open`, `_creat`,
`_write`, `fputs`, `fputc`, `SetEndOfFile`. There is therefore no memory-mapped
or CRT-handle path by which a derived store could be created that the audit above
does not cover.

A source scan adds one more, independent negative: case-insensitively, the
substring "cache" occurs exactly twice in the image: at `0x60c3fc` inside
`"Expected st_cached or st_streamed"` and at `0x60d3cb` in `"st_cached"`. There
is no cache directory name, no cache file extension and no cache configuration
key in the string table at all.

## What the engine writes

Grouping the references above by the function that owns them gives the complete
writer set. Each row is a function, not a guess from a file name.

| Writer (function) | Entry points inside it | Path or mode evidence | Purpose |
| --- | --- | --- | --- |
| `0x46bce0` (write site `0x46bda4`) | `WriteFile` (via `0x5bff10`) | `"SavedGames\%s\%s"` `0x61f7e0`, `"Status.dat"` `0x6270d0` | save-game / persistence files |
| `0x46b2f0`, `0x41a9e0` | `RemoveDirectoryA`, `RemoveDirectoryA`/`remove` | `"SavedGames\%s"`, `"%s\%s.sav"` `0x61fa58` | saved-game deletion |
| `0x41a7b0` | `fopen` `0x41a7e8` | `"%s"` under `"Planes\"`, mode `"wb+"` | a user-saved plane |
| `0x533fc0` | `CreateFileA` `0x533fdb`, `WriteFile` `0x533ff9` | `"%s.bmp"` `0x6353b0`, `"Save Captured Image"` `0x6353b8`; writes `BM` (`0x4d42`) + a 40-byte `BITMAPINFOHEADER` | screenshot as BMP |
| `0x5a9a60` | `CreateFileA` `0x5a9a8d`, `WriteFile` `0x5a9aab` | PNG chunks (`IHDR`/`IEND`) | screenshot as PNG |
| `0x4072a0` | `SetFileAttributesA`, `DeleteFileA`, `MoveFileA` | `"%1!s!\Snap_%2!d!_..."` `0x61f0a4` and `"%1!d!.png"` `0x61f0c4`; directory comes from `SOFTWARE\Microsoft\Microsoft Games\Crimson Skies\1.0` | move a captured image to the user's pictures folder |
| `0x59c920` | `freopen` `0x59c98f`, `GetTempPathA` | `".err"` / `".out"` appended to the caller's name, falling back to `%TEMP%\gamez.err` / `gamez.out` (`0x63b8ac`, `0x63b884`) | stdout/stderr log files |
| `0x406870` | `CreateFileA` `0x406916`, `WriteFile` `0x406934` | `SHGetSpecialFolderLocation(NULL, 0, …)` (the Desktop), file attributes `0x80`, mode `CREATE_ALWAYS` | one branch (command id `0x869`) of the native command dispatcher `0x4093a0` writes a script-named basename to the Desktop |
| `0x406b60` | `CreateDirectoryA` `0x406bf8`, `CreateFileA` `0x406c84`, `DeleteFileA`, `RemoveDirectoryA` | `%TEMP%`, `"~RCSA~"` `0x61f044` | write-access probe (create, write, delete, remove); reached from command id `0xc23` |
| `0x445090` | `fopen` `0x445190` | `"dynamics.txt"` `0x623a88`, modes `"r"` then `"a+"` (`0x623a98`) | flight-dynamics tuner output |
| `0x4a0bb0` | `fopen` `0x4a0bcf` | `"zbench.out"` `0x629284`, mode `"a"` | frame-rate benchmark log |
| `0x5bb820` | `fopen` `0x5bb854` | command `"PrintUsedTextures"` `0x63e708`, mode `"w"`, or `stdout` when no name is given | developer dump of the used-texture list |
| `0x424a80`, `0x425cb0`, `0x430830`, `0x432360`, `0x434650`, `0x434ca0`, `0x43b950` | `fopen` (`"w"`) + `fprintf` | `";Do not edit by hand - use net editor instead."`, `"%s(... %d ...)"` | Net Edit text data: nodes, edges, types |
| `0x494350` | `remove`, `rename`, `fopen` `0x494834` | `"location.zrd"`, `"location.bak"`, header `";; LOCATION.ZRD stores teleport data for this mission."` + `";; Last updated %s at %s."` | mission teleport data rewrite (editor) |
| `0x521420` / `0x524230` | `fopen` `0x521457` (`"wb"`), `fwrite` | magic `0x08170616`, version `0x35`, records `"Activation0000"`, `"Running%03d"` | effect-animation activation record (`zeff_anim_save.c`) |
| `0x4e2260`, `0x4e1930`, `0x4e1be0`, `0x4e20a0`, `0x4e2180`, `0x530850`, `0x559800`, `0x560320` | `fopen` `0x4e2283` (`"wb+"`), `fwrite` | GameZ archive header/node writers (`cls_zbd.c`), `"Error writing texture directory."` `0x635298`, material and model buffers; entry reached from the `GameZWriteZBDFile` command | the compiled-asset **toolchain** |
| `0x4c6720` | `fopen` `0x4c672f` (`"rb"`) + `fwrite` | `"Unable to write [%s] to zbd file."`, `"gg_read.c"` | GameGen→ZBD conversion (toolchain) |
| `0x5c0000`-block | `tmpfile` `0x5c06e1`, `fwrite`, `_rmtmp` | `"zSaveHeader"` `0x63fdbc`, `"%s/%s"` `0x63fdc8` | ZAR writer scratch file (toolchain) |

Two facts follow directly and are the core of the answer:

* every write either lands in a user-state location (save games, the user's
  plane, the user's pictures folder, the install directory's `location.zrd`,
  `dynamics.txt`, `zbench.out`, log files) or is a screenshot/debug dump, or
  belongs to the toolchain rows; and
* **there is no write of a decoded texture, a decoded sound, a parsed script or
  any other derived form, and no path or key by which such a write could be
  looked up again.**

The GameZ rows deserve their own sentence because they are the closest thing in
the binary to "the engine writes derived assets". They are reachable from the
script interpreter: the writer entry `0x4e2260`'s only direct caller is
`0x5b975d`, which is the body of the script command `GameZWriteZBDFile`
(`0x63ef30`, compared at `0x5b973a`) inside the native command dispatch at
`0x5b80a0` (the same table that implements `AnimAddZBDFile` `0x63f63c`,
`AnimSetZBDFile` `0x63f62c`, `DeleteFile` `0x63f218`, …). The load functions
this audit inspected — the zreader open (`0x579c60`), the bitmap reader
(`0x5343a0`) and the sound-set parse (`0x592d20`) — do not call it.

`GameZWriteZBDFile` is a developer command, and the script language declares it
in the shipped data (`ZBD/interp.zbd`), together with the other write-capable
commands `PrintUsedTextures`, `DeleteFile` and `mkdir`. A mission script could
therefore invoke it; what no span shows is a *read-back*: nothing opens the
archive it writes by a key derived from the inputs. The command name occurs
exactly once in the whole installation — the command table inside
`ZBD/interp.zbd` — and no script in that container calls it. Its existence is a
runtime derivation route, not a cache.

## The cache vocabulary is about sound residency

The two "cache" strings are enum names in the script language's sound/streaming
vocabulary, in a name table at `0x6406cc` (`st_cached`) and `0x6406d0`
(`st_streamed`) between `nonvolatile`/`volatile` and `sp_loop`/`sp_in_hardware`
and the `status_*` names:

| String | Addresses |
| --- | --- |
| `"Expected st_cached or st_streamed"` | `0x60c3f0` |
| `"st_cached"` | `0x60c3f9` (inside the diagnostic), `0x60d3c8` (the name) |
| `"st_streamed"` | `0x60c406`, `0x60d3bc` |
| `"PURGEABLE"` | `0x63a000` |

The retail data does **not** use `st_cached`/`st_streamed` at all: the strings
occur in the executable's string table and in `crimson.icd`, and in no container
under `$CS_GAME_DIR`. What the retail sound sets do carry is `PURGEABLE`:

* `ZBD/zrdr.zbd` contains **2,633** compiled string nodes holding `PURGEABLE`
  (tag `3`, length `9`), first at byte offset `0xe6635`; the same container
  references 2,766 `.wav` files, so the attribute is per sound, not global.
* The sound-set parser reads that child by name and folds its presence into one
  bit of the definition's flag word: at `0x592d85` it pushes `0x63a000`, calls
  the child lookup `0x57a090`, and on a hit clears and re-sets bit 6 of
  `[record+0xc]` (`and edx, 0xffffffbf` / `shl ecx, 6` / `or ecx, edx`). The
  adjacent attributes (`FREQUENCY` `0x63a00c`, `HARDWARE` `0x635300`, `VOICE`
  `0x639ff8`, `QUEUE` `0x639ff0`) are folded into neighbouring bits the same way,
  so this is an attribute bitfield, not a separate store.

So the original's observed "cached" concept is a **resident-in-memory flag on a
sound definition, whose key is the sound's own record in its set** (for example
`DIALOG → brief_c31 → c2-NW-m1_briefing.wav` + `PURGEABLE`). It carries no
content hash, no decoder version and no source-span identity. What the bit makes
the sound manager do (drop and reload from disk, or merely mark the entry
evictable; when; under what memory pressure) is **unknown**: the consumer of
bit 6 was not located in the bounded search, and this document does not guess it.

## Derived assets are compiled before shipping

The reader container is not text. Its members are a compiled node tree, and the
loader reads exactly that tree:

* `zRdrRead` at `0x57a630` reads a 4-byte node tag, dispatches tags 1–4, and
  treats tag `3` as a length-prefixed string and tag `4` as a child list
  (`malloc(count*8)` then recursion); an unknown tag logs
  `"Invalid reader node type in zRdrRead()"` `0x63993c` from
  `zrdr_main.cpp` `0x6398bc`.
* The node at `ZBD/zrdr.zbd` byte offset `0xe6635` is a tag-`3` (string) node
  whose declared length is `9` and whose payload is the ASCII token
  `PURGEABLE` — exactly the form `zRdrRead` consumes, inside the container the
  shipping data uses.
* The zreader open path `0x579c60` compares a source file's timestamp with a
  compiled copy's (`GetFileTime` `0x579d11`/`0x579d55`, `CompareFileTime`
  `0x579d61`) and, when the source is newer, logs
  `"Source file (%s >> %s) newer than in compiled version"` `0x6398e4` at
  `0x579d7b`, naming `zrdr_main.cpp:652` (`0x28c`). A compiled artefact with a
  recorded build time is therefore a concept the original has.
* The compiler that emits these trees is in the same image: the GameZ writer
  rows above, plus the GameGen→ZBD converter and ZAR writer, all under the
  `D:\zipper\gamez\...` source labels that identify the vendor's build tools.

That is derivation, but it is **build-time derivation by a tool that happens to
be linked into the game executable**, not a load-time cache. It is the answer to
"did the original precompute derived assets": yes, it compiled its reader data
(and packed its GameZ/ZBD archives) before shipping. It is not an answer to "did
it cache decoded assets at load time", and the two must not be conflated in an
F15 claim.

## The unknown, and what would settle it

This is recorded as an explicit **unknown**, not as "the original had no cache":

> Whether the original engine wrote any persistent derived artefact during a
> normal mission load, and if so what it keyed it on, is unmeasured. In the
> measured image no writer, path string, import or vocabulary supports such a
> store, but a negative over static spans cannot exclude (a) an output path
> assembled only at run time from user or registry input, (b) a DLL writing on
> the engine's behalf, or (c) a mechanism reached only through a code path this
> bounded audit did not enumerate.

What was searched is the whole import surface above (every reference of every
file-creating/writing entry point the image imports), every string in the image
(all "cache" occurrences), and the retail reader container's compiled form.
What was found is the writer table above. What evidence would settle it is an
**owner-run file-system trace of the original executable** — the same kind of
original-run evidence that `retail` alone is explicitly not:

1. Start the original game from a clean installation on a Windows host with a
   file-system monitor that records *create/write/rename/delete with full
   paths* (Process Monitor with file events, or the Sysinternals filemon
   equivalent), and record the host clock, the installation digest and the
   locale/edition.
2. Load one mission (M01 is enough) to the point where the world is
   interactive; then quit. Keep the monitor log in the owner's private
   material, not in Git.
3. Provide the log's list of written paths and the process name/EXE digest.
   Any path other than the user-state locations named above, written during
   load and re-read later, would be a candidate derived store; its content and
   the key it is looked up by are then the next question.

This is `human_play`/owner-run evidence and is not something an agent can
synthesize. It is **not** required for this task, because acceptance criterion 1
of `F15-F` explicitly allows the explicit-unknown form, and because no F15
acceptance criterion depends on the answer; it is the follow-up that would turn
the unknown into a measurement.

## What this means for F15

| F15 criterion | Effect of this finding |
| --- | --- |
| AC01 cancel a load, switch world, no old entities | unchanged — a property of our cancellation, not of an original cache |
| AC02 kill during cache write, recover cleanly | unchanged — measured only on *our* store in F15-D |
| AC03 one livery source changes only affected derived assets | unchanged — our key is the project's source-span identity; no original key was observed to copy |
| AC04 warm and cold loads deliver equal content and state | unchanged — an equality between two of our own loads |

The cache layout (`zbd-texture-base-level`, installation + source span +
decoder/IR version + options) is **ours**. It is a legitimate optimization we
chose, and this finding neither weakens nor strengthens its design; it removes
the possibility of citing the original as its justification. A future stage that
wants to claim otherwise must produce the owner-run trace above and show both
the artefact and its lookup key.

## Reproduction

```sh
# Facts on the image (no original run, no output written):
python3 tools/engine_file_io_probe.py --image "$CS_ENGINE_IMAGE"

# The same facts as JSON, for diffing:
python3 tools/engine_file_io_probe.py --image "$CS_ENGINE_IMAGE" --json

# Acceptance selection (10 tests; the two data-dependent tests need the
# variables, the parser tests do not):
CS_ENGINE_IMAGE=... CS_GAME_DIR=... \
  python3 -m unittest discover -s tools/tests \
  -p 'test_engine_file_io_probe.py' -v
```

The probe prints addresses, API names and short string literals only. It writes
nothing; it opens the image `rb` and hashes it before analysis.

## What this does not establish

* No original executable ran. Nothing here is an observed behaviour of the
  original at run time; the class is `observed_tool`/static, never
  `verified_original`.
* The absence of a persistent derived store is a property of the *spans
  searched* (listed above), not a proof of universal absence — see "The
  unknown".
* What the sound `PURGEABLE` bit does at run time, and what `st_cached` /
  `st_streamed` select, are not established.
* The GameZ writer's exact completeness (which node classes it can emit, which
  options it takes) is not established; only that it exists, that it writes the
  archive/node/texture-object/material forms named above, and that its observed
  caller is the `GameZWriteZBDFile` script command rather than a load function.
  Whether any content calls that command, and whether anything ever reads the
  archive it writes back by a key, is unknown.
* The installation's own fingerprint is recorded in the evidence report
  `docs/findings/evidence/F15-F.json` from a production `cs-inspect inventory`
  of the same `$CS_GAME_DIR`; this finding itself reads only `ZBD/zrdr.zbd`.
* This finding does not change any project cache behaviour or acceptance test.
  Nothing under `crates/` was modified.
