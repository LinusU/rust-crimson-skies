# F57-A: snapshot schema and quantization budgets — design notes and what stays open

Date: 2026-10-01. Task: F57-A "Define snapshot schema and quantization budgets"
(`specs/F57-networked-aircraft-prediction-interpolation-and-projectiles.md`,
stage `### F57-A`). Contracts read: `docs/contracts/FLIGHT-PHYSICS.md`,
`docs/contracts/UI-NETWORK.md`. Capabilities used: ordinary build/test only; no
`CS_GAME_DIR` read, no original data, no evidence report.

Every number in this slice is newly authored engine design. No original network
budget, packet layout, interpolation delay, remote-aircraft presentation or
lifecycle vocabulary has been measured, and nothing here asserts any. A pass on
this stage awards **checked** at most.

## Files and the observable failure

- `crates/cs_net/src/snapshot.rs` (new, the F57-A payload schema inside
  `SnapshotFrame::payload`): `Quantization` and the six declared budgets in
  `SNAPSHOT_BUDGET`, `QuantizedVector`, the smallest-three `QuantizedRotation`,
  the `FlightChannel`/`DamageChannel`/`WeaponChannel` records, `ActorRecord`
  (the fixed 66-byte record), `Snapshot` with `encode`/`decode`/`validate`/
  `into_frame`/`from_frame`, `OriginEpoch`, the wire code enums
  (`Lifecycle`, `ControlMode`, `Bank`) and the synthetic fixture.
- `crates/cs_sim/src/net_state.rs` (new, the server-side authority):
  `NetStateLedger` with `spawn`/`publish`/`record_destruction`/
  `end_lifecycle`/`observe_input`/`forget`, `ActorGeneration`,
  `NetLifecycle`, `NetControlMode`, `NetActorState` with `validate`.
- `crates/cs_app/src/network/physics.rs` (new, the receiver boundary):
  `publish_actor`/`publish_snapshot`, `RemoteMirror::ingest`,
  `RemoteMirror::apply_event`, `RemoteAircraft::from_record`, `IngestReport`,
  `IngestOutcome`, `IngestRefusal`, `PublishError`.
- Tests: `crates/cs_net/tests/accept_f57_a_snapshot_schema.rs` (9),
  `crates/cs_sim/tests/accept_f57_a_net_state_authority.rs` (7),
  `crates/cs_app/tests/accept_f57_a_latency_loss_and_reordering.rs` (8).
- Wiring edits only: `pub mod snapshot;` plus a doc paragraph in
  `crates/cs_net/src/lib.rs`; `pub mod net_state;` plus a doc paragraph in
  `crates/cs_sim/src/lib.rs`; `pub mod network;` plus a doc paragraph in
  `crates/cs_app/src/lib.rs`; `crates/cs_app/Cargo.toml` gains the `cs_net`
  dependency `src/network/physics.rs` needs (and the root `Cargo.lock` with
  it); `crates/cs_app/src/network/mod.rs` is the module doc + `pub mod physics;`
  declaration.
- Observable failure without the implementation: there is no schema for a
  snapshot payload at all. F54-A's `SnapshotFrame::payload` is opaque bytes, so
  `Snapshot::decode` does not exist, there is no declared quantization for any
  field (so no error budget to check a mirrored pose against), and there is no
  rule that stops a reordered late snapshot from rewinding a remote aircraft or a
  replayed destruction event from retiring it twice.

## The declared budgets

`SNAPSHOT_BUDGET` is a table, not a set of constants scattered through the codec,
so a consumer audits it instead of trusting the values it happens to use.

| Field | Scale | Width | Error budget |
| --- | --- | --- | --- |
| position | 1/64 m | i32 signed | 7.8125 mm |
| linear velocity | 0.05 m/s | i16 signed | 25 mm/s |
| angular velocity | 1/512 rad/s | i16 signed | 0.977 mrad/s |
| unit fraction (throttle, spool, boost) | 1/65535 | u16 | 7.6e-6 |
| integrity | 1/1000 | u16 | 0.5 per mille |
| rounds | 1 | u16 | 0.5 round (exact for integers) |
| orientation | smallest-three | 7 bytes | 9.2e-5 rad |

Derived, not chosen to make a test pass: position is ±33 554 km from the origin
epoch, so a world-scale distance is representable while the step stays at 7.8 mm;
velocity ranges (±1638 m/s, ±63.997 rad/s) cover any airframe F24's model can
produce with headroom. The rotation budget is a first-order bound stated in the
module doc (`sqrt(3)e + 3e` perturbation of the unit quaternion, rounded up to
`6e`); the acceptance test measures the worst case over a deterministic sweep
and separately proves the dropped component is always a largest one and keeps its
sign, which is what a wrong index or a lost sign would break.

Three properties make the budgets claims rather than comments:

1. **Encoding refuses, it never saturates.** A value outside a declared range or
   a non-finite value is a named refusal; so is a declared width this schema
   cannot store. Nothing is rounded into range.
2. **Decoding refuses.** A stored integer that does not fit its declared width, a
   session field of zero, an unknown lifecycle/control/bank code, an integrity
   above 1000 per mille, and any truncation at any offset are all refused by
   name. `Cursor` never reads past the end.
3. **The measured error is checked against the declared budget**, and the sweep
   also asserts the measured error *approaches* half a step, so a quantizer that
   dropped precision entirely could not pass.

## The F57 acceptance criteria in this stage

