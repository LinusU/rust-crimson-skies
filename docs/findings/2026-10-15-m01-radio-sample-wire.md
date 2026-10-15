# M01-RADIO-SAMPLE-WIRE: the radio prepare step publishes its decoded voice members

Task: #1466 (`M01-RADIO-SAMPLE-WIRE`), follow-up of #1465
(`M01-RADIO-SAMPLE-IDENTITY`) and #1438 (`M01-PLAY-RADIO`).
Date: 2026-10-15. Capabilities used: ordinary build/test, plus `retail`
(read-only `$CS_GAME_DIR` through the production readers) and the owner's
decrypted image (`CS_ENGINE_IMAGE`, because M01's player start is only known
from it); **no original executable ran and no person heard anything**.
Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stages
`### F41-B`/`### F41-C`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.

## The gap this closes

#1438's radio prepare step (`crates/cs_app/src/mission_session/radio.rs`,
`MissionRadio::read`) decoded each queued voice member's length straight from
the installation and kept only the number of ticks. #1465 landed the identity
and the publish API (`crates/cs_app/src/audio/voice.rs`) but **nothing called
it**, so the device's library never held the members a `RadioLine` names and
the mixer refused every line by name (`sample_unavailable`) — never silence,
but never sound either
(`docs/findings/2026-10-15-m01-play-radio-dialogue.md`,
`docs/findings/2026-10-15-m01-radio-sample-identity.md`).

## What landed

* **The prepare step keeps its PCM.** `MissionRadio::read` decodes each member
  once into a `DecodedMember` — the device-ready `PcmAudio`
  (`PcmAudio::from_decoded`, the same normalize the device uses) and the
  fixed-tick length derived from that PCM — instead of the length alone. Each
  prepared line gets a `DecodedVoiceMember` built by the production
  `DecodedVoiceMember::new`, whose id is `voice_member_id`'s: the same
  derivation the line's `asset` spells, so a line and its samples name one
  identity. `MissionRadio::voices()` exposes them in member order.
* **The publish step.** `publish_prepared_voices` publishes exactly those
  members into the world's `DeviceSampleLibrary` through
  `publish_voice_members` (i.e. `SampleLibrary::insert_sample`).
