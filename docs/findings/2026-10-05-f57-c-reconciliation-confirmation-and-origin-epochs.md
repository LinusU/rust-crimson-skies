# F57-C: wired reconciliation, projectile confirmation and origin epochs

Date: 2026-10-05. Task: F57-C. Capabilities: ordinary build/test only; no original
data. Every number, bound and rule below is newly authored engine design; no
original networked projectile, confirmation, interpolation or origin behavior has
been measured, and none is asserted. A pass awards **checked** at most.

## What was added

### Server authority (`crates/cs_sim/src/net_state.rs`)

- `ShotId`: a nonzero shot number. The **client proposes** it with its fire
  intent and the server decides whether the shot exists, so it is a request
  identity rather than an authority.
- `NetStateLedger::accept_shot`: the weapon-acceptance gate. A shot is accepted
  only if it is strictly newer than every shot already accepted for that shooter,
  so a replayed or reordered fire request is refused
  (`NetStateError::StaleShot`) instead of firing twice. A refused shot has no
  id to name anywhere else, which is what makes an unconfirmed prediction
  unconfirmable.
- `NetStateLedger::confirm_shot`: confirms a shot and reports
  `ShotOutcome::Confirmed` the first time and `ShotOutcome::AlreadyConfirmed` for
  a replay of the shooter's most recent confirmation. A hit that could end the
  target is routed through the **existing** once-per-generation
  `record_destruction` gate, and the confirmation reports which verdict it got, so
  two shots hitting one actor award one kill. A target that had already ended
  *without* a kill (bailout, mission removal) awards nothing and is not an error.
- Retention: the shot book is three `u32`-s and an `Option` per actor
  (`highest`, `last`, `accepted`, `confirmed`). It does not grow with rounds
  fired and is dropped by `NetStateLedger::forget`.

Ammunition is deliberately untouched: `NetWeapons` is still written only through
`NetStateLedger::publish`, so accepting a shot spends no round here. The weapon
domain (F27) owns what a shot costs.

### Wiring (`crates/cs_app/src/network/physics.rs`)

- `RemoteMirror::adopt_origin`: adopts a strictly newer shared origin epoch,
  re-projects every mirrored record through the new frame, and **measures** the
  drift. Returns `EpochTransition { from, to, origin, origin_shift_m, converted,
  max_conversion_m }` — the frame displacement and the conversion error reported
  separately, so a consumer can tell a conversion from a teleport.
- `NetSession`: the per-session owner the app holds. It connects the producer
  (`NetStateLedger` → `publish_snapshot` → bytes → `Snapshot::decode`) to the
  consumers in the order the data flows: `ingest` → `sample` → `next_correction`,
  plus `rebase`, `reconcile_local`, `record_local_pose`, `spawn_tracer`,
  `apply_confirmation`, `apply_event` and `teardown`. `reconcile_local` reads the
  record out of its own mirror rather than taking it as an argument, which makes
  "reconciliation follows ingestion" a property of the type.
- `Tracers`: bounded predicted cosmetics keyed by the accepted `ShotId`. A tracer
  is `Pending` until the server's `ShotConfirmation` resolves it to `Confirmed` or
  `Missed`. There is no code path from local geometry to a resolved verdict.
  Bounded by a configured cap (evicting the oldest *unanswered* tracer first, so
  a pilot sees the hit they earned), by `Tracers::TRACER_HARD_CAP = 256`, and by
  `TRACER_LIFETIME_TICKS = 120`.
- `LocalPredictor::actor` / `generation` / `reset` and
  `RemoteInterpolator::clear`: the accessors the wiring needs and the teardown
  path that releases generation memory.
- `SessionError` names every refusal the session layer adds: `Origin`,
  `TornDown`, `NoLocalActor`, `UnknownLocalActor`, `Buffer`, `Tracer`.

## Decisions worth reviewing

- **The rebase drops the interpolation buffers.** Buffered samples hold integers
  decoded against the frame they arrived in; they cannot be reinterpreted in a
  new frame. Dropping them costs one interpolation delay of smoothness and buys
  the guarantee that no aircraft is drawn `origin_shift_m` from where it was. The
  mirror's records survive because they are canonical f64 world positions, which
  is what F16 says a rebase preserves.
- **Shot numbers are the shooter's own, not a server counter.** The server only
  requires strict increase per shooter, so a client is free to skip numbers its
  own firing model never produced, and pairing a confirmation with a predicted
  tracer is exact rather than a race.
- **A replay of only the *most recent* confirmation is absorbed.** A second
  projectile hitting later is a second shot and gets a second confirmation; the
  thing that must not double is the kill, and that is the destruction gate's job.
  A confirmation replayed out of order relative to a newer one would be reported
  as a fresh confirmation of its own shot — its kill is still awarded once.
- **Unanswered tracers are evicted before answered ones.** Preferred order, not a
  correctness requirement.

## Limitations (stated, not hidden)

- Not wired into the Bevy schedule. `NetSession` is the object a Bevy system would
  own and drive; no system drives it yet, so nothing pumps it in a running app.
- Not an exact rollback: Avian state is not rewound (inherited from F57-B).
- The server does not publish which shot ids it accepted; the client learns them
  from its own fire intents. A client whose fire intent was refused has no tracer
  for that shot, which is the intended behavior but does mean the local tracer
  book and the server's shot book can differ while a refused request is in flight.
- Confirmation pairing covers the local aircraft's own shots. A remote aircraft's
  hit is presented from its snapshot record, not from a client tracer.
- No clock estimation: the render tick is an input (inherited from F57-B).
- Latency/loss measurement under real conditions is F57-D (`network_real`).

## Follow-up filed

- `F57-C.01`: wire `NetSession` into the Bevy schedule behind a system set, with
  the F54-C session lifecycle as the owner. Blocked on nothing but is a separate
  runtime concern from this stage's typed path.