- **AC01** (latency/loss/reordering; no duplicate destruction, no permanent
  ghost): `accept_f57_a_latency_loss_and_reordering_leave_no_duplicate_destruction_and_no_ghosts`
  runs 200 ticks x 9 designed conditions (0/50/150 ms RTT x 0/2/10 % loss, the
  150 ms rows with a 3-tick reorder window) x 2 seeds. Under every one: the
  authority awards exactly one destruction for the destroyed actor however many
  times it is reported, the snapshot path retires each leaving actor at most
  once, the reliable removal is applied exactly once, and the mirror holds only
  the survivor at the end. The loss and reordering rows also assert the link
  really dropped and really reordered, so the scenario cannot pass on a perfect
  link.
- **AC02** (a correction during local boost keeps ammo/fuel authoritative):
  `accept_f57_a_client_input_leaves_rounds_and_boost_authoritative` — 38 accepted
  client input sequences change only `input_ack`; rounds and boost capacity reach
  the mirror unchanged, and only a server state change moves them. F57-B owns the
  prediction half of AC02; what is fixed here is which fields stay authoritative.
- **AC03** (origin change produces no world-scale jump):
  `accept_f57_a_a_foreign_origin_epoch_is_refused_and_a_rebase_is_not_a_jump` —
  a snapshot in a foreign epoch is refused whole with nothing changed, and after
  a rebase that moves the origin 200 km the mirrored world position is inside
  the 7.8 mm position budget. The epoch-relative conversion is also asserted in
  the latency scenario itself, against the authority's own value for the record's
  tick, so a publisher that accidentally sent absolute coordinates fails there
  too (verified: mutation below).
- **AC04** (recycled ids never share history):
  `accept_f57_a_recycled_generations_never_share_mirror_state` — a new generation
  under the same id retires the old record outright (`replaced`, not `updated`),
  no field of the old generation survives, and a stale record for the old
  generation is refused. F57-D measures the interpolation-history consequence;
  this is the schema-level guarantee F57-B builds the buffer on.

## Sensitivity: what fails when the implementation is removed

Each mutation was applied, run, and reverted (the last mutation listed per row
was observed failing the named test):

| Mutation | Failing test |
| --- | --- |
| `apply_event` returns true on a replay | `accept_f57_a_the_mirror_awards_nothing` |
| drop the `tick < applied_tick` guard | `accept_f57_a_latency_loss_and_reordering_...` |
| drop the `Some(first) = destruction.get(..)` dedup | `accept_f57_a_destruction_is_awarded_once_per_generation` and two cs_app tests |
| publish absolute world coordinates instead of epoch-relative | `accept_f57_a_a_foreign_origin_epoch_is_refused_...` and the latency scenario |
| do not retire the previous generation on a generation change | `accept_f57_a_recycled_generations_never_share_mirror_state` |
| silently lower the position scale (4 steps/m) | `accept_f57_a_declared_budgets_are_explicit_and_half_a_step` |

## Known limitations, carried forward

These are real and belong to later stages; none of them is presented as working.

1. **There is no transport.** The link in the cs_app test is a deterministic
   test-side model of delay, loss and reordering. F54-B owns the pinned
   transport and its channels; until then no packet has ever left a process.
2. **No interpolation, no prediction.** The mirror holds the newest authoritative
   record only. F57-B adds the bounded jitter buffer and bounded local
   prediction; the declared budgets above are the error budget its reconciliation
   has to stay inside, and `ROTATION_ORIENTATION_ERROR_RAD` is the rotation part
   of it.
3. **No rollback, and none is claimed.** Nothing here rolls Avian state back.
   The sheet's non-negotiable 4 asks for bounded state correction with documented
   limitations when full rollback is unavailable; F57-B/C own that decision, and
   this stage's contribution is that the authority per actor and generation is
   already addressable without it.
4. **A bailout publishes as `Despawned`.** The wire flag answers "is this actor
   still here?"; the *reason* is a server-side fact that belongs in a reliable
   lifecycle event, and F54-A's `EventBody` has no such kind yet. F57-C should
   add it rather than widen the snapshot flag.
5. **`MAX_ACTORS_PER_SNAPSHOT` is 64 and a larger population is refused, not
   split.** A 64-record snapshot is 4 235 bytes inside F54-A's 8 KiB envelope cap
   (asserted at compile time). Splitting one tick's population across several
   sequenced snapshots is F57-C's decision, not something this stage guesses at.
6. **The snapshot epoch field is `u32` while `cs_app::origin::OriginEpoch` is
   `u64`.** A local epoch beyond `u32::MAX` is refused
   (`EpochUnrepresentable`) rather than truncated onto an existing epoch. If
   rebases can ever exhaust 32 bits, F57-C must widen the wire field; the
   refusal is the placeholder that makes the limit visible instead of silent.
7. **Remote projectile cosmetics are not modeled.** F57 non-negotiable 3
   ("a local tracer cannot award a kill") is structural here — the mirror holds
   no damage or reward state at all, and `accept_f57_a_the_mirror_awards_nothing`
   pins that — but the predicted-tracer path itself is F57-B/C work.

## Unknowns, recorded rather than guessed

- No original multiplayer transport, bandwidth budget or packet layout is known
  or claimed; the legacy DirectPlay/MSN/IPX question stays out of scope per the
  F54 sheet.
- The original's networked lifecycle vocabulary, interpolation delay and remote
  presentation are unmeasured. `Lifecycle`'s three wire kinds are the minimum a
  receiver needs, and the five damage-domain kinds (`cs_sim::damage::LifecycleKind`)
  stay server-side.
- Original per-airframe speeds, rates of fire and turn rates are unmeasured; the
  velocity and angular-velocity ranges above are sized from F24's model space,
  not from a retail table.
