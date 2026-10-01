# F41-B: loop lifecycle and spatial emitters (partial)

Date: 2026-10-01. Task F41-B. Capabilities: ordinary build/test only.

## Done

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

## Not done (unmet)

- **ADPCM decoding.** F06-C decodes PCM only; every retail member is IMA/MS
  ADPCM or 8-bit PCM. The decoder belongs in `cs_formats::zbd::sound_sample`,
  outside this task's owner paths, so it is filed as a follow-up task.
- Engine pitch/volume smoothing from throttle state, and a real mixer/device
  consuming `AudioSession::outcomes`, are not implemented.
- Loop regions are unknown (no `smpl` chunks); none were invented.
- The system is not yet registered in the app schedule (no `AudioSession` is
  inserted by the loading handoff); that is wiring for the mixer consumer.

The smoothing, mixer and schedule-registration items above are follow-up
task #445. Until #444 and #445 are done, no retail sound plays and F41-B
fidelity claims stay unmade.
