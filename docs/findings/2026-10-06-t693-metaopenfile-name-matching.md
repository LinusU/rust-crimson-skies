# Task #693: `MetaOpenFile`'s name-matching rule

Date: 2026-10-06. Task: #693 (`Determine MetaOpenFile's name-matching rule for ROF members
(case sensitivity)`), the follow-up #341's finding (section D) and #686's finding (sections C and E)
left open. Test prefix: `accept_t693_`. Owner paths: `crates/cs_assets/tests/`, `docs/findings/`;
doc-comment-only edits (no behaviour) in `crates/cs_assets/src/vfs/` (`gos.rs`, `mount.rs`,
`resolve.rs`) and in #686's test file.

**Outcome: the rule is settled from addresses, and it is neither of the two rules #686 offered.**

| side | rule |
| --- | --- |
| a **container** member (or directory) name | the requested name is **upper-cased, then compared byte for byte against the name the container stores, which is never folded**: `upper(request) == stored`, exact |
| a **loose** file | **no folding at all** in `roffile.dll`: the name is joined to the registered directory and handed to `CreateFileA`, so the **host file system decides** |

On this installation the container rule agrees with #686's default
`GosNameMatch::AsciiInsensitive` **on the case dimension**, because every name both containers store
is ASCII-uppercase (measured below) — and the two case-only pairs the rule decides hold
**byte-identical** copies, so for `ARIAL8.TGA` and `FONT.TGA` the choice is *which source answers*,
not *which bytes arrive*. Separators are a separate dimension and stay runtime-unsettled: the
original splits a request only on `\` (`0x1001b238`), so how callers spell `/` is a question about
requests, not about this rule. `GosNameMatch::ExactSpelling` is **not** the original's rule: it would
answer from the loose file where the original answers from the container.

## Provenance

Static analysis of the owner-supplied original files, re-derived on this machine; nothing here is a
runtime capture, and nothing is `verified_original`.

| item | value |
| --- | --- |
| decrypted image | `$CS_ENGINE_IMAGE` |
| image SHA-256 | `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` (matches #341, #689, #703) |
| matching module | `$CS_GAME_DIR/GOSDATA/ASSETS/BINARIES/roffile.dll` — where `MetaOpenFile` lives |
| module SHA-256 | `1bc7b4b1adba1bf6a510e62f93473824c662acbf2a2176087b7b4b85338ba58a` (matches #341's provenance) |
| module form | plain PE32, image base `0x10000000`, `crypto false`, compiled 2000-05-13 (`rabin2 -I`) — it is not packed, so it needs no decryption |
| method | `r2` disassembly (`aaa`, `pdf`, `axt`, `pxw` over the vtables) and `rabin2 -E`/`-i` for exports and imports, with byte scans of the module image to bound the claims (below) |

The image is read-only and **not** committed, and neither are its bytes, its strings nor any
decompilation: this file records addresses, imports and behaviour only. Addresses in the table
below are virtual addresses; `0x1000xxxx` is `roffile.dll`, `0x004xxxxx`/`0x006xxxxx` is the
decrypted image (image base `0x400000`).

The rule itself has no address inside `crimson.decrypted.exe`: that file only *calls*
`MetaOpenFile` (section E), and the matching code lives in `roffile.dll`, which it loads by name.
So the rule's addresses below are `roffile.dll` addresses, qualified by that module's sha256 in the
table, and the decrypted image's own sha256 and its own addresses are recorded beside them — both
are owner-supplied original files read from `$CS_GAME_DIR`, and neither is committed.

## A. `MetaOpenFile` itself: order, then the first source that answers

`MetaOpenFile` is export `0x10001080`; its body is `0x100016e0` (the export tests the global
`0x1001d810` and, when the system is initialised, calls `0x100016e0` at `0x10001094`; with the
global still zero it returns 0 at `0x1000109a`). The body walks the registered sources —
`esi` steps one pointer per iteration from `[this]` to `[this+4]` — and calls each source's vtable
slot at **`0x1000172a  call dword [edx+4]`** with the requested name as a `CString const&`. The
**first** source that returns non-zero wins: `0x10001742  jne 0x10001765`, which records the source
(`[this+0xc] = esi`, `[this+0x10] = 1`) and returns 1 (`0x10001765`–`0x1000176d`); exhaustion returns
0 (`0x1000174e`). This is #341/#686's insertion-order, first-hit rule, and it is unchanged here.

Each source receives a **freshly built copy** of the caller's name: `0x10001708  mov eax,[name]`
→ `0x10001713  call 0x100039b0`, which for a real pointer measures it with `lstrlenA`
(`0x10003a29`), allocates (`0x10003a38  call 0x100037d0`) and copies it (`0x10003a46  rep movsd`,
`0x10003a4d  rep movsb`). The copy is rebuilt **every iteration**, so a case conversion one source
performs on its own copy cannot reach the name the next source is asked with, and the name the
caller passed in is never written back.

## B. Container member matching: upper-case the request, compare the stored name exactly

The registered sources are built by the two `AddNew*` exports:
`AddNewROFDirectory` (`0x10001000` → `0x100014c0`) allocates 188 bytes and runs `CROFDirectory`'s
constructor `0x10004260` for a container, `AddNewDirectory` (`0x10001040` → `0x100015d0`)
allocates 52 bytes and runs `CNOWADDirectory`'s constructor `0x100068b0` for a directory — the two
push-back functions are otherwise byte-for-byte the same. Slot 1 of `CROFDirectory`
(`0x10004940`) is the path opener; it splits the requested path and asks for **one component at a
time**:

* the splitter `0x10007d10` cuts the name on the string at `0x1001b238` (bytes `5c 00`, a single
  `\`), also tests `0x1001b2e8` (`:`) for a drive prefix, and strips leading `\`;
* `0x10007ea0` (called at `0x100049a2` and `0x10004a2a`) hands out the **next component** by
  assigning it into the `CString` at the local slot (`0x10004a2a  call fcn.10007ea0` → its
  `0x10007ebc  call 0x10003aa0`);
* that `CString` — address and buffer — is what the per-component call at `0x100049ee  call
  dword [edx+0x10]` receives (`0x100049db  mov eax,[local]`, `0x100049e1  lea ecx,[local]`).

That slot-4 method is `0x10004740`, and it reaches the index lookup at
**`0x1000478b  call 0x10004ba0`** (`this` = the container's index, `param1` = the component
`CString const&`, `param2` = the out record). `CROFFile`'s slot 2 (`0x10006f10`) reaches the same
function at **`0x10006f5b`**; those two call sites are the only ones (`axt @ 0x10004ba0`).

Inside `0x10004ba0`, and this is the rule:

| step | address | what it does |
| --- | --- | --- |
| copy the request | `0x10004bdc  call 0x10003aa0` | the local `CString` takes `param1`'s text |
| **upper-case it** | `0x10004be5  call 0x10003e80` | `0x10003e80` is `MakeUpper`: `0x10003e8b  call dword [CharUpperA]` on `0x10017124` |
| compare | `0x10004bfd`–`0x10004c1b` (tree walk), `0x10004c4c`–`0x10004c75` (equality re-check) | byte-for-byte `mov`/`cmp` loops, **no case folding** |
| read the answer | `0x10004ca8`–`0x10004cbc` | writes the 24-byte record of the matched entry into the out parameter (`rep movsd` at `0x10004cbc`) |

The **keys** are the container's own name strings, inserted with the same byte-wise compare and
**no folding**: `0x10004f90` builds the index for one directory block — name = that block's name
table + the record's last `u32` (`0x10004fcd  mov edi,[ebp+0x28]`, `0x10004fda  mov eax,[esi+0x14]`,
`0x10004fdd  add eax,edi`) — constructs the key `0x10004fea  call 0x100039b0`, looks it up
(`0x10005001  call 0x10005960`) and, on a miss, builds the entry (`0x10005073  call 0x10005140`)
and inserts it (`0x1000507f  call 0x100050f0`), comparing at
**`0x10005017`–`0x10005035`** with the same unfolded loop. Its only caller is `0x10004918`, in the
slot-4 method above.

So the container rule is:

```
upper(requested component)  ==  name stored in the container     (byte-exact)
```

with the folding applied to the **request only**. Two consequences follow without further
measurement:

* a stored name that contains a lowercase ASCII letter can **never** be found — no request can
  upper-case into one — so on a container whose names are all uppercase the rule is exactly
  "case-insensitive request against a fixed spelling", and on one that is not, the odd spelling is
  unreachable rather than reachable-insensitively;
* directory components are matched by the same rule as file components: they are entries of a
  directory block like any other, indexed by `0x10004f90` and looked up by `0x10004ba0`.

## C. That is the only case conversion in the module

`CharUpperA` is imported once (`0x10017124`, `USER32.dll`) and **called at exactly one address in
the whole image**: `0x10003e8b` inside `0x10003e80`. A byte scan for `ff 15 24 71 01 10` over
`roffile.dll` returns that one site, and `CharLowerA`/`CharUpperBuff` are not imported at all: the
module's only two `USER32.dll` imports are `LoadStringA` (`0x10017120`) and `CharUpperA`
(`0x10017124`). `0x10003e80` has
three callers:

| address | caller | folds |
| --- | --- | --- |
| `0x10002056` | `InitIniFile`'s parse (`0x10001f00`) | an INI key |
| `0x1000242c` | `FindInIniFile` (`0x10002370`) | an INI key |
| **`0x10004be5`** | the container index lookup `0x10004ba0` | **the requested file name** |

Neither INI site is on the file-open path, so `0x10004be5` is the one place a file name is folded
anywhere in `roffile.dll`.

The module also imports no comparison routine (`rabin2 -i`: no `strcmp`, `_stricmp`, `lstrcmp*`,
`CompareString*`), so comparisons are hand-rolled byte loops: the two-instruction epilogue
`1b c0 83 d8 ff` of such a loop occurs 28 times in the image — in the INI code, in the
`std::map`/tree helpers, in `0x10004ba0` and in `0x10004f90`, plus one occurrence at `0x100147aa`
that sits in `.text` outside any function this analysis attributed, so it is counted but not
claimed. The loop heads read for this task (`0x10004bfd`, `0x10004c4c`, `0x10005017`, `0x1000209b`,
`0x10002129`) each compare one string's byte with the other's, untransformed: where folding happens
in this module, it is the `CharUpperA` call of section C and nothing else.

## D. Loose-file matching: the host decides

Nothing on this side can fold case, because section C already establishes that the module's one
`CharUpperA` call site sits in the container lookup. What the code then does with the name is this:

`CNOWADDirectory`'s slot 1 (`0x100069b0`) reaches the object embedded at `this+0x10` (built by
`0x100068d8`–`0x100068e3`) and stores the handle it comes back with (`0x100069d5`–`0x100069db`).
The path strings are assembled by `0x100074b0` — whose only caller is `CNOWADDirectory`'s slot 4 at
`0x10006a35`, and which `AddNewDirectory`'s push-back invokes with the registered directory at
`0x10001642` — as *copy, append the separator string `0x1001b238`, append its second argument*
(`0x100074d3  call 0x10003aa0`, `0x100074d8  push` / `0x100074df  call 0x10003d30`,
`0x100074eb  call 0x10003d90`), returning 1 at `0x100074f2`. No conversion appears anywhere in it.

`CDiskFile`'s slot 2 (`0x10007710`) finishes the open: it takes the directory's path and the name
(`0x10007744`–`0x10007765`, appending `0x1001b238` between them, or the name alone when there is no
directory at `0x1000777a`), validates it (`0x1000778c  call 0x10007240`) and passes that string
unmodified to **`0x100077bd  call dword [CreateFileA]`**. `CreateFileA` has two references in the
module: that call, and a pointer load at `0x1000765b` (the other `CDiskFile` open helper).

So on the loose side the rule is Win32's: on the retail Windows filesystems a case-insensitive
match answers. That is a property of the host, not of `roffile.dll`, and it is why the loose pair
below is served whatever case a caller asks in — the loose side never refuses on case.

## E. The decrypted image adds no folding of its own

`crimson.decrypted.exe` does not import `roffile.dll` statically and has no `CharUpperA` import at
all — it does import MSVCRT's `_strupr`, `toupper`, `tolower` and kernel32's `lstrcmpiA`, which is
exactly why the call site below is pinned rather than assumed. It loads the module and resolves the
exports itself:

| address | what |
| --- | --- |
| `0x4116d3`–`0x4116e1` | `LoadLibraryA("assets\binaries\roffile.dll")`, module stored at `0x64e714` |
| `0x411701` … `0x41175e` | `GetProcAddress` for `AddNewDirectory`, `AddNewROFDirectory`, `MetaOpenFile`, `MetaReadAndCloseFile`, `CloseROFSystem`, `InitIniFile`, `ShutdownIniFile` |
| `0x64e2c8` | where the `MetaOpenFile` pointer is stored (`0x411734`) |
| **`0x411e31`**, **`0x411ed0`** | the only two `call dword [0x64e2c8]` sites |
| `0x411820`, `0x41184a` | `AddNewROFDirectory` for `…\GOSDATA\Assets\crimptch.rof` (guarded by `GetFileAttributesA` at `0x411814`) and for `Assets\crimson.rof` |
| `0x41185e`, `0x411869` | `AddNewDirectory` for the two empty BSS strings (`0x64e710`, `0x64e71c`) — #341's step 3 and 4 |

The wrapper both call sites share (`0x411dd0`) passes the requested name through as it received it:
it walks the string only to skip DBCS lead bytes (`IsDBCSLeadByte` at `0x411df1`, `0x411e0b`) and to
collapse a doubled `\` (`0x411e14`–`0x411e1e` → the copy loop `0x411e96`–`0x411ec5`), then
`0x411e31  call [MetaOpenFile](name, &out)`. No case conversion happens before `MetaOpenFile`.

What is **not** settled here is the spelling of the name game code asks for at runtime — with no
original run, whether some caller up-cases (or spells `/` rather than `\`) before reaching
`0x411dd0` is unknown. That changes the *input* to the rule, not the rule.

## F. What it decides, measured on this installation

Measured through production code, not from the disassembly: `accept_t693_metaopenfile_name_matching`
(`#[ignore = "requires CS_GAME_DIR"]`) mounts both containers and the loose tree with
`cs_assets::vfs::gos::SessionBuilder::mount_gos_chain`, resolves and reads through
`GosChain::read`, and pins:

