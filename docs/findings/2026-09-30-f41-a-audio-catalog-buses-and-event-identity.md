# F41-A: audio catalog, buses and event identity

Date: 2026-09-30. Task: F41-A "Define audio catalog, buses and event identity"
(`specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, section
`### F41-A`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Capabilities
used: ordinary build/test only (no `$CS_GAME_DIR` read, no evidence report
required).

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/audio.rs` (new): the provenance-carrying declared
  half — `AudioBus` (seven buses), `AudioLevel`, `PlaybackMode`, `DecodedPcm`,
  `AudioPlayback`, `AudioDraft`, `AudioAssetRecord`, `AudioCatalog` (+
  `buses()`/`missing_buses()`), `is_audio_kind`, and the
  `declared_synthetic_audio_catalog` fixture (one record per bus, the weapon
  one-shot at its centre). `AudioAssetRecord::try_new` refuses a non-audio id
  by name; `AudioCatalog::insert` refuses a duplicate id by name; `AudioLevel`
  and `DecodedPcm` are validated at construction. Unknown playback metadata is
  honest content here and is refused later, at lowering.
- `crates/cs_sim/src/audio_events.rs` (new): the Bevy-free consumer half —
  `AudioEventId` (`session, tick, producer, sequence`), `AudioEmitterId`,
  runtime `AudioBus`/`PlaybackMode`, `OneShotEvent`, `LoopBinding`,
  `AudioAssetSpec`, `AudioRouter` (`play_one_shot`, `start_loop`, `stop_loop`,
  `active_loop`, `active_loop_count`, `apply_pause`, `device_lost`),
  `OneShotOutcome`, `LoopOutcome`, `EmitterStopReason`, `PausePolicy`, and the
  `synthetic_weapon_one_shot` / `synthetic_engine_loop` fixtures.
- `crates/cs_app/src/audio/mod.rs` (new): the application boundary —
  `AudioEmitterBinding` (the generation-stamped ECS record) and module docs.
- `crates/cs_app/src/audio/lower.rs` (new): `lower_record`, `lower_catalog`,
  `lower_bus`, `lower_mode`, `LoweredAudioAsset` and `AudioLowerError`
  (`UnknownBus`, `UnknownLevel`, `UnknownPlaybackMode`, `Spec`).
- `crates/cs_content/src/lib.rs`, `crates/cs_sim/src/lib.rs`,
  `crates/cs_app/src/lib.rs` (wiring only): `pub mod audio;` /
  `pub mod audio_events;` and the module-doc paragraph for each.
- `crates/cs_content/tests/accept_f41_a_audio_catalog.rs`,
  `crates/cs_sim/tests/accept_f41_a_audio_events.rs`,
  `crates/cs_app/tests/accept_f41_a_audio_boundary.rs` (new): the
  `accept_f41_a_*` acceptance tests. The minimum scenario —
  "a weapon event replayed twice plays one accepted one-shot" — is
  `accept_f41_a_a_replayed_weapon_event_plays_one_accepted_one_shot` in the
  `cs_sim` file, and it also runs end-to-end through lowering in
  `accept_f41_a_lowered_weapon_catalog_plays_one_shot_end_to_end`.
- This file.

**One observable failure:** the same `AudioEventId` delivered twice plays two
one-shots. Without the bounded per-`(session, producer)` sequence ledger, the
second delivery of the minimum scenario's weapon event returns `Accepted`
instead of `SuppressedDuplicate`, so a simulation or network replay doubles the
gunfire. `accept_f41_a_a_replayed_weapon_event_plays_one_accepted_one_shot`
fails (probe 1 below), and the same replay through the whole declared→lowered→
router path fails in `accept_f41_a_lowered_weapon_catalog_plays_one_shot_end_to_end`.

## The contracts (designed)

The split mirrors `damage` ↔ `cs_sim::damage`: `cs_sim` may depend only on
`cs_types`/`cs_script` (`docs/01-ARCHITECTURE.md`), so the declared,
provenance-carrying record lives in `cs_content::audio`, the normalized runtime
type in `cs_sim::audio_events`, and `cs_app::audio` is the only place they meet.
F41-B owns the actual mixer; this stage fixes the vocabulary, the identity and
the routing rules.

- **Seven closed buses.** F41's deliverable names engine, weapons, impacts,
  environment, music, radio and UI. `AudioBus::ALL` is the single closed table
  in each half, and `from_label` scans it, so `label` and `from_label` cannot
  disagree. The declared and runtime vocabularies are deliberately parallel
  types (neither crate may see the other); `lower_bus`/`lower_mode` map field
  for field and `accept_f41_a_every_declared_bus_lowers_to_its_runtime_twin`
  asserts the two label sets agree, so they cannot drift apart silently.
- **Identity is qualified, and defined once.** `AudioEventId` is the contract's
  `EventId(session, tick, producer, sequence)` id and `AudioEmitterId` its
  `ActorId(session, serial)` id.
  `cs_types::net` carries the shared `SessionId`/`EventId`/`ActorId` types
  (F54-A); `cs_sim::animated_object` migrated onto them in #397 and
  `cs_sim::audio_events` in #496, so both audio names are aliases of the shared
  types rather than second definitions (`T-IDENTITY-AUDIO`;
  `docs/findings/2026-10-03-t496-shared-audio-identity.md`). The audio emitter
  is the neutral shared `ActorId`, not a damage-vocabulary type: F41-B binds an
  actor to an emitter explicitly, so audio identity stays independently
  testable. The session is the shared nonzero `SessionId` at every constructor
  boundary, and a foreign session generation is refused by name, never aliased.
- **Dedup is bounded.** A producer stamps a strictly increasing `sequence` on
  its own events and the router keeps only the highest accepted sequence per
  `(session, producer)`, an `O(producers)` `BTreeMap`, not a set that grows with
  mission length. Every re-delivery at or below the mark is
  `SuppressedDuplicate` (F41 non-negotiable behavior 3).
- **Loops are one per emitter.** The loop registry holds at most one
  [`LoopBinding`] per `AudioEmitterId`; a new bind replaces the old one and
  reports `Swapped` carrying the stopped loop's id, so a swap cannot leak. The
  four stop reasons (`Despawned`, `EmitterSwapped`, `Paused`, `DeviceLost`) are
  distinct because the consumer differs; a stop for an idle emitter is reported
  as `NotActive` rather than swallowed (F41 non-negotiable behavior 3).
- **Device loss is total and does not gate progress.** `device_lost` stops every
  loop and is a no-op the second time, so mission progression never depends on
  a physical device (F41 non-negotiable behavior 2). The declared `PausePolicy`
  (a designed policy, not an observed one) either suspends every loop or leaves
  them running.
- **Unknowns refuse, they do not default.** A declared bus, level or mode that
  is `Resolved::Unknown` is honest content, but `lower_record` refuses it with
  its claim id and reason (`UnknownBus`/`UnknownLevel`/`UnknownPlaybackMode`)
  and never lowers it to a guessed `Designed` bus. `lower_catalog` refuses the
  whole catalog by the first offending row in id order, so one unmixable cue is
  a visible gap rather than a silent fidelity claim. The asset-kind and gain
  rules are applied on both sides of the boundary, with
  `MAX_AUDIO_GAIN` asserted equal so the two ceilings cannot drift.
- **Generation-stamped binding.** `AudioEmitterBinding` carries the
  session-qualified emitter, bus, asset and the scene `generation`, exactly like
  `crate::scene::SceneNodeBinding` and `crate::damage::DamageActorBinding`, so a
  reload stamps new bindings and a stale one is identified by mismatch.

## Designed vocabulary, not original data

Every bus, level, playback mode, id grammar, gain ceiling, pause policy, stop
reason and fixture value in the three new modules is **newly authored project
design**, carrying `Designed`/`SyntheticFixture` provenance. None of it is a
measurement. The following are **unknown** and are not guessed here:

- the original 2000 PC game's audio routing and mix: which bus a cue plays on,
  the fader/level, the voice limit and the effect chain (unmeasured);
- the original engine's doppler, attenuation, spatialization and smoothing
  (unmeasured; F41-B/D own them);
- the original loop semantics: F06/T344 measured that **no retail member
  declares a loop region** (no `smpl` chunk), so whether and how the game loops
  a sound is decided outside the WAVE header and is unknown;
- the original pause policy (whether a paused session suspends its loops);
- the original radio priority, interruptibility, subtitle and completion
  semantics (F41-C);
- the decoded sample payload itself: retail members are almost all IMA/MS ADPCM
  and F06-C decodes PCM only, so `DecodedPcm` here is the *shape* of a future
  decode, never samples.

The declared fixture says so: `declared_synthetic_audio_catalog` is
`Origin::SyntheticFixture` with `designed()` provenance on every field, and
every id is under the `synthetic.` key. Resolving tasks: **F41-B** (decode,
loops and spatial emitters), **F41-C** (radio queue, music transitions and
subtitles), **F41-D** (original-media audit and real audible review).

## Known limitations that gate later stages (not silently dropped)

Affected content: the whole F41 audio path. Resolving tasks: **F41-B**,
**F41-C**, **F41-D**.

1. **No decoding and no audio device.** Nothing here opens a device, decodes a
   sample or mixes a signal; `DecodedPcm` is a shape and `AudioRouter` answers
   what *should* play. F41-B is the first stage with an actual mixer and F41-D
   the first with an audible claim.
2. **No runtime wiring into the ECS or an event stream.** `AudioRouter` is a
   pure, caller-driven object; no Bevy system feeds it, and `AudioEmitterBinding`
   is a record with no `Despawned`/swap handling system behind it. F41-B closes
   that loop.
3. **No radio, music-transition or subtitle logic.** Priority, interruption,
   queueing and music transitions are F41-C; the `Radio`/`Music` buses exist
   only as vocabulary here.
4. **Spatialization is absent.** `AudioEmitterBinding` carries no transform or
   listener; doppler, attenuation and stereo orientation are F41-B/D.
5. **The declared and runtime fixtures are parallel types, not yet asserted
   equal as data.** `declared_synthetic_audio_catalog` and
   `synthetic_weapon_one_shot`/`synthetic_engine_loop` mirror each other by hand.
   The bus vocabulary and gain ceiling are asserted equal, but the per-record
   data is not projected by a shared producer. F41-B asserts the mapping against
   real assets.
6. **ADPCM decoding is a separate, unfiled piece of work.** F06-C refuses an
   ADPCM member with its own declared tag; F41-B needs PCM, MS ADPCM and IMA
   ADPCM decoded before it can play a retail sound. See "Follow-ups".

## Mutation probes (tests fail when the behavior is removed)

Each probe was applied to the working tree, the named test was run, and the tree
was restored (verified by `diff` against the saved original). No probe was
committed.

1. The strictly-monotonic dedup check removed from `AudioRouter::play_one_shot`
   (every event accepted) →
   `accept_f41_a_a_replayed_weapon_event_plays_one_accepted_one_shot` panicked
   at line 55: `left: Accepted { .. }, right: SuppressedDuplicate { .. }`
   (`test result: FAILED`).
2. The foreign-session check removed from `AudioRouter::play_one_shot` →
   `accept_f41_a_events_from_another_session_are_refused` panicked at line 100:
   `left: Accepted { session: 12, .. }, right: RefusedForeignSession { .. }`.
3. The `UnknownBus` refusal in `lower_record` replaced by a guessed
   `AudioBus::Weapons` →
   `accept_f41_a_lowering_refuses_unknown_playback_fields` panicked at line 115:
   `left: Ok(LoweredAudioAsset { .., bus: Weapons, .. }), right: Err(UnknownBus
   { .. })`.
4. `lower_bus` mapped `Weapons → Music` →
   `accept_f41_a_every_declared_bus_lowers_to_its_runtime_twin` panicked at
   line 47: `left: "music", right: "weapons"`.
5. The duplicate-id check removed from `AudioCatalog::insert` →
   `accept_f41_a_catalog_refuses_duplicate_ids_and_foreign_kinds` panicked at
   line 100: `left: Ok(()), right: Err(DuplicateId { .. })`.
6. `PausePolicy::Continue` made to stop every loop replaced →
   `accept_f41_a_pause_policy_and_device_loss_stop_loops` panicked at line 203
   (the `Continue` assertion).

The content and runtime validation boundaries are likewise sensitive: the
in-crate tests `accept_f41_a_records_refuse_non_audio_ids`,
`accept_f41_a_levels_are_finite_non_negative_and_bounded`,
`accept_f41_a_inputs_refuse_corrupt_gain_and_foreign_kinds` and the integration
`accept_f41_a_levels_and_decoded_shapes_are_validated` fail if the kind, level
or decoded-shape validators stop refusing.

## Commands run

All four required checks, run from the repository root; exit codes as printed.

```
cargo fmt --all -- --check                                             -> 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -> 0
cargo test --workspace --locked                                        -> 0 (152 "test result: ok" after the rebase onto origin/main; 146 before it)
cargo test --workspace --locked -- accept_f41_a_ --include-ignored     -> 0 (28 tests selected, all passed)
```

The 28 selected tests are 4 unit + 5 integration in `cs_content`, 7 unit + 7
integration in `cs_sim`, and 5 integration in `cs_app`, all carrying the
`accept_f41_a_` prefix. No test is `#[ignore]`d, so `--include-ignored` selects
the same set.

## Evidence

Synthetic fixtures and design only. No original-data, visual, audible or
ordinary-play claim; this stage can award at most **checked**.

## Follow-ups

A retail sound cannot be played by F41-B until its AD(P)CM block encoding is
decoded: F06-C decodes PCM only and records that every retail member is IMA
ADPCM, MS ADPCM or 8-bit PCM (task #344 measured the `fmt ` shapes), and no
member declares loop points. That decoding work is independent of the F41
contract defined here. It is recorded as a prerequisite note on **F41-B**
(#163), which the spec already owns ("Implement decoding, loops and spatial
emitters"): F41-B either decodes both ADPCM variants or splits that decoding
into its own bounded task per `docs/TASK-SPLITTING.md`.

## Sources

- `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md` (`### F41-A`,
  acceptance test AC01, non-negotiable behaviors 1–5, "Research boundary"),
  `docs/contracts/IDENTITY-CONTENT.md`, `docs/01-ARCHITECTURE.md`,
  `docs/contracts/CLI-EVIDENCE.md`.
- `crates/cs_types/src/content.rs` (`ContentId`, `ContentKind`, `Origin`,
  `Provenance`, `Known`, `Resolved`), `crates/cs_types/src/evidence.rs`
  (`ClaimId`), `docs/research/FORMAT-NOTES.md`.
- `docs/findings/2026-09-28-f06-c-vfs-members-and-audio-assets.md` (PCM-only
  decode, `UnsupportedFormat` for ADPCM) and
  `docs/findings/2026-09-28-t344-zbd-sound-member-wave-headers.md` (retail
  `fmt ` shapes, no `smpl` chunk, cue points are positions not regions).
- The `damage` ↔ `cs_sim::damage` producer/consumer split and the
  declared-schema patterns in `crates/cs_content/src/config.rs` and
  `crates/cs_content/src/damage.rs`;
  `docs/findings/2026-09-30-f31-a-route-graph-and-maneuver-envelope.md`
  (findings template).
