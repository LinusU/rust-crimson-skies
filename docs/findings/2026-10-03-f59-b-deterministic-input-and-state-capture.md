# F59-B: deterministic input and state capture — what was built, what is measured, what stays open

Date: 2026-10-03. Task: F59-B "Implement deterministic input and state capture"
(`specs/F59-replays-captures-probes-and-acceptance-evidence.md`,
`### F59-B`). Capabilities used: ordinary build/test only — no `retail`, no
`gpu`, no `audio`. Everything below is newly authored synthetic design: the
airframe it flies is `cs_sim::flight::synthetic_fixed_wing`, carries
`Origin::SyntheticFixture`, and no value in this stage is derived from the
original game or can certify anything about it.

## Files and the observable failure

- `crates/cs_app/src/capture/mod.rs` (new, the module): `identity`, `state`,
  `replay`, `render`, and the re-exports.
- `crates/cs_app/src/capture/identity.rs` (new): `LoadedContent`
  (`from_manifest`, `of_record`, `insert`, `with_row`, `row`, `len`,
  `is_empty`, `keys`, `digest`), `airframe_content_digest`, `rules_digest`,
  `engine_digest`, `host_platform`, `RunIdentity`, `AIRFRAME_CONTENT_KEY`,
  `STATE_FORMAT_VERSION`.
- `crates/cs_app/src/capture/state.rs` (new): `StateReading` (`at_spawn`,
  `after_tick`, `digest`, `non_finite_field`), `StateProbe` (`start`,
  `measure`, `initial`, `envelope`, `last_tick`, `is_empty`, `initial_state`),
  `StateProbeError`, `STATE_DIGEST_DOMAIN`.
- `crates/cs_app/src/capture/replay.rs` (new): `BuildContext`, `ReplaySubject`,
  `RunRequest`, `RecordedRun`, `ReplayOutcome`, `CaptureRunError`,
  `record`, `record_run`, `replay`.
- `crates/cs_app/src/capture/render.rs` (new): `settings_for`, `render_for`,
  `tonemap_for`, `tonemap_label`.
- `crates/cs_app/tests/accept_f59_b_replay_capture.rs` (new, 17 tests, prefix
  `accept_f59_b_`).
- Wiring only: `pub mod capture;` plus one module-doc paragraph in
  `crates/cs_app/src/lib.rs` (after the F57 paragraph — the crate-doc order
  guard `accept_doclib_conflict` enforces ascending feature-sheet order).
- Two additions to `crates/cs_content/src/replay.rs`, both needed by the
  boundary and both in this feature's owner paths:
  `ReplayError::MissingInitialState` (a runtime that skipped measuring a start
  must say so instead of inventing one) and
  `CaptureError::RenderSettingUnpinned` (a renderer setting the pinned
  `RenderConfig` has no field for — currently the internal render resolution).

### Observable failures without the implementation

- A changed content asset is accepted as the same run and its old replay keeps
  replaying (`accept_f59_b_a_changed_content_asset_rejects_the_old_replay`).
- Two runs of one input stream report the same per-tick state hashes even when
  the worlds differ (`accept_f59_b_the_state_hash_follows_the_world_not_the_input`).
- A recorded `fire_primary` edge is silently dropped and the replay promises the
  state of a run whose press never happened
  (`accept_f59_b_a_recorded_edge_this_run_cannot_execute_is_refused`).
- A recorded frame past the last tick is dropped from the promise
  (`accept_f59_b_a_recorded_tick_outside_the_run_is_refused`).
- A non-finite tick state is hashed into the promise
  (`accept_f59_b_a_non_finite_state_is_refused_rather_than_hashed`).
- A renderer configuration with an internal resolution override lowers into a
  record that claims a frame it does not describe
  (`accept_f59_b_the_render_lowering_round_trips_and_refuses_an_unpinned_setting`).

## Decisions

- **The flight world is the run.** F59-B needed a production session that an
  input stream can drive and a state can be measured from. The workspace already
  has exactly one: `cs_app::physics::PhysicsSession` hosting
  `FlightForcesPlugin`, `spawn_flight_body` and `cs_sim::flight::FlightModel` —
  the same code F23-D's `stability_probe` flies. The capture path adds no
  simulation: it drives that world from a recorded `CommandStream` through
  `cs_sim::control::ControlBuffer` (the production input boundary `cs_app::input`
  already uses) and reads the state back out of it. Deleting the recorder would
  make the acceptance tests fail on missing state, not on a missing stub.
- **The state hash is a measurement or it is nothing.** A `StateReading` can
  only be built from a `PhysicsSample` read out of Avian and the `FlightOutput`
  the tick's own law computed. There is no field a caller can fill with "what the
  input said", so a hash cannot agree with the promise merely because the input
  was copied into it. `accept_f59_b_the_state_hash_follows_the_world_not_the_input`
  states that as a property: the same stream over two different worlds produces
  two different envelopes.
