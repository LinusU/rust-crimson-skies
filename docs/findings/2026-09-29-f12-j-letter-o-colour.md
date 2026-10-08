# F12-J: what the original does with a colour spelled `oxff1E283C`

Date: 2026-09-29. Task: #376 / F12-J "Establish how the original reads a
colour value spelled with a letter o" — a follow-up to F12-H (#369), which
found the misspelling while surveying fractional values and recorded it
under "A finding this survey did not go looking for" in
`docs/findings/2026-09-29-f12-h-fractional-configuration-values.md`. Shared
contract: `docs/contracts/IDENTITY-CONTENT.md`. Required capability:
`retail`, used **read-only**: the members below were decoded through
`cs-inspect rof` into `private/` (git-ignored) or read straight out of the
archive by an `#[ignore = "requires CS_GAME_DIR"]` test, and the engine
image was inspected as bytes — sections, entropies, imports and readable
strings. Nothing from the installation is committed except paths, lengths,
counts, digests, value shapes and the identifiers named as evidence.

This is not a lettered slice of the feature sheet: F12-A to F12-C and the
F12-E/F12-H/F12-I follow-ups are merged stages, and F12-J exists to settle
or bound one recorded unknown.

## The question

Lines 1156-1158 of `ASSETS/LAYOUT.CSV` spell a text colour `oxff1E283C` — a
letter `o` where the `0` of `0xff1E283C` belongs — at field 7 (the
documented `Color` position of a `T` record) of `SBZ_T_TITLEJ`,
`SBZ_T_CAPTIONJ` and `SBZ_T_TEXTJ`, all inside `[@ScrapbookZoom@]`. Every
neighbouring colour is a well-formed `0x…`. A fourth field of the same
position, `SBZ_T_CAPTIONA` (line 1121), drops the `0` outright and spells
`xff000000`. Both slips face the same unknown: does the original's reader
skip the stray byte, parse what it can, refuse the field, or clamp?

## The affected content is live

`[@ScrapbookZoom@]` is driven by `ASSETS/SCRIPTS/SCRAPBOOKZOOM.SCRIPT`
(the section name is the script's base name, matching under the R1
case-insensitive rule). The script binds the objects by name — it writes
`"sbz_t_title"`, `"sbz_t_caption"` and `"sbz_t_text"` each concatenated
with a slot letter handed in by `@scrapbook@`'s callback into each
object's `szid` — assigns each object's `nresid` text resource, and calls
`initialize`. It writes no position and no colour on the text objects:
the layout record supplies both. The layout has one `SBZ_T_*` triple per
slot letter, A through Z (26 title records, pinned by the retail test),
so the J rows are reachable content, not dead data.

The script never writes the objects' colour property: the symbol map
member `ASSETS/SCRIPTS/DEBUGINFO.TXT` shows a script-visible `goscolor`
member exists, and other scripts do assign colours at run time
(`setpencolor`, `packcolor`), but `SCRAPBOOKZOOM.SCRIPT` touches these
objects' `szid` (property `YC`) and `nresid` (property `DI`) only. So
whatever colour the layout reader produced from `oxff1E283C` is the
colour the original renders on every visit to the J zoom page.

## What static inspection can and cannot reach

The layout reader is machine code, and only one image in the installation
can hold it:

| image | shape | readable? |
| --- | --- | --- |
| `crimson.exe` (344 851 bytes) | SafeDisc wrapper: imports KERNEL32/USER32/ADVAPI32/VERSION only — no MSVCRT at all, so it converts no number | yes, but irrelevant: it contains no configuration reader |
| `crimson.icd` (2 580 578 bytes) | the engine image | `.rdata` (0x159a0) and `.idata` are intact; `.text` (2 105 344 raw bytes) and `.data` are encrypted |

Measured here, matching what task #351 recorded before editing and what
the independent S12 static-recompilation project reports for the same
image (SafeDisc v1.50, `BoG_` marker): every 64 KiB block of `.text`
measures 7.3-8.0 bits/byte of entropy, the entry point is not code, and
the readable `.rdata` carries the gosScript interpreter's diagnostics
(`gos_TextDraw`, `setpencolor`, `packcolor`, the `Syntax error:` strings)
but **no layout-parser strings at all** — no field names, no member
names, no format or error text — which is consistent with a positional
reader that needs no strings. No other installed PE (`mcp.dll`,
`roffile.dll`, `drvmgt.dll`, `dplayerx.dll`, `ifc21.dll`, `ztiff.dll`,
`ijl10.dll`, `language.dll`, `langui.dll`, `strings.dll`) contains a
matching string or import either.

