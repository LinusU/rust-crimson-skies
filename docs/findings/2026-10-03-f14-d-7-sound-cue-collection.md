# F14-D.7: the sound, music and dialogue collections of the retail baseline inventory

Date: 2026-10-03. Task #490, stage `### F14-D` of
`specs/F14-canonical-content-catalog-and-dependency-closure.md`, the audio
follow-up of #389 (F14-D.2). Capabilities used: `retail` (read-only,
`$CS_GAME_DIR`) plus ordinary build/test. Test prefix: `accept_f14_d_7_`.
Evidence: `docs/findings/evidence/F14-D.7.json`.

## Problem

`docs/contracts/IDENTITY-CONTENT.md` requires "sounds/music/dialogue/video" as
catalog collections. The baseline inventory (`cs_content::catalog::baseline`)
held **no** `ContentKind::Sound`, `Music`, `Dialogue` or `Video` row, and the
only audio catalog in the tree — `cs_content::audio` — is F41-A's **declared
synthetic** one, which carries `Origin::SyntheticFixture` and can never stand in
for original data.

The task's own rule is one bounded container family per stage
(`docs/TASK-SPLITTING.md`). This stage therefore took the **ZBD sound family**,
which is the only audio container the installation holds.

## What is in the installation

`cs_formats::zbd::role_for_path` (F06-A's own observed role rule, `RolePattern::
Prefixed { prefix: "sounds", suffix: ".zbd" }` at `RoleLevel::ContentRoot`, tied
to the pinned mech3ax v0.6.0 README in `docs/research/SOURCES.md` S02/S06)
names exactly two inventoried files:

| Container | Declared members | Distinct member names | Distinct case-folded names |
| --- | --- | --- | --- |
| `ZBD/soundsl.zbd` | 2520 | 2476 | 2475 |
| `ZBD/soundsh.zbd` | 2521 | 2477 | 2476 |

No other inventoried file matches, so no other audio container exists in this
installation: the 10 `.mpg` files under `GOSDATA/ASSETS/GRAPHICS/MPG/` are loose
video files (F40), not an audio container family.

### The member name is the only identity the index carries

The version-one index entry is 148 bytes (`EntryC`): `u32 start`, `u32 length`,
a 64-byte name field and **76 bytes the pinned source reads without explaining**
(`cs_formats::zbd::trailer`, `UNEXPLAINED_REASON`). Measured on this
installation, those 76 bytes carry no identity beyond the name:

| Bytes | Content, measured over every entry of both archives |
| --- | --- |
| `[0..4)` | A `u32` that is **not** the member position (it equals the position in exactly 0 of 2520 `soundsl` entries and 1 of 2521 `soundsh` entries) and **not** unique: `soundsl` holds 557 distinct values with 1963 members carrying `62`; every `soundsh` entry carries `62` |
| `[4..68)` | A second, NUL-padded copy of the member name — exact in 2520/2520 and 2521/2521 entries |
| `[68..76)` | One constant 8-byte tail per archive (`00 70 54 33 3c 0f c0 01` in `soundsl`, `00 d1 b3 a0 3b 0f c0 01` in `soundsh`); the two archives share the last two bytes |

The constant value `62` in every `soundsh` entry suggests the word is not a
per-member id at all. **What the word means is unknown** and stays unknown; it
is recorded here only to show that it cannot be used as identity.

### Repeated names

Fourteen names are declared more than once in each archive. In every case the
repeated members are **byte-identical**, so a repeated name is one cue stored
several times, not several cues:

- ten names five times each (`VO_id47_*` capture-throw, round-loss and
  zeppelin/chaser result lines), and
- four names twice each (`VO_c2-NW-M5_Sparks_3-5.wav`,
  `VO_c3-HW-m2_CharlieSteele_25.wav`, `VO_c3-HW-m4_Sparks_33.wav`,
  `VO_c5-MN-m4_Sparks_17-5.wav`).

`soundsl` declares `VO_c4-RM-m3_Blacke_9.wav` at index 180 and
`VO_c4-RM-m3_blacke_9.wav` at index 2149. The two differ only in letter case and
their bytes are identical (sha256 `77f03b95f8bcd980…`). The id grammar folds
ASCII case (`cs_types::content`), so these two are one identity, which is why
`soundsl` holds 2475 cues and not 2476.

### The two containers are a low-rate and a high-rate set

`soundsl` and `soundsh` declare 2476 shared names plus `chaingun.wav`, which
only `soundsh` declares. Of the shared names, **1911 store identical bytes in
both and 565 store different bytes**: `soundsl` is the 11025 Hz set and
`soundsh` the 22050/44100 Hz set of the same cues (the 2026-09-28 T344 finding
records the per-container `fmt ` shapes). So the container is part of the
identity: a bare member name would merge a low-rate and a high-rate recording of
one cue into a single row whose span names only one of them.

**Nothing in the installation states which of the two containers the original
engine played for a given cue.** That stays unknown.

## Code

`crates/cs_content/src/catalog/baseline.rs`:

- `sound_rows` finds the containers with `cs_formats::zbd::role_for_path`,
  re-checks each with `cs_formats::zbd::dispatch` (so a file whose bytes carry a
  contradicting documented signature is refused with `header_role_conflict`
  instead of parsed as a sound container), and reads each through
  `cs_formats::zbd::read_version_one_index` +
  `cs_formats::zbd::read_sound_archive`. No reader was derived for this task.
- `sound_cues` turns the listing into rows. A row is minted only for a member
  whose extent lies inside the container **and** whose RIFF/WAVE header reads
  (`cs_formats::zbd::read_wave_header`), so a cue is a recording this engine has
  read, never a name. Members are grouped by their declared name folded to ASCII
  lowercase, and every group is resolved explicitly:
  - one member, or several with identical bytes → **one row**, located at the
    first declared occurrence (the declared index is the deterministic
    tie-break and is *not* identity); each repeat is counted under
    `duplicate_member`;
  - several members with different bytes → **no row at all**, each counted under
    `ambiguous_member_name`, because the index gives no identity that tells them
    apart and minting one would guess (AGENTS.md rule 4).
- `sound_row` gives the row the identity *container + declared name* (escaped
  with the same `install_file_key` grammar the install-file rows use), a span
  carrying the container path, the member key, the member's own extent and the
  member's digest, `Parsed` + `NotNormalized` + `Unavailable`, no runtime
  consumer, and the two reasons `NotNormalized` and an explicit
  `UnsupportedReason::Unknown` carrying `f14.d.7.baseline.sound_playback`.
  Its single static edge (`f14.d.7.baseline.sound_member`, `ObservedTool`)
  points at the install-file row of the container those bytes live in.
- `unclassified_audio_statuses` adds the `music` and `dialogue` records.

## Retail results (installation `b4e780ab84cf31d8…`)

- **4951 `sound` rows**: 2475 from `ZBD/soundsl.zbd` and 2476 from
  `ZBD/soundsh.zbd`. Every member of both archives reads its RIFF/WAVE header
  (the T344 finding), so there is no header gap.
- `duplicate_member`: **90** (45 per archive: 44 byte-identical repeats plus the
  one case-folded name). `ambiguous_member_name`: **0**.
- Every row: `Origin::Installation`, a span naming the member's own bytes
  (verified by re-reading and re-hashing the range out of the read-only
  installation), `ParseState::Parsed`, `NotNormalized`, `Unavailable`, no
  runtime consumer, and one static edge onto its container's inventory row.
- `music` and `dialogue`: **0 rows**, each with a named
  `CollectionStatus::diagnostic`. Nothing in a member's bytes or in its index
  entry states a cue class; the only class-like signal is the member name
  (`music_*`, `VO_*`, `*_briefing.wav`), and a name is not a class.
- A sound is not launchable content, so the collection adds no root and moves no
  coverage denominator; all 4951 rows are unreachable unknowns counted in
  `coverage.unreachable_by_kind.sound`.

## Unknowns

- **Cue class.** Nothing states whether a member is a sound effect, a music cue
  or a spoken line. The `music` and `dialogue` collections are therefore empty
  with a recorded reason. Resolving work: F39 (dialogue cues) and F33-C (AI
  roles, dialogue voices and mission callbacks) once a **mission program** names
  a cue together with its speaker, plus F41-B/F41-C for the bus and transition
  rules. Nothing in the sound containers can supply it.
- **Which container the engine played.** 1911 cues have identical bytes in both
  containers and 565 do not; the choice rule is not stated anywhere in the
  installation. Resolving work: an original-run capture (owner-supplied) or a
  reference that shows the selection.
- **Playback metadata.** No member states a mix bus, level, one-shot/loop mode or
  consumer; every row carries `f14.d.7.baseline.sound_playback` as an explicit
  unknown. Resolving work: #444 (ADPCM decode — most retail members are MS/IMA
  ADPCM), F06-C (`sound_sample`) and #445 (mixer/device consumer).
- **Loop points.** No retail member carries a `smpl` chunk (T344), so no cue
  declares a loop region; whether the game loops a cue is decided outside the
  WAVE header and is unknown.
- **Cue points.** 35 members per archive carry a `cue ` chunk (T344). Only the
  count is reported; what the game does with the points is unknown.
- **The unexplained index words.** The `u32` and the constant 8-byte tail of each
  entry are recorded above and left uninterpreted. The repeated `62` suggests the
  `u32` is not a per-member id, but nothing states what it is.
- **Video.** `IDENTITY-CONTENT` also requires `video`. The installation's 10
  `.mpg` files under `GOSDATA/ASSETS/GRAPHICS/MPG/` are loose files with no
  container index, and no stage has read them; they are outside this stage's one
  container family. Resolving task: F40 (cutscenes, video and transitions).

## Checks

`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
--all-features --locked -- -D warnings`, `cargo test --workspace --locked` and
`cargo test --workspace --locked -- accept_f14_d_7_ --include-ignored` (9 tests,
9 passing, 0 failing) all pass on the pushed head.

### Mutation probes run on this branch

The eight synthetic tests build the sound containers and their WAVE members byte
by byte, so the production `retail_baseline` really reads them. Five mutations
were applied and reverted, and each failed the named tests:

| Mutation | Tests that failed |
| --- | --- |
| Drop the `sound_rows` call from `retail_baseline` | 7 of 8 |
| Key a cue by the member name without its container | 6 of 8 |
| Mint a row for a member whose RIFF/WAVE header does not read | `accept_f14_d_7_a_readable_member_yields_a_sound_row` |
| Mint a `music` row from a `music_` member-name prefix | `accept_f14_d_7_music_and_dialogue_hold_no_row_and_say_why`, `accept_f14_d_7_a_repeated_name_is_one_cue_or_never_two_rows` |
| Drop the `duplicate_member` count for byte-identical repeats | `accept_f14_d_7_a_repeated_name_is_one_cue_or_no_cue_and_never_two_rows` |