* **The ordering (#1466's load-bearing half).** The publish runs in
  `PreUpdate`, **after** `audio::insert_audio_session` — the same load's
  population pass, which fills the library with `replace_samples`
  **wholesale**. `install_radio_voice_publication` installs it with that
  ordering and waits for the pass's own recorded install
  (`AudioHandoffLog::installed`), so a publish can only run once the pass has
  run and can never be discarded by it. One publish per `LoadIdentity`
  (`VoicePublicationWatch`): a reload's install is a different load, so its
  prepare step publishes into the fresh library.
* **Wiring.** `compose::add_composition` installs the system beside
  `install_launch_audio` (the library's owner); `MissionHost::radio()` exposes
  the prepared radio. An id no prepare step delivered is still refused
  `sample_unavailable` by name — the refusal path is unchanged.

Nothing here mints a new id spelling and no `ContentKind::Voice` was added: the
identity is #1465's `ContentKind::Dialogue` over F14-D.7's
`install_file_key("<container>/<member>")`, the id the `RadioLine` already
names.

## What is designed and what is unknown

* The identity rule and route (b) are the owner's designed decision (task
  #1465 note of 2026-10-15): the installation states no cue class (F14-D.7).
  Nothing here is `verified_original`.
* The container (`ZBD/soundsh.zbd`) is the labelled choice #1438 made;
  `soundsh` and `soundsl` hold the same M01 members and which the original
  opens is unmeasured (F14-D.7).
* Decoded-PCM evidence is not audible evidence. This does not show a person
  hears M01's radio; the owner's F41-D listening test stays the gate, and no
  original executable ran.
* One refusal changed shape inside the prepare step: a member whose decode
  produces no playable device shape (no samples at all, or a channel count the
  device cannot place) is now refused `Undecodable` by name instead of being
  queued as an asset no device could ever hold. No such member is known in the
  container; the change is fail-closed, not a measured finding.
* The synthetic CI scenario's member (`VO_m1_Wire_1.wav`) is authored here: it
  states no retail fact and the test says so.

## What is still open

* **Audible M01.** The owner's F41-D listening test, and after it the campaign
  playback stages (music state requests resolve to no measured asset: F14-D.7).
* A per-frame publish cost does not exist (one publish per load), but the
  prepare step now retains every queued member's PCM for the life of the
  stage; for M01 that is the members its declared groups reach, and the
  samples are shared behind an `Arc`.

Affected content: every spoken line of every mission that plays through the
radio queue. Resolving tasks: #1466 (this wiring).

## Tests

`crates/cs_app/tests/campaign/m01_radio_wire.rs`, prefix
`accept_m01_radio_wire_`, two scenarios. Both drive production code only.

1. `accept_m01_radio_wire_a_the_composition_publishes_a_prepared_member_into_the_library`
   (runs in CI, no original data): the real `build_headless` composition over
   the synthetic harbor stage, given a hand-built `LaunchSound` so the
   composition installs the real `AudioPlugin`, the real `DeviceSampleLibrary`
   and the real population pass, and a `MissionRadio` built from one
   already-decoded member. Before the first frame the library holds nothing;
   after it the member is under the exact dialogue id the prepared line names
   and the real `AudibleDevice`'s own lookup finds it (`device_closed`) while an
   id nothing published is refused `sample_unavailable`. **The ordering is
   load-bearing here**: the population pass replaces the library wholesale in
   the same frame, so a publish ordered before it is wiped and this test fails.
2. `accept_m01_radio_wire_b_the_composed_m01_run_delivers_its_opening_members`
   (`#[ignore]`, `CS_GAME_DIR` **and** `CS_ENGINE_IMAGE`): the real M01 plan and
   stage, composed headlessly and pumped past `OBJECTIVE1`'s measured
   two-second wake. The host's own sound step queues the opening group's two
   members; the library holds both under their dialogue ids; the device's own
   lookup resolves them and refuses only the absent output, while a member
   nothing published is still refused `sample_unavailable` by name.

**Sensitivity, measured on this branch (change, run, restore):**

| Change | Result |
| --- | --- |
| the `publish_voice_members` call removed from `publish_prepared_voices` | A fails (library empty after the pass) |
| the install ordered `.before(insert_audio_session)` instead of after | A fails (the population pass wipes the publish) |

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m01_radio_wire_ --include-ignored` | 0 (2 run, 2 pass) |

## Evidence

`docs/findings/evidence/M01-RADIO-SAMPLE-WIRE.json` — the acceptance report the
`retail` capability requires (`docs/contracts/CLI-EVIDENCE.md`), written by the
harness `crates/cs_app/tests/campaign/evidence/m01_radio_wire.rs` (the
`evidence_report_m01_radio_wire_writes_the_acceptance_report` test of the
`campaign` binary) from the recorded run. Its second production observation composes M01 through the real
`stage_for`/`build_headless`, pumps it past the opening wake and records the
device library's own lookup: the prepared/retained counts, each opening
member's dialogue id and decoded shape, and the device's refusal codes for the
delivered and the never-delivered id (ids, digests and counts only; no original
bytes). The report the review left committed is the **reviewer's own
regeneration on the reviewed tree** — its `candidate_tree` is the tree of the
commit that carries this documentation, so the only difference between that
tree and the pushed head is the report file itself — not the author's: the
prepared/retained counts (45/45), the tick (245), both opening
members' ids, shapes and delivery, and the `sample_unavailable` refusal for a
member nothing published are identical to the author's run. The report is
`implemented`, not `checked`: no *independent* reviewer (another agent instance
or model) looked at it, no original executable ran and no person heard anything.

## Identity of this work

Implementer: `bunny-alpha-2/bunny-alpha-2` (Rally #1466 implementation claim of
2026-10-15T13:03:22Z). Reviewer: `bunny-alpha-2/bunny-alpha-2` (the review claim
of 2026-10-15T15:39:15Z) — the **same agent name** with a fresh session context,
so by the policy in `AGENTS.md` this review is not independent evidence and no
agent review replaces the owner's approval. The review re-measured the two
sensitivities (the `publish_voice_members` call removed, and the install ordered
before `insert_audio_session`: scenario A fails both times, then restored
byte-for-byte), ran both discovered tests alone with `--exact` (including the
retail scenario B), re-ran the full workspace suite and the task selection, and
regenerated the evidence report on the reviewed tree, which reproduced the
author's numbers exactly. This pass is `checked` at most — never `recreated`:
no original executable ran and no person heard anything.