The one thing the imports *do* bound: `crimson.icd` imports `sscanf`,
`fscanf`, `strtol`, `atol`, `atoi`, `strtod` and `atof` from MSVCRT, and
not `strtoul`. Any CRT primitive that could parse the field is in that
set — but which of them the colour conversion calls, with what checking
of the result, is a property of code the encryption keeps unreadable.
The candidates disagree on the value: a `strtol`-family call yields `0`
with the pointer unmoved; `sscanf("%x")` fails the conversion entirely
and what the caller then reads depends on how it was written; a stricter
reader could skip the field. The data cannot choose between "renders
`0x001E283C`", "renders transparent black" and "keeps a default" — the
difference is a rendered colour on a shipped screen.

## Result: the original's rule stays `Unknown`

The task offered two outcomes; (b) applies. What the original does with
the misspelt colour is **unmeasurable from the data and the readable part
of the binaries**, for one precise reason: the code that performs the read
is inside a SafeDisc-encrypted `.text`, and no byte of it is reachable
without executing the wrapper's decryption. This is a statement about
measurability, not about likelihood — nothing here argues the original
fails, defaults or tolerates the value, and none of those is asserted.

Two paths could still settle it, both outside this task and both named:

- **An owner original-run capture** (`human_play`, never available to an
  agent): visit the zoomed scrapbook page of the item bound to slot J and
  report the rendered title/caption/text colour — e.g. invisible,
  `0x001E283C`-dark or default-black — through the REF capture protocol.
- **Unpacking `crimson.icd`**, the code-side lead task #351 already
  recorded: it would turn the rule into a code reading of the parse
  routine (which CRT call, and what the caller does on failure). That is a
  separate, larger unit of work, and a code reading is still not runtime
  evidence; it would feed the same verification stage.

The verification stage the owner routed these rules to is **F12-D**
(original-data configuration verification): this unknown joins the nine
T351 rules and the F12-H fraction rule already waiting there. A narrower
next step — an owner capture of the J zoom page, or the `crimson.icd`
unpack lead — is filed as follow-up task **#380** so the smallest possible
observation can close it independently of F12-D's larger survey.

## What the workspace does with the bytes (unchanged, now pinned)

No consumer substitutes a colour, and nothing changed to make that true:

- `FieldSpelling` classifies `oxff1E283C` and `xff000000` as `Text`, not
  `Color` — the `0x` prefix rule is the member's own spelling.
- `TuningSchema::tune` refuses both as `NotANumber` under every declared
  width; the bytes stay in the document and `reassemble` gives the member
  back.
- `RecordView` keeps position 7's `Color` kind — established by the other
  293 `T` records and the documented list — while the F12-I retail test
  counts the four unspellable fields by name; the misspelling is carried,
  never repaired.
- The `KeyedList` dialect row's `unknowns` list now records the rule as
  unmeasured, citing this task.

A consumer that needs the scrapbook colours declares what it accepts, and
this finding is where the record says why the acceptance cannot be
inferred from the data.

## Test inventory (`accept_f12_j_*`)

| Test | Covers |
| --- | --- |
| `cs_content::config::tests::…a_misspelt_colour_is_retained_unconverted` | an authored `T` record carrying the misspelt value: `FieldSpelling::Text` under a `Color` kind, `NotANumber` for a 32-bit and a float spec, the field counted typed but never converted, byte-exact reassembly, and a well-formed `0x…` control that does spell `Color` and convert |
| `…retail_misspelt_colour_fields_stay_raw` (ignored, retail) | the three `SBZ_T_*J` entries of `[@ScrapbookZoom@]` at their lines with `oxff1E283C` verbatim at field 7, each refused as `NotANumber { len: 10 }`; the sibling `xff000000` of `SBZ_T_CAPTIONA` refused likewise; the well-formed `SBZ_T_TITLEI` control converting; all 26 slot letters present; byte-exact reassembly; `SCRAPBOOKZOOM.SCRIPT` present in the container |

The retail test fails with "CS_GAME_DIR is not set" when run without it.

## Mutation probes

| Mutation | Failing test |
| --- | --- |
| `spell` treats an `o`- or bare-`x`-prefixed hex run as `Color`/`Hex` | both tests |
| `number` skips or folds the leading `o` (a "compat" parse) | both tests (`NotANumber` becomes a value) |
| the `T` schema drops position 7's `Color` kind | both tests (`kind()` / `name()` assertions) |

## Evidence

The contract requires an acceptance report for a task that uses `retail`;
it is `docs/findings/evidence/F12-J.json`, generated by
`private/evidence/F12-J/harness.py` from real runs: the task's own
selection with `--include-ignored` (2 tests, both passing — the retail
test is in the selection, so it ran), the production `cs-inspect
inventory` installation fingerprint, and production `cs-inspect rof`
decodes of `ASSETS/LAYOUT.CSV` and `ASSETS/SCRIPTS/SCRAPBOOKZOOM.SCRIPT`,
with digests only. The binary-side measurements above were made with a
private reader of the PE headers (sections, entropies, import table,
ASCII strings) over `crimson.exe` and `crimson.icd`, read-only; the
scripts live in `private/f12j/` and are not committed.

