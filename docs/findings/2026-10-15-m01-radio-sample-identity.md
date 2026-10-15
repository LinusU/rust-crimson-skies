# M01-RADIO-SAMPLE-IDENTITY: a spoken member's identity, and the route its samples take

Task: #1465 (`M01-RADIO-SAMPLE-IDENTITY`), follow-up of #1461
(`M01-RADIO-DEVICE`) and of #1438 (`M01-PLAY-RADIO`).
Date: 2026-10-15. Capabilities used: ordinary build/test, plus `retail`
(read-only `$CS_GAME_DIR`, through the production sound-archive reader); no
original executable ran and no person heard anything.
Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stages
`### F41-B`/`### F41-C`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.

## The gap

#1438 names each spoken M01 member with a `ContentKind::Dialogue` id over
F14-D.7's `install_file_key("<container>/<member>")` spelling and puts that id
on the `RadioLine` the mixer voices. Nothing put the member's **samples** in the
library the device reads:

* `cs_app::audio::samples::populate` decodes only members the declared
  `AudioCatalog` carries, and `ContentSampleSource` derives its member ids with
  `ContentKind::Sound`; a `Dialogue` id over the same key is refused
  `sample_member_absent`.
* F14-D.7 records that **no container family states a cue class**
  (`docs/findings/2026-10-03-f14-d-7-sound-cue-collection.md`), so no dialogue
  row exists in the declared catalog and the loader has nothing to hang a
  dialogue member on.
* #1438's prepare step decodes each member's length straight from the
  installation and never goes through the F15 closure or the library; no
  production closure builder exists on main.

So the line was refused `sample_unavailable` by name (#1461's refusal, not
silence) and the owner could not hear M01.

## The rule (designed, not measured)

The owner ruled on task #1465 (note of 2026-10-15) rather than leaving the task
blocked:

1. **Content kind.** A spoken voice member is `ContentKind::Dialogue` — the kind
   #1438 already mints. **One identity per member**, the one the `RadioLine`
   already names; no `ContentKind::Voice`, no new id spelling.
2. **Route (b).** The radio prepare step, which already decodes the members it
   queues, publishes exactly those decoded members into the device's
   `SampleLibrary` under that dialogue id, at prepare time, through the
   library's insert path.
3. Unchanged: an id the prepare did not deliver still refuses
   `sample_unavailable` by name; `ContentSampleSource` keeps deriving
   `ContentKind::Sound` ids for sound members; nothing here is a measured cue
   class (F14-D.7 still applies).

The installation states no cue class, so the rule is a **project convention**;
it is labelled designed everywhere and is not `verified_original`.

## What landed

`crates/cs_app/src/audio/voice.rs`:

* `voice_member_id(container, member)` — the exact id a `RadioLine` names:
  `ContentKind::Dialogue` over `install_file_key("<container>/<member>")`. It
  derives the id #1438 derives; it does not mint a second spelling.
* `DecodedVoiceMember::new(container, member, pcm)` — pairs one already-decoded
  member with its identity, computed once at construction.
* `publish_voice_members(library, members)` — inserts each member under its
  dialogue id through `SampleLibrary::insert_sample`, and reports how many
  reached the library.

`crates/cs_app/src/audio/device.rs`: `SampleLibrary::insert_sample` — the
member-level insert path the publish step uses. It is deliberately **not**
`replace_samples`: it runs on top of the same load's population pass, after it,
so the closure's sound members and the prepare step's voice members are both
reachable. The reload semantics are unchanged — a reload's `replace_samples`
empties the library (voice members included) and the new load's prepare step
publishes into that fresh library.

## What is designed and what is unknown

* The identity rule and route (b) are designed (owner decision), not measured:
  the installation states no cue class.
* The container the retail scenario reads (`ZBD/soundsh.zbd`) is the labelled
  choice #1438 made (`soundsh`/`soundsl` hold the same M01 members; which the
  original opens is unmeasured, F14-D.7).
* No gain, bus, priority or timing claim is added here: the radio path is
  #1461's and #1438's, unchanged.
* Nothing here is `verified_original`.

## What is still open

