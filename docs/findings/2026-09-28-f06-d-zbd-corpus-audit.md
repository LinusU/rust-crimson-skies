# F06-D: family-by-family ZBD corpus audit with a strict status

Date: 2026-09-28. Task: F06-D "Complete family-by-family private corpus
coverage" (`specs/F06-zbd-families-reader-archives-and-sound-containers.md`,
section `### F06-D`, AC04). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: `retail`
(read-only `$CS_GAME_DIR`) plus ordinary build/test. Test prefix:
`accept_f06_d_`. Evidence: `docs/findings/evidence/F06-D.json`.

## Files and the one observable failure (listed before editing)

- `crates/cs_assets/src/zbd.rs`: `ZbdContainer::reader_archive` (the reader
  family through the same producer and foreign-index guard as
  `sound_archive`), and the audit model — `MemberVerdict`, `MemberAudit`,
  `ContainerVerdict`, `ContainerAudit`, `ZbdAudit`, `audit_container`,
  `audit_containers`.
- `tools/cs_inspect/src/zbd.rs` (new): the `zbd-audit` command, its JSON
  report and the inline `accept_f06_d_*` tests plus the evidence harness.
- Wiring: `tools/cs_inspect/src/lib.rs` (`pub mod zbd;`, doc),
  `tools/cs_inspect/src/main.rs` (dispatch `zbd-audit`, help text, doc),
  `tools/cs_inspect/Cargo.toml` (depend on the sibling `cs_formats`) and
  `Cargo.lock`; `crates/cs_assets/src/lib.rs` (doc sentence).

