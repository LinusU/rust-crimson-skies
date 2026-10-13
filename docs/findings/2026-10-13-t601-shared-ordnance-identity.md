# T-IDENTITY-ORDNANCE: ordnance session/instance identity on the shared `cs_types::net` types

Date: 2026-10-13. Task: #601 (`T-IDENTITY-ORDNANCE`) "Migrate
cs_sim::weapons::ordnance session stamps onto the shared
cs_types::net::SessionId" — the bounded follow-up of the identity unification
(#397 animation, #442 damage, #496 audio), noticed while reviewing #496 and
deliberately not fixed there. Owner paths used: `crates/cs_sim/`,
`crates/cs_app/` and `docs/findings/`. Required capability: ordinary
build/test only — no `retail`, no evidence report.

**Verdict: migration complete.** `cs_sim::weapons::ordnance` no longer defines
its own session or instance identity: `StatusEffectInstanceId` and
`AreaInstanceId` are the shared `cs_types::net::ActorId`, and every
per-session record and input boundary — `GuidanceTracker`, `GuidanceSet`,
`StatusEffectLedger`, `AreaLedger`, `NitroLedger`, `OrdnanceRuntime` and its
`guidance_tick` / `apply_statuses` / `apply_detonation_statuses` /
`advance_status` / `advance_areas` / `route_trigger` / `route_detonation` /
`route_detonation_area` / `open_area` / `open_detonation_area` entry points —
is bound to the shared nonzero `SessionId`. The confinement rules did not
change: a foreign generation is still refused **by name**, and the only
behavioural difference is that session 0, previously a valid `u64` a ledger or
runtime could be built for, is now unrepresentable — which is the point of
the nonzero `SessionId`. This document awards at most **checked**.

## Before / after

| Shape | Before | After |
| --- | --- | --- |
| Effect identity | `struct StatusEffectInstanceId { session: u64, serial: u64 }` | `pub type StatusEffectInstanceId = ActorId;` |
| Area identity | `struct AreaInstanceId { session: u64, serial: u64 }` | `pub type AreaInstanceId = ActorId;` |
| `GuidanceTracker` / `GuidanceSet` | `session: u64`, `new(session: u64)`, `session() -> u64` | `SessionId` throughout; `GuidanceSet` lost its `Default` derive (a "generation zero" set cannot be built) |
| `StatusEffectLedger` / `AreaLedger` / `NitroLedger` | `new(session: u64, ..)`, `session() -> u64` | `new(session: SessionId, ..)`, `session() -> SessionId` |
| `OrdnanceRuntime` | `new(session: u64, ..)`, `session() -> u64` | `new(session: SessionId, ..)`, `session() -> SessionId` |
| `LostTargetReason::ForeignSession`, `StatusEffectError::ForeignSession`, `NitroError::ForeignSession` | `expected: u64, found: u64` | `expected: SessionId, found: SessionId` |
| `OrdnanceRuntimeError::ForeignSession` | `expected: u64, found: u64` | unchanged — see below |
| `OrdnanceRuntimeError::NoSession` | a runtime built for session 0 could not stamp a hit | **removed**: a runtime can only be opened for a real generation, so the case no longer exists |
| `cs_app::ordnance::OrdnanceSession` | `new(session: u64, ..)` refusing 0 with `SessionRefusal::NoSession`, holding `session: u64` **and** `session_id: SessionId` | unchanged at its own boundary; it now hands its `SessionId` to the runtime instead of its `u64` |

The ordnance-facing names survive as type aliases (the pattern #397 set with
`AnimationEventId` and #496 repeated for audio), so `cs_app::ordnance`, the
F28 acceptance suites and the T548 blast suite keep reading. The two
ordnance-scoped `Display` impls are gone with the structs — an alias cannot
carry its own impl — and an instance id now displays in the shared
`actor s:n` form rather than `status/s#n` / `area/s#n`. No test or report
asserted those two strings (checked before removal).

## Why `AreaInstanceId` was migrated too

The task text names `StatusEffectInstanceId` concretely; `AreaInstanceId` was
migrated with it because its done-when criterion says "no second struct
definition of either shape", and `AreaInstanceId { session, serial }` is
field-for-field the second definition of the `ActorId` shape in the same
module, with the same never-recycled-serial promise. Leaving it a struct would
have closed half the gap and left the module straddling both conventions —
the exact awkwardness #496 noticed while reviewing.

## Why `OrdnanceRuntimeError::ForeignSession` still reports generation numbers

Every other refusal in this module now carries the shared `SessionId`, but
the runtime-level error keeps `expected: u64, found: u64`. The reason is one
input the runtime validates that this migration does not own: a
`ProjectileId` (F27, `cs_sim::weapons::guns`), which still stamps its
generation as a bare `u64`, is compared in `launch`, `route_detonation`,
`route_detonation_area` and `open_detonation_area`. A refusal must be able to
report the number it was given — including 0, which names no session at all
and therefore cannot be wrapped into a `SessionId` without lying. The variant
keeps its name (`foreign_session`) and its semantics, reports both sides as
the numbers the shared type wraps (as `ProjectileId`'s own `Display` does),
and its doc comment records why. Migrating `ProjectileId`/`guns` onto the
shared type is separate follow-up work — the guns family (`guns.rs`,
`cs_app::weapons`) is unmigrated as a whole, and folding it into this task
would have doubled the diff and the test churn. The ledgers, the guidance set
and every session argument are already the shared type, so the "session-0
sentinel becomes unrepresentable" property holds everywhere a `SessionId` can
be supplied.

`OrdnanceRuntimeError::NoSession` disappeared for the same reason and is not
coming back: the three sites that produced it were `SessionId::new(self.session)`
conversions on hit stamping, and `self.session` is now already the shared
nonzero type. No test or caller referenced it (checked: the only `NoSession`
in the ordnance path is `cs_app`'s `SessionRefusal::NoSession`, which stays —
that is the app boundary that still accepts a raw `u64` generation and refuses
zero before wrapping, exactly the boundary #496 left in
`cs_app::audio::insert_audio_session`).

## What did not change

- No rule, ledger bound, guidance behavior, expiry boundary or falloff rule.
  The only differences in observable behavior are the removed session-0
  sentinel, the two lost `Display` impls, and `GuidanceSet` no longer
  implementing `Default` (nothing used it; verified across the workspace).
- Report strings: `LostTargetReason::label()` still renders
  `foreign_session(11->12)` and the three error `Display`s still read
  "… belong to session 11, but the caller is session 12", because they format
  `SessionId::get()` rather than the newtype.
- `cs_app::ordnance::OrdnanceSession` keeps its public `session() -> u64`
  (its HUD consumer compares generations) and its `SessionRefusal::NoSession`
  zero check; only the calls it makes into the `cs_sim` runtime changed to pass
  its existing `session_id` field.
- `cs_sim::weapons::guns` (`ProjectileId`, `ProjectileRuntime`,
  `FireEventId`, `IntentRefusal`, `SweepRefusal::NoSession`) is untouched.

## Tests

New file `crates/cs_sim/tests/accept_t601_ordnance_identity.rs` (5 tests,
synthetic, run in CI), prefix `accept_t601_`:

- `accept_t601_status_effect_instance_id_is_the_shared_actor_id` — a real
  `StatusEffectLedger::apply` returns an id that binds to `ActorId`, equals an
  `ActorId` literal field for field and passes through a `fn(ActorId)`;
  reinstating the ordnance-scoped struct fails to compile.
- `accept_t601_area_instance_id_is_the_shared_actor_id` — a real
  `OrdnanceRuntime::open_detonation_area` (driven through a guidance-loss
  detonation of a guided item with a declared area) returns the shared id,
  which keys the area ledger.
- `accept_t601_session_boundary_is_the_shared_nonzero_session_id` —
  `SessionId::new(0)` is `None`; the runtime, guidance set, status ledger and
  area ledger all report the shared `SessionId` and pass a `fn(SessionId)`
  boundary; a foreign generation is refused by name with shared-`SessionId`
  payloads.
- `accept_t601_guidance_confinement_survives_the_session_id_migration` — an
  observation stamped with another `SessionId` still loses every target as
  `LostTargetReason::ForeignSession { expected, found }` carrying the two
  shared ids, and the runtime refuses the same generation through both the
  status-ledger path and the named runtime-level refusal.
- `accept_t601_nitro_confinement_is_the_shared_session_id` — the nitro ledger
  refuses a foreign generation with the shared ids.

The existing F28 suites (`accept_f28_a_ordnance`,
`accept_f28_ae1_area_ledger`, `accept_f28_b_ordnance_runtime`,
`accept_t548_lost_target_blast` in `cs_sim`, and the six `accept_f28_*`
suites in `cs_app`) were updated to the new signatures — the `cs_sim` files
gained the `fn session(...) -> SessionId` helper the T397/T496 suites use —
and are unchanged in intent; no test was weakened, skipped or deleted. The
`OrdnanceRuntimeError::ForeignSession` expectations keep their `u64` payload
assertions, which is precisely what that variant still reports.

### The discrimination was checked, not assumed

Reinstatement of the two ordnance-scoped structs was actually performed and
reverted. Both were restored with the **same** field types the aliases now
have (`session: SessionId, serial: u64`) plus the two `Display` impls the
aliases had to give up, so the library body still compiles. Result:

- `cargo check -p cs_app --all-targets` — exit 0, zero errors.
- `cargo check -p cs_sim --all-targets` — six errors, **all** in
  `accept_t601_ordnance_identity.rs`, all at the shared-type boundary:

```
accept_t601_ordnance_identity.rs:143:41: expected `ActorId`, found `StatusEffectInstanceId`
accept_t601_ordnance_identity.rs:153:22: expected `ActorId`, found `StatusEffectInstanceId`
accept_t601_ordnance_identity.rs:165:43: expected `StatusEffectInstanceId`, found `ActorId`
accept_t601_ordnance_identity.rs:169:29: expected `ActorId`, found `StatusEffectInstanceId`
accept_t601_ordnance_identity.rs:199:41: expected `ActorId`, found `AreaInstanceId`
accept_t601_ordnance_identity.rs:202:22: expected `ActorId`, found `AreaInstanceId`
```

That is exactly the intended discrimination: the `through_shared` bindings and
the `shared_actor(...)` boundary calls are what fail, not incidental trait
plumbing. The tree was restored (`git checkout --`) and re-checked clean.

## Commands

Run on the working tree; exit codes as recorded.

```sh
cargo check --workspace --all-targets                                            # 0
cargo test -p cs_sim --locked --test accept_t601_ordnance_identity               # 0: 5 passed
cargo test -p cs_sim --locked -- accept_f28_ accept_t548_                        # 0: 33 + 3 + 14 + 4 passed
cargo test -p cs_app --locked -- accept_f28_                                     # 0: 43 passed
cargo fmt --all -- --check                                                       # 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings    # 0
cargo test --workspace --locked                                                  # 0
```

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**. No
`retail` capability was required or used, so no CLI-EVIDENCE report applies.

## Review

Implemented by `bunny-alpha-1` (opencode, MiMo) on
`rally/601-migrate-cs-sim-weapons-ordnance-session`, session of 2026-10-13.
Independent review is requested from a different agent instance/model with a
fresh context — the review policy asks for it on identity/format migrations —
and the review outcome and identity are recorded on the task at merge time. No
agent review replaces the owner's human approval, and this task does not award
`verified_original` or `release_approved`.
