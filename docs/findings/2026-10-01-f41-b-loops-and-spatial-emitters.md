# F41-B: loop lifecycle, spatial emitters, smoothing and mixing

Date: 2026-10-01 (updated 2026-10-02 by task #445, 2026-10-10 by task #531).
Tasks F41-B, then its follow-ups #445 and #531. Capabilities: ordinary
build/test only. No original audio was read; every value below is authored
project data.

## Done in F41-B

- `crates/cs_app/src/audio/loops.rs`: `AudioSession` resource and the
  `sync_emitter_loops` system. Added `AudioEmitterBinding`s bind their asset's
  loop; despawned ones stop it as `Despawned`; removals run before additions;
  a predecessor's teardown never stops a loop a replacement already bound to
  the same emitter; a stale scene generation, an unknown asset and a one-shot
  spec are refused by name and recorded.
- `crates/cs_sim/src/audio_events.rs`: `SpatialPolicy`, `Listener`,
  `spatialize` → `SpatialMix { gain, pan }`. **Designed** inverse-distance law
  and right-axis pan; the original attenuation/doppler is unmeasured and
  nothing here claims it.
- Tests: `cs_app/tests/accept_f41_b_engine_loop_rebind.rs`,
  `cs_sim/tests/accept_f41_b_spatial_emitters.rs`. Minimum scenario:
  `accept_f41_b_destroyed_aircraft_ends_loop_and_new_aircraft_binds`.
  Sensitivity probe: removing the "is still my loop" guard in `unbind` fails
  `accept_f41_b_replacement_on_same_emitter_survives_predecessor_teardown`.

## Done in #445 (this task)

- **Engine pitch/volume smoothing from throttle state.**
  `cs_sim::audio_events::EngineSmoothing` is the designed law (attack 2.0/s,
  release 1.5/s, pitch sweep 3.0/s, spool spanning gain 0.25–1.0 and pitch
  ratio 0.7–1.6); `EngineVoice` is the smoothed state, moved only by
  `EngineVoice::advance`. `cs_app::audio::engine` runs it in `FixedUpdate` from
  `Time<Fixed>` and the authoritative `cs_sim::flight::EngineState` spool the
  F24-B driver integrates, so the mix follows measured engine state rather than
  a render delta. The step is a clamped linear ramp, so equal elapsed time
  reaches the same level however it was divided; a stopped engine asks for
  silence.
- **A mixer/device consumer.** `cs_sim::audio_events::AudioDevice` is the whole
  output boundary (open, start, update, stop, close) and `AudioMixer` consumes
  `AudioSession`'s `LoopOutcome`s plus `spatialize`, sending voice commands with
  `loop gain × engine gain × spatial gain`. It never consults the device for a
  decision, so a device that refuses everything changes nothing in the
  simulation. `cs_app::audio::mixer` holds the device (`AudioOutput`), the
  listener and policy (`AudioSpatial`), runs `mix_session` after
  `sync_emitter_loops`, and exposes `device_lost`/`device_restored`, which stop
  every voice, close the device and let the session retry — while radio timing
  and mission state stay untouched (F41 non-negotiable behavior 2). The session
  is the authority on whether an output exists, so **no mix pass runs while the
  device is lost**: `AudioMixer::mix` opens a closed device on demand, and
  without that gate the frame after `device_lost` re-opened the device nobody
  had restored, leaving nothing but a `Closed` and an `Opened` a frame apart in
  the device log. Both halves of the device-failure path are total in the
  *device*, not only in the mixer: a world with an output and no mixer (nothing
  loaded) still closes and re-opens it.
- **Loading handoff and schedule registration.**
  `cs_app::audio::handoff::insert_audio_session` installs the `AudioSession`
  (and the session's `AudioMixer`) that a delivered F15 load owns: the session
  generation is the delivered closure's, the scene generation is the load
  path's `SceneGenerations::latest`, and the specs are the delivered audio
  content lowered through `lower_record` — so a binding for an asset the load
  did not deliver is refused as `UnknownAsset`. `AudioPlugin` registers
  `insert_audio_session` in `PreUpdate`, `smooth_engine_voices` in
  `FixedUpdate`, and `sync_emitter_loops → advance_radio → mix_session` in
  `Update`. A reload replaces the session and releases the replaced mixer's
  voices, so the previous load cannot stay audible. The handoff reads the
  delivered bindings in three passes over the query and clones no binding: a
  `LoadedItemBinding` owns four `String`s, so collecting the world's delivered
  items into a `Vec` every `PreUpdate` would allocate for every mesh and texture
  in the level to look at the handful that are audio.
- Tests: `cs_app/tests/accept_f41_b_audio_wiring.rs` (8 scenarios, production
  plugin + real F15 handoff + real flight body + real fixed clock) and
  `cs_sim/tests/accept_f41_b_engine_voice_smoothing.rs`. Measured probes:
  unregistering `smooth_engine_voices` fails the throttle scenario; dropping the
  clamp in `EngineVoice::advance` fails two `cs_sim` scenarios; disabling the
  handoff install fails 7 of the 8 app scenarios; stopping the mixer from
  draining the session's outcomes fails 4.

## Done in #531 (F41-B2, this task)

- **The spawn/bind path.** `crates/cs_app/src/audio/spawn.rs`,
  `bind_spawned_emitters`, is now the one producer of `AudioEmitterBinding`.
  It is an exclusive system registered by `AudioPlugin` in `Update`, chained
  **before** `sync_emitter_loops`, so a frame that spawns an aircraft binds
  its loop and mixes it in that same frame. It attaches only to entities that
  carry no binding yet, so an emitter entity keeps one id for its whole life,
  and a binding inserted by anyone else is left exactly as it is.
- **Who it binds, and from what.** Two roles, both filled by spawn paths that
  already existed:
  - an **engine** emitter on every aircraft carrying an engine authority the
    workspace spawns — `physics::FlightAircraft` (the designed F24 law) *or*
    `playtest::scene::PlaytestOriginalFlight` (the recovered original law the
    mission's `spawn_player` body carries) — together with
    `EngineVoiceFollow`, so its level follows throttle;
  - an **environment** emitter on the installed load's own delivered item
    entity whose declared record is the environment loop (the entity F15's
    `ReadyBundle::attach` spawned), which is how the world's ambient loop
    gets an emitter without inventing content or a position.

  An engine loop belongs to an aircraft, music is the F41-C director's bus
  and the remaining cues are one-shots, so none of those become world
  emitters.
- **Engine voices follow both authorities now.** `smooth_engine_voices` reads
  whichever of the two records its body carries — the same pair
  `spin_propellers` reads — instead of `FlightAircraft` alone, so a mission
  player body flying the original law has a following engine voice rather
  than a fixed one.
- **Stable session-qualified emitter ids.** `AudioEmitterIds` wraps
  `cs_types::net::ActorAllocator`: serials start at 1, are monotonic inside
  the installed session and are never recycled, so one id names one emitter
  entity for as long as it lives and a replacement spawn takes a fresh serial.
  The resource is created from the installed session and replaced whenever
  the session generation changes, so an id minted for a replaced load can
  never alias a live emitter of the new one.
- **Content is never invented here.** A binding's asset is resolved
  *delivered first*: the lowest-id spec the installed load actually lowered
  for that role's bus and mode (the new `AudioSession::specs()`), and only
  then the lowest-id declared record of `DeclaredAudioCatalog` for the role.
  The second case is what keeps the refusal honest: the binding names a
  `cs_content::audio` record and `sync_emitter_loops` refuses it by name as
  `LoopRefusal::UnknownAsset`, instead of the emitter quietly missing from
  the mix. `AudioBindLog` records every attachment (entity, emitter, asset,
  bus, role) and every refusal, at most once per session rather than once per
  frame.
- Tests: `crates/cs_app/tests/accept_f41_b_spawn_bindings.rs`, 7 scenarios,
  prefix `accept_f41_b_`. **No fixture spawns a binding**: the aircraft enters
  through the production `spawn_flight_body`, the world's emitter is the
  entity the production handoff spawned, and every loop asserted on the
  device got there through `AudioPlugin`'s own schedule. Minimum scenario
  through this path:
  `accept_f41_b_destroyed_aircraft_ends_loop_and_replacement_binds`; throttle
  scenario: `accept_f41_b_spawn_path_engine_voice_follows_the_aircraft_throttle`.
  One pre-existing assertion was *sharpened*, not relaxed:
  `accept_f41_b_undelivered_sound_is_refused_by_name` used to assert global
  silence while the load delivered the environment loop — which this path now
  correctly plays — and now asserts that the refused engine asset reaches no
  device voice at all while exactly the delivered loop sounds.

## Still not done / still unmeasured (gates fidelity claims)

- **The world's emitter is placed nowhere.** The environment loop now binds
  and reaches the device, but the entity it binds is the delivered item
  entity, which carries no `Transform`: `AudioMixReport::unplaced` reports it
  and the designed spatial law never places it — it keeps the mix its voice
  started with. Deciding whether the world's ambience is positional, and if
  so which spawn path gives it a pose, is **F41-B3 (#1277)**; nothing here
  guesses a position. Affects every world/environment loop, synthetic and
  retail alike.
- **Over retail content this path still binds nothing.** Every retail sound
  row's bus, level and playback mode are still `Resolved::Unknown`
  (F14-D.7, `CLAIM_SOUND_PLAYBACK`), so `lower_record` refuses them, no role
  resolves from the delivered specs and the declared fallback finds no record
  that can state one either: `bind_spawned_emitters` records
  `NoRoleAsset { Engine }` and attaches nothing. No retail aircraft sound,
  gunfire loop or environment loop can play until those fields carry a
  measurement from an original run — F41-D, with the catalog stage it depends
  on. Affects the sound rows of `ZBD/soundsl.zbd` and `ZBD/soundsh.zbd`.
- **Loop regions are unknown.** No retail member carries an `smpl` chunk, so no
  loop start/end was invented; the loops bind whole assets and loop seams stay
  unevidenced until F41-D has an original run to measure.
- **Attenuation, pan and doppler are designed, not measured.** The
  inverse-distance law and right-axis pan are project design; the original's
  curve is unknown and nothing here claims it.
- **No hardware audio device.** `RecordingAudioDevice` records what the mixer
  asked for and makes no sound. It is not evidence that a user heard anything
  (F41 non-negotiable behavior 5), and an audible original-media review needs
  the `audio` capability — that is F41-D.
- **No retail sound has played.** ADPCM decoding is #444; this task mixed
  whatever the catalog declared, which for retail means nothing until a decode
  and an audible review exist.