**One observable failure:** before this stage no command could show a corrupt
ZBD member at all. `SoundAssets` kept bounds failures out of its entries and
counted WAVE failures separately ("a strict audit (F06-D) must count this
**and** `failures`"), and no code listed reader members through the VFS. A
corpus with one member whose extent reaches into the index therefore had no
row saying so and no nonzero status. The minimum-scenario test fails at
`assert_eq!(result.exit_code, 3)` / the `failed` row assertions when the
member bounds verdict is dropped.

## What the command does

```sh
cs-inspect zbd-audit --cs-path "$CS_GAME_DIR" --strict --out private/zbd-audit.json
```

1. F02 discovery, then one content session with F04's designed layout.
2. Every inventoried `.zbd` file is opened by its `install:` key through
   `ZbdContainer::open` (F06-C producer: VFS read, digest check, validated
   installation path, two-key dispatch).
3. Sound and reader containers: the container's **own** version-one trailer
   index (task #343), then one row per declared member — duplicates stay
   separate rows, index anomalies are listed on their row.
   - sound: `decoded` (the member decoded under its own `fmt ` declaration:
     the `0x0001` PCM plan of stage F06-C and, since task #444, the `0x0002`
     Microsoft ADPCM and `0x0011` IMA ADPCM block layouts read through the
     plan task #524 gave the consumer), `readable` (a declared format this
     stage cannot decode from the declaration the member carries, with its
     tag), `failed` (out of bounds, unreadable WAVE header, payload
     contradicting its declaration);
   - reader: `readable` with the recorded "encoding undocumented" reason, or
     `failed` (out of bounds).
4. Texture, interp, GameZ and animation containers are routed (header
   validated where a signature rule exists) and reported `not_listed` with
   the feature that reads them (F08, F07, F10, F20).
5. A container that cannot be opened, routed or indexed is a `failed` row
   beside the other containers.

The report carries the installation and content fingerprints, per-family
totals, and per container the key, SHA-256, mount, session generation,
installation path, length, family, dispatch basis, header status, verdict,
uncovered ranges and member rows (index, name as lossless text, offset,
length, anomalies, verdict, code/reason/detail). `readiness` is always
`not_assessed`: the audit never claims playability (non-negotiable #4).

Exit codes (CLI-EVIDENCE): `0` pass; `3` any container or member failed, or
with `--strict` any content left uninterpreted (readable member, not-listed
container, index anomaly, uncovered range); `2` invalid input or `--out`
inside the installation; `4` no installation; `1` runtime failure. The report
is written on exit 3.

## Retail result (fingerprint `b4e780ab…1978`, content `a0223506…c12d`)

Recounted on 2026-10-02 (Rally task #525), after task #444 taught `cs_formats`
the two ADPCM layouts and task #524 routed the runtime consumer through the
block-aware plan. Until then this audit only decoded PCM, and its census below
read "22 decoded, 5,019 readable" for the sound family.

From `accept_f06_d_retail_every_zbd_container_is_audited_family_by_family`
and the evidence artifact `zbd-audit.json` (private, referenced by digest):

| Family | Containers | Verdict | Members | Decoded | Readable | Failed |
| --- | --- | --- | --- | --- | --- | --- |
| sound | 2 | listed | 5041 | 5041 | 0 | 0 |
| reader | 62 | listed | 1293 | 0 | 1293 | 0 |
| texture | 49 | not_listed | — | — | — | — |
| interp | 1 | not_listed | — | — | — | — |
| gamez | 9 | not_listed | — | — | — | — |
| animation | 61 | not_listed | — | — | — | — |

- 184 containers, 6334 members, **no corruption**: `zbd-audit` exits 0.
- With `--strict` it exits **3**: 1293 readable reader members plus 120
  not-listed containers = 1413 uninterpreted items. No sound member is left
  uninterpreted. This is the honest state of the corpus, not a defect of this
  stage.
- Independent probe in the test: every sound/reader row count equals the
  member count read directly from the file's trailer, and a sound member's
  `decoded` row is the one its own `fmt ` chunk earns — a tag out of
  {0x0001 PCM, 0x0002 Microsoft ADPCM, 0x0011 IMA ADPCM} together with the
  geometry that tag needs (a nonzero `nBlockAlign`, the `fmt ` extension
  behind the 16 common bytes, a `data` payload with at least one block; a PCM
  payload a whole number of frames). The census it re-reads from the archives'
  bytes is the one tasks #344 and #444 measured, and every row agrees: 22 PCM
  members, 555 IMA ADPCM and 4,464 Microsoft ADPCM, none undecodable.
- No index anomaly and no uncovered range anywhere (as task #343 found).

Per archive, as that census measured it on this installation (the aggregate is
task #344's 22 PCM and task #444's 5,019 compressed members; the split is
measured by this task, and only `soundsl` declares the IMA codec):

| Archive | Members | 0x0001 PCM | 0x0011 IMA ADPCM | 0x0002 MS ADPCM | Undecodable |
| --- | --- | --- | --- | --- | --- |
| `ZBD/soundsl.zbd` | 2520 | 11 | 555 | 1954 | 0 |
| `ZBD/soundsh.zbd` | 2521 | 11 | 0 | 2510 | 0 |
| both | 5041 | 22 | 555 | 4464 | 0 |

`accept_f06_d_retail_a_corrupted_copy_fails_beside_its_valid_siblings`
copies `ZBD/C1/MP1/zrdr.zbd` and `ZBD/C1/MP2/zrdr.zbd` into a Git-ignored
directory under `private/`, stretches the last member of the first copy 16
bytes into its index, and runs the command with `--strict`: exit 3, exactly
that member is `member_out_of_bounds`, every sibling row is identical to the
audit of the unmodified copy, and the other archive has no failure. The
installation is never written.

## Tests (`tools/cs_inspect/src/zbd.rs`, `accept_f06_d_*`)

| Test | Covers |
| --- | --- |
| `a_corrupt_member_is_shown_beside_valid_siblings_with_a_nonzero_strict_status` | **AC04**: a sound archive with a non-RIFF member and a member reaching into the index between decoded siblings (one a duplicate name); exit 3 with and without `--strict`; the written report and stderr name both corrupt rows with their codes and keep the siblings |
| `strict_fails_on_uninterpreted_content_and_passes_a_decoded_corpus` | an all-PCM corpus passes strict; a member that declares a tag with no `fmt ` extension to read it under is `readable` with its tag (exit 0, strict 3); a GameZ container is `not_listed` naming F10 |
| `a_corrupt_container_is_a_row_beside_the_others` | a version-2 trailer (`unsupported_trailer_version`) and a GameZ header at the interp role (`dispatch`) are failed rows beside a listed archive |
| `cli_refuses_bad_input_and_a_missing_installation` | exit 4 without an installation, 2 for bad flags and for `--out` inside the installation (nothing written) |
| `the_census_counts_each_decoded_tag_and_needs_its_own_geometry` (added in the #525 review) | the census helper's own rules on authored members: each of the three decoded tags with the geometry it needs, a tag with no geometry, a tag this crate does not decode, a zero `nBlockAlign`, an empty payload, a partial PCM frame, a short trailing block, a member that is not RIFF, a member with no `data` chunk, and an archive whose fourth member reaches past the file (counted `undecodable`, so nothing drops out of `decoded() + undecodable`) |
| `retail_every_zbd_container_is_audited_family_by_family` (ignored without `CS_GAME_DIR`) | the retail result above |
| `retail_a_corrupted_copy_fails_beside_its_valid_siblings` (ignored without `CS_GAME_DIR`) | AC04 on retail bytes |

Mutation probes (applied, `accept_f06_d_` run, restored):

| Mutation | Failing test |
| --- | --- |
| member bounds failure ignored | `a_corrupt_member_…` |
| strict ignores uninterpreted content | `strict_fails_…` |
| listing stops after the first failed member | `a_corrupt_member_…` |
| an unreadable WAVE header counted as readable | `a_corrupt_member_…` |
| failed containers dropped from the audit | `a_corrupt_container_…` |
| exit code always 0 | three synthetic tests |
| the re-read census stops counting Microsoft ADPCM members as decoded (task #525) | `retail_every_zbd_container_…` (`left: 2521 right: 11` on `soundsh`) |
| the runtime consumer refuses a block-coded member again (the pre-#524 state), in the #525 review | `retail_every_zbd_container_…` (`left: 11 right: 2521` on `soundsh`) |
| the census drops the PCM whole-frames rule (review of #525) | `the_census_counts_each_decoded_tag_…` |
| the census drops the Microsoft `fmt ` extension check (review of #525) | `the_census_counts_each_decoded_tag_…` |

## Recorded unknowns (not guessed)

- Reader entry encoding (F06-B), the 76 unexplained index bytes (task #343)
  and loop points (task #344) stay unknown; the audit reports them as
  `readable`, never as decoded. The 1,293 reader members and the 120
  containers no F06 reader member-lists are why `--strict` still exits 3.
- ADPCM decoding is no longer unknown (task #444 decoded the two layouts the
  archives declare and task #524 made the consumer read them), so the 5,019
  compressed members are `decoded` rows. What the original executable does
  with the decoded samples — pitch, volume, spatialisation, looping — is still
  unknown and belongs to F41; this stage measures the bytes, not the game.
- The content of the texture, interp, GameZ and animation containers is not
  read here; each row names the feature that owns it.

## Review fix

`SoundAssets::entry(index)` (F06-C) indexed the list of assets, which skips
members that failed their bounds check. After such a member it returned the
next member's asset, although its doc promised a lookup by declared position.
The review made it look the asset up by declared position. The F06-C test
that had pinned the shifted result now asserts the declared positions, and
the audit uses `entry` directly.

## Recount (Rally task #525) and what still awaits review

Task #525 updated this stage's arithmetic after task #444 decoded the two
ADPCM layouts and task #524 gave the runtime consumer the block-aware plan:
the retail census above is a recount, and `docs/findings/evidence/F06-D.json`
was regenerated from the same harness. Two smaller corrections came with it —
the synthetic `readable` row in
`strict_fails_on_uninterpreted_content_and_passes_a_decoded_corpus` is now
described as what it is (a `fmt ` chunk with a tag and no `fmt ` extension,
which no retail member declares) instead of as "ADPCM is not decoded", and the
independent census counts every tag the runtime decodes rather than PCM alone.

### Review of #525 (self-review, not independent)

**Identities.** Implementer: `bunny-alpha-1/bunny-alpha-1` (implement claim of
2026-10-02T18:16:34Z). Reviewer: **the same agent instance**
`bunny-alpha-1/bunny-alpha-1`, in a fresh session over the implementer's branch
(review claim of 2026-10-02T18:58:20Z). A review by the agent that wrote the
change is **not** independent evidence, so the numbers below are checked but
unwitnessed; an independent reviewer is still wanted before any fidelity claim
rests on them. Nothing here is a `verified_original` or `release_approved`
claim, and no agent review replaces the owner's approval.

**What the reviewer checked, on the installation itself.**

- Re-read both sound archives with a from-scratch Python probe sharing no code
  with the workspace (`private/review/f06d_probe.py`, Git-ignored): trailer,
  index, `fmt ` and `data` chunks. It gives `soundsl` 11 PCM / 555 IMA / 1,954
  Microsoft and `soundsh` 11 / 0 / 2,510 — the same numbers the test helper,
  the finding table and task #444 record.
- All seven `accept_f06_d_` tests pass with `CS_GAME_DIR` set; the retail test
  fails loudly (`CS_GAME_DIR is not set`) when it is not.
- Production-side mutation probe: refusing a block-coded member in
  `cs_assets::zbd`'s `sound_verdict` (the pre-#524 state) makes the retail test
  fail loudly with `left: 11, right: 2521` on `soundsh` — the assertion really
  pins the consumer's verdict, it does not merely restate the census.
- `--strict` still exits 3, and `passes(strict)` is untouched.

**Three fixes the review made**, all in `tools/cs_inspect/src/zbd.rs`:

1. The census skipped a member whose extent reached past the archive, so such a
   member vanished from `decoded() + undecodable` while the production row
   counted it `failed`. It is now counted `undecodable`, and the retail test
   asserts `census.decoded() + census.undecodable == count` per sound archive.
2. A comment claimed the ADPCM arms needed "a `data` payload holding at least
   one block". That is wrong: a block-coded member's trailing block may be
   shorter than `nBlockAlign`, which `decode_payload` accepts as the format's
   own final block. The rule (a nonempty payload) and its comment now agree,
   and `declared_wave_layout`'s PCM comment states that the runtime's stronger
   `nBlockAlign == nChannels * bytesPerSample` rule agrees with the census's
   simpler whole-frames rule on every retail PCM member (12 mono 8-bit, 8 mono
   16-bit, and one 2-channel member of each width).
3. Added `accept_f06_d_the_census_counts_each_decoded_tag_and_needs_its_own_geometry`,
   a synthetic test pinning every branch of the census helper. Before it, each
   geometry rule was reachable only from the retail test, so a mutation in the
   census itself was invisible to CI. Dropping the PCM whole-frames rule or the
   Microsoft `fmt ` extension check each fail it loudly.