* **both containers store only ASCII-uppercase names** — 846 members of `crimson.rof` and the single
  member of `crimptch.rof`, directory components included, spell like `ASSETS/GRAPHICS/ARIAL8.TGA`;
  so on this installation `upper(request) == stored` and `#686`'s `AsciiInsensitive` answer the same
  way for every key the containers hold;
* **exactly two case-only pairs exist**, and they are the two #686 named: the loose
  `GOSDATA/ASSETS/GRAPHICS/arial8.tga` and `GOSDATA/ASSETS/GRAPHICS/font.tga` against the
  container's `ASSETS/GRAPHICS/ARIAL8.TGA` and `…/FONT.TGA`. Every other loose file — the
  `MPG/*.mpg` names, the two `.rof` files, the four `.dll`s — has no container counterpart at all,
  case-folded or otherwise, so this rule decides nothing else on this installation;
* **the pairs are byte-identical**: the decoded container member and the loose file are the same
  bytes for both names (45636 bytes, sha256 `a8d6dca7…`, and 65580 bytes, sha256 `3c544ab4…`). The
  rule therefore decides *which source reports the answer* — the container, because it is registered
  first and it matches under every casing — and not *which bytes are served*. Had they differed, the
  same rule would have made the container's copy the one every caller gets.

`AIRFRAME.SCRIPT` is **not** a case pair: both containers store the identical spelling and
different bytes (670/1641 versus 703/1813), so its answer is decided by registration order alone
(#686), which the case rule does not touch.

## G. Corrections this makes to the record

* `docs/findings/2026-10-05-f04-d-original-lookup-order.md` section D — its "depends on
  `MetaOpenFile`'s matching rule, which the static analysis did not settle (#693)" — now states the
  rule and the measured outcome.
* `docs/findings/2026-10-06-f04-d-gos-registration-order.md` (#686) sections C and E: the rule is
  settled, `AsciiInsensitive` is what the original does on this installation, `ExactSpelling` is not
  what it does, and the ARIAL8/FONT pairs are answered by the container with identical bytes.
* Task #686's own scope note (`add_note` on #686) records the same correction where its next worker
  will read it.
* `docs/findings/evidence/T686.json` keeps its `unknowns` row for this rule. That report records
  what #686 knew when it ran, and rewriting reviewed machine-readable evidence to look better is
  exactly what AGENTS.md forbids; this file supersedes it and says so.
* Neither `GosNameMatch` variant implements the rule's shape literally — both fold both sides or
  neither, while the original folds the *request* only — and on retail that changes nothing, so it
  is filed as #714 (`F04-D-gos-request-fold-rule`) rather than fixed here.

Still unknown, and named rather than smoothed over:

| what | why | who settles it |
| --- | --- | --- |
| the case and separator spelling the game's GOS requests carry at runtime | only an original run shows it; the rule above is what happens *to* a request, not what requests look like | an owner-supplied original run |
| the host filesystem's case behaviour for a loose name | `CreateFileA` decides it; no `roffile.dll` address can | an original run on the target host |
| non-ASCII/DBCS names | `CharUpperA` folds per the ANSI codepage; every retail GOS name measured is ASCII | nothing on retail; recorded for completeness |

## Test

`crates/cs_assets/tests/accept_t693_metaopenfile_name_matching.rs`, prefix `accept_t693_`, one
retail test `accept_t693_metaopenfile_name_matching_retail_pairs_and_container_spelling`
(`#[ignore = "requires CS_GAME_DIR"]`, panics without it). It pins section F through production code
only: the container spellings, the two-and-only-two case pairs, the resolution of the lowercase
request to the container member, and the byte equality of each pair.

No production **behaviour** changes: the VFS keeps both rules as explicit chain inputs, because the
rule's shape (fold the request, not the stored name) is not what either enum variant implements
literally. What did change outside this file are doc comments in `crates/cs_assets/src/vfs/`
(`gos.rs`, `mount.rs`, `resolve.rs`) and in #686's test file that still called the rule
*unmeasured* — they now point here. The evidence report `docs/findings/evidence/T686.json` and the
`unknowns()` text that generates it are left as they were (section G).

## Evidence

`docs/findings/evidence/T693.json`, generated by
`evidence_report_t693_writes_the_acceptance_report` (`crates/cs_assets/tests/evidence_report_t693.rs`)
from the recorded acceptance log, production discovery of `$CS_GAME_DIR` and the production GOS chain
of that installation — `pairs.json` holds the member counts, the registration order, the two
case-only pairs and both digests of each. The acceptance run it describes was green (exit 0, 1
discovered / 1 executed / 1 passed, 0 failed). It validates with
`python3 tools/validate_evidence.py … --artifact-root private/evidence/T693` **without**
`--require-pass`, because the report's three `unknowns` are this task's residue (the table above)
rather than failed assertions, and removing them to satisfy that flag would state that this task
knows things it does not; `claim` stays `implemented`, never `verified_original`.