- **The content digest is F02's, not a second one.** `LoadedContent::digest`
  hashes the loaded rows' digests in logical-key order with **no**
  domain-separation prefix, which makes it byte-for-byte
  `cs_assets::install::content_fingerprint(manifest)` for a manifest-derived set.
  A prefixed copy would have been a second definition that agrees with the first
  nowhere, which is the drift F59-B exists to remove: the `content_sha256` in a
  replay record, in an evidence artifact and in `cs_assets` is then one number.
  The record's compatibility signature is domain-separated separately in
  `cs_content::replay`, so nothing reads this digest without that separation.
  `accept_f59_b_installed_content_digests_exactly_as_f02_does` pins it, including
  the host-root and one-byte-edit properties.
- **Content and rules are disjoint.** `airframe_content_digest` covers the
  coefficients (mass, inertia, engine, boost, drag, lift, stall, attitude
  response, assist gains, wing area) and the record's `origin`.
  `rules_digest` covers the evaluation context (which law, which handling
  profile, whether assists may contribute, the fixed rate). Folding the profile
  into the content digest would make a fidelity→improved switch report as a
  content change, which is exactly the distinction AC02's refusal is worth
  having; `accept_f59_b_a_changed_handling_profile_is_a_rules_difference` and the
  in-module `an_edited_coefficient_is_content_not_rules` hold both directions.
- **The engine digest names the engine, the tree names the sources.**
  `engine_digest` covers the crate version, the toolchain string the caller
  recorded and `STATE_FORMAT_VERSION`; a rebuild on another toolchain or a
  changed reading shape is a different engine. Source changes move `BuildId`
  (the candidate tree). No code reads `CS_CANDIDATE_TREE` or
  `CS_CAPABILITIES` from the environment — that is F59-C's command wiring, and
  `BuildContext` takes the coordinates as arguments so the value in the record is
  one the caller can prove.
- **Compatibility gate and state comparison are separate answers.** `replay`
  flies the record's own stream in a fresh world, builds the candidate record
  that run would produce, and reports `CompatibilityVerdict` beside
  `EnvelopeComparison`. A content change is named as `Content` even though the
  state moved too, and the state comparison is still reported. Folding them into
  one verdict would lose which question a divergence answered.
- **No policy is defaulted.** `replay` takes the caller's `CrossBuildPolicy`.
  `Reject` is F59's initial determinism target; `BestEffort` yields a verdict
  that names the differences and certifies no determinism, so a caller that
  reaches for it has to say why in whatever it does next.
  `accept_f59_b_best_effort_names_the_difference_and_certifies_nothing` holds
  both halves.
- **An input the run cannot execute is refused.** The fixed-wing path hosts one
  aircraft and its law, with no weapon, ordnance, gear, target or menu system, so
  every `Action` has no consumer here. `CaptureRunError::UnconsumedAction` names
  the tick and the action instead of dropping the press. This is a real
  limitation of the slice, not a modelling choice, and it is stated in the
  module documentation so a later stage knows to add consumers rather than
  relax the refusal.
- **A frame outside the run's ticks is refused.** A recorded tick 0 (before any
  input boundary) or past the last tick would never be applied.
  `check_stream_range` rejects the run instead of writing a record whose promised
  stream is longer than the run it came from.
- **A non-finite reading is refused, not hashed.** `NaN` bits compare unequal to
  themselves and a digest over them describes nothing, so `StateProbe` names the
  offending field and leaves the envelope untouched.
- **Run bookkeeping goes through the record.** The run's purpose, named debug
  overrides and profile-write request come from the subject's `OverrideLog` and
  land in the record. Nothing here writes a profile at all, so the record's
  `profile_write=` line is a claim about the request, not about a side effect;
  a tooling run that asked for one is visible as `violates_profile_rule()`.