* **The producer (the prepare step's call).** On this base #1438
  (`M01-PLAY-RADIO`) is still open (`in_progress`, branch
  `rally/1438-play-m01-s-radio-dialogue-and-music-cues`), so its
  `MissionRadio::read`/`offer_cues` prepare step is **not on main** and cannot
  be edited here. The wiring that makes the prepare step call
  `publish_voice_members` into the composition's `DeviceSampleLibrary` belongs
  to #1438's `crates/cs_app/src/mission_session/` paths and is filed as
  **#1466** (`M01-RADIO-SAMPLE-WIRE`, depends on #1438 and #1465). This task
  proves the whole device-side half — a real
  member reaching the library under the exact id, and the device's own lookup
  telling delivered from never-delivered — through production code.
* **The ordering contract.** `publish_voice_members` must run after the same
  load's population pass (`replace_samples` would discard an earlier publish).
  The follow-up must place it there.
* **Audible M01.** This is not evidence that a person hears M01's radio. That
  stays the owner's F41-D listening test.

Affected content: every spoken line of every mission that plays through the
radio queue. Resolving tasks: #1438 (the prepare step that publishes) and #1466
(its call site).

## Tests

`crates/cs_app/tests/accept_m01_radio_sample.rs`, prefix
`accept_m01_radio_sample_`, three scenarios. They drive production code only:
the real `publish_voice_members`, the real `AudibleDevice`'s own library lookup,
and the real mixer in a real Bevy `App`. The session and mixer are installed by
hand because the loading handoff that normally installs them is #1438's cue
path, which lands separately; the API driven (`AudioSession::enqueue_radio`) is
the one `offer_cues` calls.

1. `accept_m01_radio_sample_a_...` — the id is `Dialogue` over F14-D.7's key and
   is **not** the sound id over the same key; before the publish the library
   holds nothing; after it the member is under the exact id; the device's own
   lookup finds the samples (only the absent output refuses, `device_closed`);
   the line is voiced as one `OneShotStarted` on `AudioBus::Radio` carrying
   that asset, with nothing refused; and the same published library is driven
   through the mixer's **own** device (a real `AudibleDevice` sharing it), where
   the line resolves the published member and is refused only the absent output
   (`device_closed`), never `sample_unavailable` — the delivered half of the
   boundary contrast whose never-delivered half is scenario B.
2. `accept_m01_radio_sample_b_...` — the sound id over the same key does not
   satisfy the line; an id the prepare did not deliver is refused
   `sample_unavailable` by the device's own lookup and by the mixer against the
   line; the queue still completes the line on schedule and names the voiced
   completion with no voice behind it.
3. `accept_m01_radio_sample_c_...` (`#[ignore]`, `CS_GAME_DIR`) — reads the real
   M01 member `VO_c2-NW-m1_WorkersVoyage_1.wav` from `ZBD/soundsh.zbd` with the
   production VFS/`ZbdContainer`/`sound_member_pcm` chain, publishes it under its
   dialogue id, and shows the device's lookup telling that delivered member
   (`device_closed`) from one it was never given (`sample_unavailable`).

**Sensitivity, measured on this branch (remove, run, restore):**

| Change removed | Result |
| --- | --- |
| `publish_voice_members` no longer inserting | A and C fail; B (the refusal path) still passes |

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m01_radio_sample_ --include-ignored` | 0 (3 run, 3 pass) |

The review re-ran all four on the rebased commit `b17232b0`: fmt 0, clippy 0,
the prefix selection 0 (3 run, 3 pass), and the workspace suite 0 (582
binaries, 0 failed). One earlier workspace run failed
`accept_m01_play_weather_the_plugin_draws_each_layer_under_the_camera` with
`Resource does not exist`; that test passes in isolation and the immediate
rerun of the identical command was green, so it is a pre-existing flake
unrelated to this branch (filed as **#1471** `M01-PLAY-WEATHER-FLAKE`), and the
green rerun is the run of record.

## Evidence

`docs/findings/evidence/M01-RADIO-SAMPLE-IDENTITY.json` — the acceptance report
the `retail` capability requires (`docs/contracts/CLI-EVIDENCE.md`), written by
the harness `crates/cs_app/tests/evidence_report_m01_radio_sample.rs` from the
recorded run. Its second production observation re-runs the
read/decode/publish/lookup chain over the owner's installation (no original
bytes: ids, digests and counts only) and records the member, its dialogue id,
the decoded frames/rate/channels, and the device's own refusal codes for the
delivered and the never-delivered id. The report is `implemented`, not
`checked`: no reviewer looked at it here, no original executable ran and no
person heard anything.

## Identity of this work

Implementer: `bunny-alpha-2/bunny-alpha-2` (Rally #1465, implementation claim of
2026-10-15T06:44:13Z). This pass is `checked`, not `recreated`: no original
executable ran and no person heard anything. Review must be an independent pass
(fresh context, different agent instance or model); same-agent review is not
independent evidence and no agent review replaces the owner's approval.

## Review of this work

Reviewer: `bunny-alpha-2/bunny-alpha-2` (Rally #1465 review claim of
2026-10-15T08:32:15Z). The reviewer ran in a **fresh session context** but is
the **same agent name as the implementer**, so this is not an independent
review and is not independent evidence (owner directive, 2026-09-28).

What the review changed: scenario A previously proved the library insert and
the device lookup as two disconnected steps — its mixer branch used the
recording stand-in, which never consults a sample library, so the delivered
member was never *resolved* by the device the mixer drives (the
delivered/never-delivered contrast lived in scenario B alone). Scenario A now
adds a second world whose output is a real `AudibleDevice` sharing the
published library; the line resolves the published member there and is refused
only the absent output (`device_closed`), never `sample_unavailable`. Removing
the publish now fails this boundary assertion on its own (measured: with the
boundary library detached from the publish, the assertion fails with
`sample_unavailable`). The finding's scenario-A description was updated to
match.

The `retail` acceptance report was regenerated on the rebased commit
`b17232b0` (tree `b7999712`) and re-validated with
`tools/validate_evidence.py --require-pass`; its `review.identity` records this
same-agent review. The claim stays `implemented`; a Rally merge awards
`checked`, and no agent review replaces the owner's F41-D listening gate.
