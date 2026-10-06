# M01-LC-AUDIO-LOAD: the delivered closure now fills the sample library

Task: #652 (`M01-LC-AUDIO-LOAD`). Date: 2026-10-06.
Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
`### F41-D`, on top of F14-D.7's sound-member identity and F15's delivered
closure. It closes the last open item recorded in
`docs/findings/2026-10-05-m01-lc-audible-audio-device.md` ("the hop from a
delivered F15 load closure to a populated `SampleLibrary`").

## What was missing

Task #635 left a real device, a real plugin and a real mixer, and one hole: no
loader filled the `SampleLibrary` the audible device reads. A `VoiceStart`
naming an asset the load had actually delivered was refused as
`sample_unavailable` because the samples were nowhere. A mission had to hand
`AudioPlugin::audible` a library it built by hand.

## What the loader populates

One call inside the handoff that already installs the session —
`cs_app::audio::handoff::insert_audio_session` — now also runs
`cs_app::audio::samples::populate`, on the same install, from the same
reading of the same closure:

* **Candidate set**: the newest load's own `LoadedItemBinding` entities whose
  content kind is `sound`, `music` or `dialogue` **and** whose id the world's
  declared `AudioCatalog` holds. An undeclared id is already refused by name in
  that pass (`AudioHandoffRefusal::UndeclaredAsset`) and is not decoded behind
  the catalog's back; it contributes neither a sample nor a second refusal.
* **Identity of a member**: the F14-D.7 derivation — the container's
  installation-relative spelling plus the name its own index declares,
  `install_file_key("<container>/<member>")` — recomputed from the container
  the binding's own `AssetKey` addresses. Identity is never taken from an
  offset or a digest, so a library entry and a catalog row cannot name two
  different members.
* **Decode**: `cs_app::audio::sound_member_pcm`, the same per-member consumer
  of the F06-C decode the device test uses: the member's own bytes under its
  own WAVE header, normalized onto `-1.0 ..= 1.0`.
* **Install**: one `SampleLibrary::replace_samples` call with the whole map.
  Replacing rather than merging is what makes a reload observable: the
  previous closure's samples are unreachable the moment the call returns, and
  a voice naming one is refused as `sample_unavailable`.
* **Counts**: `AudioInstall` gained `samples` and `sample_refusals`, so the
  handoff's own record says how many delivered members reached the library and
  how many did not, next to the existing `specs`/`refused` pair for routing.

The library and the device are one object: `AudioPlugin::audible` now
publishes the very `Arc` it opened the device from as the
`DeviceSampleLibrary` resource, so the loader cannot fill a library the device
does not read. A world that mixes to the recording stand-in has no such
resource, and nothing is refused for it — there is nothing to fill.

## The source, and its generation discipline

Bytes come from `AudioSampleSource`, a resource the load's issuer publishes
alongside the announcement, wrapping the production `ContentSampleSource`
(a `ContentSession` — the same session the closure was read through). Two
rules, both refusal-shaped rather than silent:

1. **No source** → `AudioHandoffRefusal::NoSampleSource`, library emptied.
2. **A source from another content session** →
   `AudioHandoffRefusal::ForeignSampleSource`, library emptied. The source's
   `SessionGeneration` must be the delivered load's own, so one session's
   bytes can never be decoded into another session's library.

An empty candidate set also empties the library: a reload that delivered no
audio must not leave the replaced closure's samples behind.

## Refusal codes this pass can produce

| Code | Meaning |
| --- | --- |
| `unreadable_header` (and every other `SampleError` code) | the member's own WAVE header or payload did not decode; carried unchanged from `sound_member_pcm` |
| `sample_member_absent` | the container the binding's key addresses declares no member under that derived id (also used when a source reports no result at all) |
| the `ZbdError` code | the container did not open, did not index, or is not a sound archive (`resolve`, `read`, `dispatch`, `index`, ...) |
| `NoSampleSource` / `ForeignSampleSource` (handoff variants) | the world published no source, or one from another content session |

Every one lands in `AudioHandoffLog::refusals` with the content id it concerns.

## What it cannot populate (named, not skipped)

1. **Only the ZBD sound family is decoded.** A delivered `music` or
   `dialogue` id has no sound-container member under its derived id, so it is
   refused `sample_member_absent`. Whether music and dialogue archives exist
   in the installation and what they contain is F14-D.7's open question, not
   something this pass guesses. Affected content: every delivered `music` and
   `dialogue` member. Resolving task: the F14-D.7 music/dialogue collection,
   then this loader.
2. **Identity is by derived content id, not by digest.** The closure records a
   `payload_sha256` per item and the catalog records a member fingerprint;
   this pass matches neither against the bytes it decodes. A container whose
   member *names* match but whose bytes differ would be decoded as-is. Named
   here rather than papered over; a digest check belongs with the closure
   builder, which does not exist yet.
3. **An audio load item's `AssetKey` must address the container**, not a
   single member: `ZbdContainer::open` resolves one file. No production
   closure builder exists on main (no `LoadItem::new` call in `src/`), so this
   convention is recorded here as the one this loader honours; a member-level
   key is refused by the reader's own code rather than silently misread.
4. **The source must be published before the first frame the bundle is
   attached.** The population pass runs once per install, with the session
   install — deliberately, so a retry cannot add a refusal per frame. A source
   that arrives later is a late announcement, and the library says so by being
   empty rather than by being refilled forever.
5. **No retail run in this task.** Nothing in #652 was measured against
   `$CS_GAME_DIR`: the scenarios are synthetic fixtures written into a temp
   installation and read through the production VFS, ZBD and decode path.
   Decoding an *original* member is covered by #635's `#[ignore]`d scenario
   (`accept_m01_lc_audio_device_a_retail_sound_member_decodes_and_plays_...`);
   the loader itself has not been run over the installation, so "the retail
   closure populates the library" is **not** claimed here.
6. **Decode cost lands on one frame.** Population runs synchronously in
   `PreUpdate` of the install frame. What a real mission's closure costs there
   is unmeasured; there is no closure of that size to measure yet.
7. **Nothing about hearing changed.** `human_play` and `human_review` remain
   owner gates (#654); this task adds no audible claim and awards no
   `verified_original`.

## The API change this required

`SampleLibrary::pcm` now returns an owned `PcmAudio` handle instead of
`Option<&PcmAudio>`, and the trait gained `replace_samples`. The reason is
structural, not cosmetic: the library is written through a shared handle *after*
the device holds it, so a lookup cannot borrow the map it is filled from.
`PcmAudio` keeps its samples behind an `Arc`, so the handle costs a clone of
three words, never a copy of a decoded member. `InMemorySamples` now holds its
entries behind a mutex for the same reason. There is exactly one other
implementation of the trait in the workspace (none in tests), and the device's
single call site was updated in the same commit.

## Tests

`crates/cs_app/tests/accept_m01_lc_audio_load.rs`, prefix
`accept_m01_lc_audio_load_`, six scenarios, all production code (real
`AudioPlugin`, real F15 `LoadTransaction` and `ExpectedLoad` handoff, real VFS
session, real ZBD reader, real `sound_member_pcm`):

1. `..._a_delivered_closure_populates_the_library_the_device_reads` — the
   member's own samples are in the library, the `LoopingVoice` source emits
   them, and the device's own lookup tells "delivered" (`device_closed` on an
   unopened device) from "never delivered" (`sample_unavailable`).
2. `..._a_reload_replaces_the_library_and_drops_the_old_samples` — two
   closures, two content sessions: exactly one member left, and the device
   refuses the previous load's.
3. `..._a_refused_member_is_named_by_its_own_code_and_absent` — a non-WAVE
   member is named `unreadable_header`, a member no container holds is named
   `sample_member_absent`, both absent, siblings unaffected.
4. `..._a_source_from_another_session_is_refused_and_empties_the_library`.
5. `..._a_world_without_a_sample_source_names_it_and_fills_nothing`.
6. `..._an_undeclared_delivered_audio_is_named_and_not_decoded` — delivered
   next to a declared member, so it asserts the filter rather than an empty
   library.

**Sensitivity**: with the `populate(...)` call removed from
`insert_audio_session`, all **6 of 6** fail (measured: `cargo test -p cs_app
--test accept_m01_lc_audio_load` after that removal, then restored).

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m01_lc_audio_load_ --include-ignored` | 0 (6) |
