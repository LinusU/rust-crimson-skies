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
   - sound: `decoded` (PCM decoded under the member's own header), `readable`
     (a declared format this stage does not decode, e.g. ADPCM, with its tag),
     `failed` (out of bounds, unreadable WAVE header, payload contradicting
     its declaration);
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

From `accept_f06_d_retail_every_zbd_container_is_audited_family_by_family`
and the evidence artifact `zbd-audit.json` (private, referenced by digest):

| Family | Containers | Verdict | Members | Decoded | Readable | Failed |
| --- | --- | --- | --- | --- | --- | --- |
| sound | 2 | listed | 5041 | 22 | 5019 | 0 |
| reader | 62 | listed | 1293 | 0 | 1293 | 0 |
| texture | 49 | not_listed | — | — | — | — |
| interp | 1 | not_listed | — | — | — | — |
| gamez | 9 | not_listed | — | — | — | — |
| animation | 61 | not_listed | — | — | — | — |

- 184 containers, 6334 members, **no corruption**: `zbd-audit` exits 0.
- With `--strict` it exits **3**: 6312 readable members plus 120 not-listed
  containers = 6432 uninterpreted items. This is the honest state of the
  corpus, not a defect of this stage.
- Independent probe in the test: every sound/reader row count equals the
  member count read directly from the file's trailer, and the decoded count
  per archive equals the members whose raw `fmt ` tag is `1` (PCM). 22 PCM
  members in total, which matches task #344's shape table (11 in `soundsl`,
  11 in `soundsh`). F06-C's findings say "11 8-bit PCM ones"; that is the
  `soundsl` figure only.
- No index anomaly and no uncovered range anywhere (as task #343 found).

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
| `strict_fails_on_uninterpreted_content_and_passes_a_decoded_corpus` | an all-PCM corpus passes strict; an ADPCM member is `readable` with its tag (exit 0, strict 3); a GameZ container is `not_listed` naming F10 |
| `a_corrupt_container_is_a_row_beside_the_others` | a version-2 trailer (`unsupported_trailer_version`) and a GameZ header at the interp role (`dispatch`) are failed rows beside a listed archive |
| `cli_refuses_bad_input_and_a_missing_installation` | exit 4 without an installation, 2 for bad flags and for `--out` inside the installation (nothing written) |
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

## Recorded unknowns (not guessed)

- Reader entry encoding (F06-B), ADPCM decoding (F06-C), the 76 unexplained
  index bytes (task #343) and loop points (task #344) all stay unknown; the
  audit reports them as `readable`, never as decoded.
- The content of the texture, interp, GameZ and animation containers is not
  read here; each row names the feature that owns it.

## Review fix

`SoundAssets::entry(index)` (F06-C) indexed the list of assets, which skips
members that failed their bounds check. After such a member it returned the
next member's asset, although its doc promised a lookup by declared position.
The review made it look the asset up by declared position. The F06-C test
that had pinned the shifted result now asserts the declared positions, and
the audit uses `entry` directly.