- **The `RenderConfig` ⇄ `ComparisonSettings` lowering lives in one place.**
  `settings_for` / `render_for` / `tonemap_for` / `tonemap_label` are the only
  code that crosses between the record's `u32` thousandths and the renderer's
  `f32`s. The comparison baseline round-trips to itself (F59's `1000`/`2200`/`1`
  are F17's `1.0`/`2.2`/`1`), the two tone-curve vocabularies share their
  `none`/`filmic` labels, and a setting the record cannot pin is refused with
  `CaptureError::RenderSettingUnpinned` rather than dropped.
- **Tick 0 is the initial state, ticks 1..=n are the envelope.** The record's
  `first_tick` is `0` and its `last_tick` is the run's tick count, so a recorded
  input frame anywhere in `1..=n` is inside the declared range and the envelope's
  entries are. The initial state is the pose read before the first tick ran, with
  the label `<subject>@tick0` so a reader can tell a start-of-run measurement
  from a measurement taken after one step.
- **Floats go in as bits.** `StateReading::digest` writes every `f32`/`f64` as
  its IEEE-754 bit pattern, so `0.1` and `0.10000000000000001` — the same double —
  are the same state, and no rounding of a decimal spelling can make two runs
  disagree or agree.

## Sensitivity of the tests (mutations actually run and reverted)

- `airframe_content_digest` dropping `mass.mass_kg` → 2 tests fail
  (`..._a_changed_content_asset_rejects_the_old_replay`,
  `..._best_effort_names_the_difference_and_certifies_nothing`).
- `StateReading::digest` keeping only the pose and dropping the tick's forces →
  `..._a_state_hash_covers_the_tick_and_the_forces_it_computed` fails. (First
  tried with an unverified mutation that left the build broken; the mutation was
  rewritten and re-run.)
- `StateProbe::measure` recording one repeated hash instead of the measured one →
  4 tests fail, including AC01's replay-twice case and the state-follows-the-world
  case.
- The recorder reading a constant `FlightOutput` instead of
  `FlightAircraft::last_output` → `..._every_promised_hash_is_over_this_tick_s_own_measurement`
  fails. This mutation initially passed: the pose still varied with the input, so
  the digest tests were satisfied by the wrong half of the reading. The test was
  added for it and checks the two halves against each other — the airspeed the
  law reports against the speed Avian integrated, `q = ½ρV²` against the reported
  airspeed, and that consecutive ticks do not report one repeated value.
- Removing the `UnconsumedAction` refusal → 1 test fails.
- Removing the stream-range check → 1 test fails.
- Removing the non-finite check in `StateProbe::measure` → 1 test fails.
- `replay` returning `CompatibilityVerdict::Compatible` unconditionally → 3 tests
  fail (AC02, the rules/content distinction, and the best-effort case).

All 17 `accept_f59_b_` tests are fast (the whole file runs in well under a
second) and none is ignored: the stage needs ordinary build/test only.

## Open / not claimed (resolving stages)

- **Nothing captures bytes.** No GPU render, no screenshot writer, no PCM write
  and no offscreen path exist behind `CaptureRecord`; this stage measures state
  and lowers render settings, it does not produce an image. AC03's runtime half
  ("capture at fixed tick from fixed camera on two runs") is F59-C's minimum
  scenario. Resolving: F59-C/F59-D.
- **No command line, no file.** `--input-replay`, `--cam` and `--screenshot`
  from `docs/contracts/CLI-EVIDENCE.md`, the `cs-inspect`/`cs_xtask` evidence
  commands, reading `CS_CANDIDATE_TREE`, and writing `acceptance.json` are all
  F59-C/F59-D. Resolving: F59-C, F59-D.
- **Only the fixed-wing law has a consumer for input here.** A recorded stream
  carrying a weapon, ordnance, gear, target or menu press is refused by name
  (`CaptureRunError::UnconsumedAction`). Those consumers exist elsewhere in
  `cs_app` (weapons, ordnance, `ai::navigation`); wiring them into this run is
  later work, and the refusal is the correct answer until then.
- **`ReplaySeeds` carries a root and no derived streams here.** The flight path
  consumes no random stream, so `ReplaySeeds::new(seed, vec![])` is what
  `record` writes. The synthetic body's domain-separated stream is the only one
  a replayed flight run can name today; a consumer that adds one must use
  `ReplaySeeds::derive` with its own documented domain constant so adding it
  never moves another's values.
- **Authored choices are caller-supplied.** `ReplaySubject::choices` is whatever
  the caller pinned; the recorder does not derive an airframe or ruleset choice
  from the tuning it loaded. A stage that knows the catalog should derive them.
- **Unmeasured and therefore unclaimed:** the original game's tick rate, whether
  it had a replay or capture format at all, what its evidence artifacts looked
  like, its throttle axis mapping (the affine `(axis + 1) / 2` used here is
  project design and is recorded as such in `flight_command`'s documentation),
  any original value for a render setting, and any original airframe coefficient.
  The airspeed tolerance used in the airspeed cross-check is a test bound for
  sea-level cruise, not a declared engine tolerance.
- **The `engine` digest's scope is a design decision.** It covers the crate
  version, toolchain and state-format version; source changes move `tree`. If a
  later stage decides the engine digest must cover more, that changes which
  difference a build reports — recorded here so the choice is visible rather than
  accidental.

## Note on where this feature's `tests/` owner path lives

The F59 sheet lists `tests/` as an owner path. The workspace root is a virtual
manifest (`Cargo.toml` has `[workspace]` only, no `[package]`), so a root
`tests/` directory is never compiled and could not carry the required
`cargo test --workspace --locked -- accept_f59_b_` selection. F59-A set the
precedent for this feature by placing `accept_f59_a_replay_schema.rs` in
`crates/cs_content/tests/`, and this stage follows it with
`crates/cs_app/tests/accept_f59_b_replay_capture.rs` — the `cs_app` crate owns
the production code under test. Flagged rather than silently reinterpreted, in
case the owner wants the sheet's owner paths rewritten to say
`crates/*/tests/`.
