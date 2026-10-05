# F41-D: the original-media audit of every sound member

Date: 2026-10-05. Task #653 (`F41-D-MEDIA-AUDIT`), stage `### F41-D` of
`specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`. Capabilities used:
`retail` (read-only, `$CS_GAME_DIR`) plus ordinary build/test. No audio device was
used. Test prefix: `accept_f41_d_`. This document awards at most **checked**.

## What was built

`cs_app::audio::audit` (`crates/cs_app/src/audio/audit.rs`):

- `sound_container_spellings` finds every sound container with the sound family's
  own role rule (`cs_formats::zbd::role_for_path`), so the set is what the
  installation holds, not a list written here.
- `audit_container` opens one container through the production producer
  (`cs_assets::zbd::ZbdContainer`) and gives **every declared member** a
  `MemberRow`: container, declared index position, declared name, the F14-D.7
  `ContentId` (`ContentKind::Sound`, key = `install_file_key("<container>/<name>")`,
  the same derivation as `cs_content::catalog::baseline`), `wFormatTag`, channels,
  rate, `nBlockAlign`, `wSamplesPerBlock` (block-coded members) and readiness.
- A member that does not decode is `MemberReadiness::Refused` carrying the
  refusal's own code; a member whose extent fails its bounds check has no bytes
  and is counted by the refusal's code in `unreadable_extent`. Nothing is dropped,
  and `ContainerAudit::reconciles` states `decoded + refused + unreadable ==
  declared`, with `declared` read from the archive's own index listing.
- `ContainerAudit::playable` keeps up to two **distinct** (different bytes)
  members per (tag, channels, rate) shape that decode into non-silent samples.
  Unsigned 8-bit PCM is measured from its silence at 128.

"Playable" here means: decodes under the member's own declared format into samples
with a nonzero excursion. It is **not** audible evidence.

## Retail results (installation `$CS_GAME_DIR`, run of 2026-10-05)

| Container | Declared (own index) | Rows | Decoded | Refused | Unreadable extent |
| --- | --- | --- | --- | --- | --- |
| `ZBD/soundsl.zbd` | 2520 | 2520 | 2520 | 0 | 0 |
| `ZBD/soundsh.zbd` | 2521 | 2521 | 2521 | 0 | 0 |

**AC04 media coverage: 5041 of 5041 declared members have a row and decode
(5041 = 2520 + 2521, matching the index counts F14-D.7 measured).** Every member's
name keys (5041 of 5041 `ContentId`s). Repeated names keep separate rows here
(one per declared position); the 4951 catalog rows of F14-D.7 are these 5041 less
its 90 counted repeats.

Shapes (members per `(tag, channels, rate)`), each with two distinct non-silent
decoded samples except where noted:

| Container | Tag | Ch | Rate Hz | Members | Samples kept |
| --- | --- | --- | --- | --- | --- |
| soundsl | PCM 1 (8-bit) | 1 | 11025 | 11 | `30cal_gun.wav`, `40cal_gun.wav` |
| soundsl | MS ADPCM 2 | 1 | 11025 | 1954 | `gull_hit.wav`, `metalbang.wav` |
| soundsl | IMA ADPCM 0x11 | 1 | 11025 | 555 | `c2-NW-m1_briefing.wav`, `c2-NW-m2_briefing.wav` |
| soundsh | PCM 1 | 1 | 11025 | 1 | `milesdrop.wav` (only one) |
| soundsh | PCM 1 | 1 | 22050 | 8 | `30cal_gun.wav`, `40cal_gun.wav` |
| soundsh | PCM 1 | 2 | 22050 | 2 | `taxi.wav`, `train2.wav` |
| soundsh | MS ADPCM 2 | 1 | 11025 | 1954 | `gull_hit.wav`, `metalbang.wav` |
| soundsh | MS ADPCM 2 | 1 | 22000 | 3 | `fireball.wav`, `mechmove.wav` |
| soundsh | MS ADPCM 2 | 1 | 22050 | 470 | `engine_zep.wav`, `enginelooped.wav` |
| soundsh | MS ADPCM 2 | 1 | 44100 | 24 | `c2-NW-m1_briefing.wav`, `c2-NW-m2_briefing.wav` |
| soundsh | MS ADPCM 2 | 2 | 9710 | 1 | `chaingun.wav` (only one) |
| soundsh | MS ADPCM 2 | 2 | 22050 | 58 | `spruce_engine_start.wav`, `spruce_engine_loop.wav` |

Observations: the 22000 Hz and 9710 Hz shapes are what the headers declare; no
reason for them is known. `soundsh` holds the 1954 members at 11025 Hz that
`soundsl` also holds (F14-D.7: 1911 byte-identical across the archives, 565
different), so those shapes are not exclusive to the "low" archive.

## What this does not establish

1. **Nothing was heard.** No member was played through the audible device (the
   backend is task #635's branch and is not on `main`), and a decode with a
   nonzero peak does not prove a recording is the cue its name says. Audible
   review is the owner's `human_review` (task #654); an automated run is not
   `human_play`.
2. **Music and dialogue coverage is still unknown.** Nothing in a member or its
   index entry separates a music cue from a spoken line (F14-D.7). This audit adds
   no `music` or `dialogue` row. Affected content: every cue F41-C's music
   director and radio queue would route. Resolving work: F39 / F33-C once a
   mission program names a cue with its speaker.
3. **Loop seams are unmeasured.** This task did not open any member's `smpl`
   chunk (the reader reports it unread). Whether the original repeated assets end
   to end is unknown.
4. **Bus faders, a limiter and Doppler** are untouched: no original fader is known
   and Doppler is neither evidence-backed nor a designed option yet (F41
   non-negotiable behavior 1). These items of #653's background stay open and
   need their own tasks.
5. "Playable" says nothing about which container the original engine played for a
   cue; F14-D.7 records that as unknown.

## Tests

`crates/cs_app/tests/accept_f41_d_media_audit.rs`, all `#[ignore = "requires
CS_GAME_DIR"]` and failing loudly without it: every member of every container has
a reconciling row (names and counts asserted against the index); refusals are
counted by their own code; each shape has a distinct playable sample. The three
share one audit run, which decodes all 5041 members (about 2.5 minutes in a debug
build).