The report is validated with `tools/validate_evidence.py` **without**
`--require-pass`, because the task's acceptance criterion is that this
unestablished rule stays recorded as an unresolved issue — deleting it to
satisfy the flag would be exactly the shortcut the contract forbids.

## Commands

- `cargo fmt --all -- --check` → 0
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` → 0
- `cargo test --workspace --locked` → 0
- `cargo test --workspace --locked -- accept_f12_j_ --include-ignored` → 0
  (2 tests, 1 of them retail, in `cs_content`)

## Sources

`specs/F12-text-configuration-strings-and-pe-resources.md` (non-negotiable
#1, "no unrecorded compatibility assumptions"), `docs/contracts/
IDENTITY-CONTENT.md` (`ClaimStatus::Unknown`), the T351 finding (the
engine-image encryption lead and the R1 name rule the binding relies on),
the F12-H finding (the misspelling's discovery), the F12-I finding (the
documented `Color` kind of `T` position 7 and the four unspellable
fields), the S12 static-recompilation project's Phase-0 report (SafeDisc
v1.50 identification of the same image, independent of this workspace),
and `$CS_GAME_DIR` (read-only) for every measurement.

## Update 2026-10-06 (task #380): resolved by code evidence

The `Unknown` above is resolved from code, not from a runtime capture. The
owner supplied a decrypted engine image (`$CS_ENGINE_IMAGE`,
sha256 `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`,
decrypted from `crimson.icd` `0e3b4724…9833b`) and had it analysed
statically; the owner accepts that code evidence for this question. Status:
**code-derived, below `verified_original`**. Virtual addresses; for `.text`,
`.rdata` and `.data` below VA 0x643000, file offset = VA - 0x400000. No image
bytes or decompiled code are committed.

Rule, in reading order:

1. `Layout.csv` is read as an ini file through `roffile.dll`
   (`0x4112b0` / `0x411b30`, lookup `0x4119d0`).
2. A control's `gui_init` calls the native `gosPageInit` (`0x403680`). It
   first calls `0x405220`, which **zeroes the record-field block
   `0x64b2a4..0x64b340`**, the colour at `0x64b2dc` included, then
   dispatches on the type letter; `T`/`t` goes to `0x403f10`.
3. `0x403f10` splits the fields with `0x404830(..., ",")`: ResID, X, Y, Z,
   Width, Height (`atoi`), then **Color: `if (*token) sscanf(token, "%x",
   &colour)`** (format string at `0x61eccc`), then Justify (`atoi`). The
   `sscanf` result is not checked and nothing else writes a `T` colour.
4. MSVC `%x` takes optional whitespace, an optional sign, an optional
   `0x`/`0X`, then hex digits. A token that starts with `o` or a bare `x`
   fails at once and **leaves the destination unwritten, so the colour stays
   0**. Nothing carries over between records, because of the reset in 2.
5. The `s_text` control (script class `PE`) copies the value in `gui_init`
   unconditionally (no "0 means default" branch) and `gui_draw` passes it to
   `print3d_attributes`, which reaches `gos_TextSetAttributes` (`0x5cecd0`).
   That **drops the alpha byte** and, when `(colour & 0xFFFFFF) == 0`,
   **draws RGB(15,15,15)** instead (`0x5cecfd`-`0x5ced0e`). The control's
   alpha bookkeeping never runs (`framerate` is 0).

| Record | Stored | Parsed | Drawn |
| --- | --- | --- | --- |
| `SBZ_T_TITLEJ`, `SBZ_T_CAPTIONJ`, `SBZ_T_TEXTJ` (lines 1156-1158) | `oxff1E283C` | `0x00000000` | RGB(15,15,15), visible; not the dark navy `0x1E283C` |
| `SBZ_T_CAPTIONA` (line 1121) | `xff000000` | `0x00000000` | RGB(15,15,15), the same as the intended `0xff000000` |
| any well-spelled `0xAARRGGBB` | as written | as written | its RGB, alpha ignored; RGB 0 draws (15,15,15) |

The script names come from the `DEBUGINFO.TXT` obfuscation map
(`$$JI$$` gosPageInit, `$$OJ$$` gosColor, `PE` s_text).

In the workspace: `cs_content::config::legacy_text_colour` and
`legacy_text_rgb` implement the parse and the draw rule; the test
`accept_f12_j_misspelt_colour_parses_to_zero_and_renders_near_black` pins both
and the byte-exact reassembly. The raw token is still never repaired.
Shapes the measurement did not reach (a bare `0x`, more than eight digits)
return `None` rather than a guess. There is no UI text renderer yet; the
function is the rule it must use. The `KeyedList` dialect row no longer lists
this unknown.
