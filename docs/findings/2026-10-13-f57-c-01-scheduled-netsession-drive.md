# F57-C.01: the typed NetSession driven by the Bevy schedule

Date: 2026-10-13. Task: Rally #657 "Drive NetSession from the Bevy schedule
behind the session lifecycle" (follow-up to F57-C and the F54-C session
drive).
Capabilities used: ordinary build/test only. `network_local` loopback; no
original data read and nothing here asserts original networked behavior. All
of this is newly authored engine design.

## What this stage closes

The F54-C drive pumped the lifecycle and fed the mirror mailbox, but left
sampling, prediction, tracer expiry and the origin hook to this stage
(recorded as such in `2026-10-13-f54-c-session-drive-binding.md`). F57-C's
typed `NetSession` consumers existed only under test; nothing in production
called them. This stage wires them into `FixedPostUpdate` as an explicitly
chained set — `NetSessionSet::{Origin, Drive, Sample, Correct,
ExpireTracers}` ordered after `PhysicsSystems::StepSimulation` — installed by
`NetSessionDrivePlugin`:

1. `drive_net_origin` advances the real `crate::origin::FixedTickDriver`
   (`NetOriginDriver`, a `Resource` newtype) once per committed tick. A
   policy rebase produced by `advance_frame` is adopted by `NetHost`,
   `NetClient` and the live mirror through `NetSession::rebase`, so host,
   client and mirror can never sit in different epochs. If the driver
   restarted *below* the mirror's epoch the mirror is torn down rather than
   fed a frame it cannot trust. The marked pose owner's `Position` is
   converted through canonical world space (`world_of`/`local_of`) — the
   rebase re-anchors coordinates, it is never applied as a physical
   displacement.
2. `drive_net_session` (the F54-C pump) drains the latest-wins snapshot
   mailbox into `NetSession::ingest` at most once per `(session, tick)` —
   refeeding would replay one frame as if it arrived twice. The wire's
   `ActorSpawned` event whose `owner` is the granted peer names the local
   actor for the Correct stage.
3. `sample_net_mirror` runs `NetSession::sample` at the committed tick into
   `NetPresentation` (a `Resource` map of actor → interpolated state). The
   local actor is never interpolated — the predictor owns it.
4. `correct_local_pose` finds the *one* `NetPredictedBody` marker and
   reports the count by name (`NetPoseOwner::{Unbound, Bound, Unposed,
   Ambiguous}` — two markers are a reported ambiguity, not a picked winner).
   It records the predicted pose in canonical world coordinates, runs
   `reconcile_local` only when this run's ingest actually moved the local
   record, applies `NetSession::next_correction` through the pose owner's
   `Position`/`Rotation`, and enacts an unbounded `Snapped` verdict
   honestly: the owner adopts the record's pose outright (`adopted_pose`).
   Session state that outlives the mirror lives in `NetDriveState`
   (`local_actor`, `attach_refused`, the fed-mailbox stamp).
5. `expire_net_tracers` expires client tracers at the committed
   `PhysicsTickLedger` tick, so a shot nobody answered cannot pin a tracer
   forever.

Session end, lifecycle `Closed` and mirror replacement all route through
`NetSession::teardown` and are reported as `NetTeardown`s by side and
generation — a reconnect under a new session id builds a fresh mirror with
empty aircraft, no predictor and no tracers. Every refusal family —
`SessionError`, `IngestRefusal`, `PublishError`, `SnapshotError`,
`BufferRefusal`, conversion faults — lands in `NetSessionReport`
(`mirror_refusals` aggregates them as strings because the report is the one
caller-visible channel across several error types); nothing is dropped
silently.

## Discovery: `LocalPredictor::reconcile` could never match a live wire

The first scheduled run exposed a defect in F57-C's `LocalPredictor::reconcile`
(`crates/cs_app/src/network/physics.rs`). When no predicted pose existed at a
record's tick it did `history.clear()` — wiping *all* entries, including the
ones **newer** than the record's tick. On any wire with positive transit the
record's tick always lags the just-committed tick, so the entry that would
have matched the *next* record was destroyed by the current clear: every
subsequent reconcile found front > record.tick, cleared again, and repeated
forever — a permanent infinite-`Snapped` loop that `next_correction` could
never interrupt with a bounded correction. Prediction history became
permanently unusable after the first unmatched record.

The fix keeps the `while`-pop of entries older than `record.tick` (they can
never match a later record — ticks are monotone) and drops the `clear()`:
entries newer than the record's tick are exactly the predictions later
records compare against. The unmatched record still reports `Snapped` with
unbounded error and the pose owner adopts the record outright — the honest
enactment is unchanged. Verified by `accept_f57_c_01_the_scheduled_drive_
ingests_samples_corrects_and_expires`: with the `clear()` in place, 600
committed ticks of live loopback traffic produced nothing but infinite snaps
and `corrected` stayed `None`; without it the first finite compare snaps the
spawned-at-origin body the ~317 m onto the record and a later +30 m
displacement is paid back as a bounded `-30 m` correction. All 25
`network::` unit tests — including the F57-B predictor acceptance set — pass
unchanged, so the narrower clearing was unreachable dead code on every path
they exercise.

## Observable failure without the implementation

`accept_f57_c_01_without_the_plugin_the_schedule_drives_nothing` builds the
same loopback world without `NetSessionDrivePlugin`: no report is produced,
no handshake completes, no mirror is built and no `NetPresentation` exists —
the schedule alone drives nothing, so the wiring is what the tests measure,
not a fixture-side replica.

## Files

- `crates/cs_app/src/network/session.rs` — `NetSessionSet`, the five
  systems, `NetOriginDriver`, `NetPresentation`, `NetPredictedBody`,
  `NetDriveState`, `NetPoseOwner`, `NetOriginStep`, `NetTeardown`, the
  report fields, `NetHost::reopen`, `NetClient` prediction config and
  `NetHostError::Reopen` (`NetHostError` loses `Copy`: `ServerFault` is not
  `Copy`).
- `crates/cs_app/src/network/physics.rs` — the `reconcile` history fix
  described above.
- `crates/cs_app/tests/accept_f57_c_01_schedule.rs` — five acceptance tests
  over a real loopback pair on a production `PhysicsSession`: the scheduled
  ingest/sample/correct/expire chain (including a second marker refused as
  `NetPoseOwner::Ambiguous`), the real origin driver moving host, client,
  mirror and pose owner into one epoch with a post-rebase snapshot still
  applying, disconnect teardown plus fresh-generation reconnect, refusal
  visibility for every family, and the no-plugin negative.
- `docs/findings/2026-10-13-f57-c-01-scheduled-netsession-drive.md` — this
  note.

## Checks

- `cargo fmt --all -- --check` — clean.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — clean.
- `cargo test --workspace --locked` — all pass (548 harness results, zero
  failures).
- `cargo test --workspace --locked -- accept_f57_c_01 --include-ignored` —
  5 tests discovered, all pass.
