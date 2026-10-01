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
| orientation | smallest-three | 7 bytes | 1.22e-4 rad (`8e`) — see the review correction below |

Derived, not chosen to make a test pass: position is ±33 554 km from the origin
epoch, so a world-scale distance is representable while the step stays at 7.8 mm;
velocity ranges (±1638 m/s, ±63.997 rad/s) cover any airframe F24's model can
produce with headroom. The rotation budget is derived in the module doc and
measured by two acceptance tests (a dense random sweep and the structural worst
case); the first version of that derivation was wrong and is corrected below. The
acceptance test separately proves the dropped component is always a largest one
and keeps its sign, which is what a wrong index or a lost sign would break.

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

## Review corrections (F57-A review, 2026-10-01)

The review of this branch found two measurement defects that were hiding a real
bug in a declared budget, plus four smaller defects. All are fixed on the branch;
the claims above are the corrected ones.

1. **The declared orientation budget was ~13 % too tight, and the test could not
   see it.** `ROTATION_ORIENTATION_ERROR_RAD` was declared as `6e` from the
   derivation "`sqrt(3)e + 3e` of perturbation turns the axis by at most the same
   angle". That last step is wrong by a factor of two: for two unit quaternions
   `|q - q'| = 2*sin(theta/4)`, so a perturbation `p` separates the two rotations
   by `theta ~= 2*|p|`, not `|p|`. The corrected first-order bound is therefore
   `2*sqrt(3e^2 + (3e)^2) = 6.93e`, and **`8e` (1.22e-4 rad, 0.007°) is now
   declared**. Two things hid this. First, the test's own measurement helper
   returned `2*acos(|dot|)*0.5` — exactly half the true angle, and its doc comment
   described a formula (`2*asin(|q1 - q2|/2)`) that the code did not implement and
   that is itself half the angle. Second, the sweep used four axes and 256 random
   angles, which never reaches the encoding's structural worst case.
   The measured worst cases now agree with the derivation: a dense random sweep
   peaks at `2.8e`, and rotations whose four components are near equal in
   magnitude — where the dropped component is smallest and its reconstruction
   from the unit norm is most amplified — peak at `6.93e`.
   `accept_f57_a_the_rotation_budget_bounds_the_structural_worst_cases` measures
   both families, asserts the structural one is the binding one and that it comes
   within 25 % of the budget, and fails if the constant is lowered to `6e`.
   The helper also normalizes its arguments now: `Quaternion::try_new` accepts a
   rotation within `QUATERNION_LENGTH_TOLERANCE` (1e-6) of unit length while
   `decode` returns a strictly unit rotation, so measuring against a denormalized
   input reported the input's own error as the encoding's — worth `9e-7` on its
   own, and it dominated the apparent error of every random sample.
2. **`NetStateError::ForeignSession` was unreachable.** `expect_own_actor`
   returned `InvalidActorId` for both a malformed id and an id of another session,
   so the variant every method's `# Errors` section promised could never be
   constructed, and the distinction the type drew was not real. It now returns
   `InvalidActorId` only for a zero session/serial and `ForeignSession` for a
   valid id of another session generation; the test asserts every entry point
   (`spawn`, `publish`, `record_destruction`, `end_lifecycle`, `forget`) and a
   second ledger refusing this one's actors.
3. **The mirror named the wrong field.** `RemoteAircraft::from_record` mapped a
   position or velocity stored integer that did not fit its declared width onto
   `MirrorError::Rotation`, so a corrupt position was reported to the application
   as "the rotation did not decode into a unit quaternion". `MirrorError` and
   `IngestRefusal` now carry `UnreadableField { field }` and name the field.
4. **A record could travel with integers that do not fit its field.** `ActorRecord`
   fields are public and `QuantizedVector` is `Copy`, so a vector quantized under
   the 32-bit position budget could be assigned to a 16-bit velocity field, and
   `write_to` would have narrowed it silently. `Snapshot::validate` now checks every
   stored position/velocity integer against its own field's declared width, and
   `accept_f57_a_a_record_whose_stored_integers_exceed_its_field_width_is_refused`
   covers it. (`Snapshot::decode` runs `validate`, so this also refuses a corrupt
   payload whose position integer is `i32::MIN`.)
5. **The mirror replayed an older generation.** A record naming an older generation
   than the one held was treated as a *newer* actor — the previous record was
   retired and the mirror rewound to the stale pose. That needs a lost intermediate
   snapshot, which the 10 % loss rows of AC01 produce. It is now refused as
   `IngestRefusal::StaleGeneration { record, held }`.
6. **A reliable event from another session generation retired a live actor.**
   `apply_event` checked the actor's session but not the event's own `EventId`
   session. It now refuses an event that is not this session's event, and the same
   test covers it.

Two test tolerances were also wrong rather than merely loose, and both were
fixed against the right declared bounds rather than by widening a number:

- The latency scenario and the rebase test compared a **three-axis distance**
  against the **per-axis** quantization budget, and ignored the f32 the local frame
  is held in. A mirrored pose is now checked against
  `POSITION_QUANTIZATION::max_error() + cs_app::origin::local_round_trip_tolerance_m(..)`,
  the composition of the two bounds that actually apply, and `publish_actor`'s doc
  says so explicitly.

### Review sensitivity checks

Applied, observed failing, reverted:

| Mutation | Failing test |
| --- | --- |
| `ROTATION_ORIENTATION_ERROR_RAD` back to `6e` | `accept_f57_a_the_rotation_budget_bounds_the_structural_worst_cases` |
| drop the `check_width` calls in `Snapshot::validate` | `accept_f57_a_a_record_whose_stored_integers_exceed_its_field_width_is_refused` |
| collapse `ForeignSession` back into `InvalidActorId` | `accept_f57_a_generations_are_allocated_once_per_actor_and_never_recycled` |
| drop the `current.generation > record.generation` guard | `accept_f57_a_an_older_generation_is_refused_rather_than_replayed_back` |
| drop the `event.id.session` check | `accept_f57_a_an_older_generation_is_refused_rather_than_replayed_back` |

The reviewer of this branch is the same agent instance that implemented it
(`bunny-alpha-1`), on a fresh session but without an independent context, so this
review is **not** independent evidence. Nothing here claims original-reference
fidelity, and none of these numbers is measured from the original game.

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
8. **The declared position budget is a quantization budget, not a
   world-position-resolution claim.** Publish and mirror both go through
   `WorldOrigin::local_of`/`world_of`, whose local frame is `f32`: a position
   100 km from the origin epoch has an f32 ulp of about 8 mm, the same order as the
   7.8 mm quantization step, and it grows with distance. The bound that actually
   applies to a mirrored pose is
   `POSITION_QUANTIZATION::max_error() + local_round_trip_tolerance_m(world, local)`,
   and F57-B's reconciliation budget must use that composition rather than the
   quantization step alone. This stage neither removes the f32 narrowing (F16 owns
   the local frame) nor claims a world-precision the frame cannot hold.

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
