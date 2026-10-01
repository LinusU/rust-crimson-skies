# F62-A: corpus separation and the oracle contract

Task #248. Declared in `tools/cs_xtask/src/corpus.rs`, bound to production
parsers in `crates/cs_formats/tests/corpus/`, exercised by the
`accept_f62_a_*` tests and reported by `cs-xtask corpus manifest|audit`.

## The three corpus classes

The spec's central rule is that three kinds of input must never be
confused, so they are distinct `CorpusClass` values with distinct source
kinds the audit can check:

- **Synthetic** — authored bytes. Either a `Builder` resolved by name in
  `crates/cs_formats/tests/corpus/fixtures.rs` (generated in readable code,
  per `fixtures/synthetic/README.md`'s recommendation) or a
  `CommittedFixture` that must be tracked under `fixtures/synthetic/`.
- **Private** — members of the read-only original installation or seeds
  kept private. `PrivateInstall` entries select install members by
  extension (`*.rof`, `*.zbd`, `*.tga`, `*.exe`/`*.dll`); `PrivateSeed`
  covers crash seeds that cannot be minimized into clean bytes (spec
  non-negotiable #2). Neither ever resolves inside the repo: `corpus audit`
  fails if any tracked file lands under `private/`, `original/`,
  `assets/original/`, `research-output/`, `cache/` or `captures/`, and it
  fingerprints matched members with a dependency-free SHA-256 so a private
  run can be compared without copying bytes.
- **Regression** — minimized reproductions of fixed bugs. The verifier
  requires the synthetic-clean source rule (builder or committed fixture)
  and a bug reference in the note. There are none yet; the class exists so
  the first fixed parser bug has a defined home rather than an ad-hoc one.

`verify_manifest()` enforces class/source separation, unique ids, resolvable
containers, non-empty provenance notes and the committed-fixture root; the
audit repeats the check against the real `git ls-files` listing.

## The oracle contract per container

`ContainerSpec` records each known parse entrypoint, the boundary kinds its
layout has, the fuzz target that feeds it, and one of three truncation
oracles:

- `PrefixRefusal` — the container is self-framed; a prefix that damages a
  required span must be refused with a structured diagnostic.
- `ExtentStatus` — the extents arrive as an input the bytes do not carry
  (`read_reader_archive`, `read_sound_archive` take a `MemberTable`), so a
  cut cannot be a top-level error; the contract is that the listing reports
  the loss (`ContainerStatus::Failed`), never a silent success.
- `NotApplicable` — the input is not prefix-closed at all: the text readers
  are total over any byte slice, and `discover_container`/`inventory_scripts`
  absorb anything into findings and opaque records. They are still declared
  (with the rationale) so "every known container" is a checkable statement,
  and each still gets a fuzz target for panic hunting.

### Boundary resolution

A fixture carries a span map resolving each declared `BoundaryKind` to real
offsets, with one of three damage rules:

- `Refuse` spans are load-bearing: a cut below the span's `end` removes at
  least part of it and must be refused. Applied to headers, tables,
  trailers and payload extents alike, this makes the oracle a one-line
  rule: *a cut below the end of any required span must refuse.*
- `Frame` spans (`script.program` opcode words) refuse only cuts strictly
  inside an element — a cut on a boundary is a valid shorter program —
  which is what "variable-length table boundary" means for fixed-width
  element streams.
- `Slack` spans are bytes the entrypoint is allowed to ignore (tolerated
  tails, unreferenced overlays, probe bytes past the signature window).
  Cuts there demand only a bounded outcome — they demonstrate the corpus
  does not claim refusal where the format doesn't.

### Measured outcomes, stage A

`accept_f62_a_truncation_refuses_every_required_cut` probes every prefix of
every fixture (46,707 probes, 46,273 required cuts). Notable shape the
sweep surfaced and the span maps record:

- `rof.tree`/`pe.layout`/`gamez.*`/`bm.image` tolerate trailing bytes, so
  the fixtures carry a trailing slack span; `read_bmp`, `read_tga`,
  `read_zbd_textures`, `read_wave_header`, `read_interp` and
  `read_version_one_index` consume to EOF, so every cut refuses.
- `zbd.trailer_index` reads its version/count from the tail: a cut
  re-reads arbitrary mid-file bytes as the version. The fixture's
  unexplained-entry bytes carry a fill pattern so no truncation can parse a
  bogus index — the sweep verified all 157 cuts refuse.
- `zbd.dispatch` only ever sees the probe prefix (≤ 8 bytes for a
  signature family): required span is the 8-byte signature window, the rest
  is slack.
- `read_member` truncations refuse via `ExtentOutOfBounds`; mid-directory
  cuts also refuse because they remove the member extent the record
  describes.
- `gamez.materials` is the heavy fixture: the format fixes a 1000-slot
  material array, so the minimal container is 44,220 bytes and contributes
  44,216 required cuts. The sweep still runs in milliseconds since parsing
  is bounds-checked reads.

## Inputs the contract deliberately does not call containers

Second-stage transforms take already-parsed values, not container bytes —
`read_placeholders` (a `KeyedList`), `decode_base_level`/`decode_levels`
(`ImageDescriptor` + slices), `decode_strip` (`&[u32]`),
`decode_sound_sample`/`decode_payload` (`SampleFormat` + member bytes) and
`probe_records`. Truncation of their byte inputs is a member-level question
for the F62-B production runners; declaring them here as containers would
fabricate boundaries they do not have.

## What stage A does not do

- It runs no private corpus member through a parser: the audit only
  enumerates and fingerprints `PrivateInstall` selectors (a run against the
  installation at `$CS_GAME_DIR` matched 2 ROF archives, 184 ZBD
  containers, 2 TGA and 23 PE images). Executing them is the F62-B runner.
- It does not diff against reference tooling (F62-B/C) and has no
  regression entries yet — the class is defined, the first fixed bug adds
  the first entry.
- `fixtures/synthetic/` and the install are untouched. The only committed
  bytes remain the existing redistributable fixtures.

## Sensitivity

Every `accept_f62_a_*` test calls production entrypoints through the
binding registry; a stubbed or removed parser fails to compile or fails
the sweep. The boundary-resolution test fails if a declared kind has no
span. The audit test runs the real `git ls-files` separation check on this
tree, so a committed file under a private-only prefix breaks it.

## Commands

```sh
cargo run -p cs_xtask -- corpus manifest          # JSON manifest + counts
cargo run -p cs_xtask -- corpus audit             # separation rules
cargo run -p cs_xtask -- corpus audit --private-root "$CS_GAME_DIR"
cargo test -p cs_formats --test corpus            # the accept_f62_a_ suite
cargo +nightly fuzz run rof_tree                  # once F62-B schedules it
```
