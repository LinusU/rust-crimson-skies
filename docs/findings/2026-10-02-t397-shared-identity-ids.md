# T-IDENTITY-IDS: one canonical session/event/actor identity in `cs_types`

Date: 2026-10-02. Task: #397 (`T-IDENTITY-IDS`) "Add shared
`SessionId`/`EventId`/`ActorId` contract types to `cs_types`". Owner paths
used: `crates/cs_sim/`, `crates/cs_app/`, `crates/cs_types/` (tests only) and
`docs/findings/`. Required capability: ordinary build/test only — no `retail`,
no evidence report.

**Verdict: the shared types were already added by F54-A (#283); this task closes
the migration half.** `cs_types::net` already defines `SessionId`, `ActorId`,
`EventId` and `ActorAllocator` exactly as `docs/contracts/IDENTITY-CONTENT.md`
sketches them. The conforming work left for #397 was to move the animation-scoped
copies onto that definition, so event identity is defined once. That is done.

This document records what already existed, what was migrated, what was
deliberately left out and why, and the checks that exercise the result. It
awards at most **checked**.

## What already existed (F54-A)

`crates/cs_types/src/net.rs` (added by F54-A, task #283) is the canonical
definition, and it is untouched by this task:

| Contract sketch (`IDENTITY-CONTENT.md`) | `cs_types::net` | Notes |
| --- | --- | --- |
| `SessionId(u64)` | `SessionId(u64)` | private field; `new() -> Option`, zero refused; `get()` |
| `ActorId { session, serial }` | `ActorId { session: SessionId, serial: u64 }` | `serial` never recycled; `ActorAllocator` issues from 1 |
| `EventId { session, tick, producer, sequence }` | `EventId { session: SessionId, tick: Tick, producer: u32, sequence: u32 }` | `Tick` not `u64`; total order |

So "add the shared types" was already satisfied. Re-adding them, or moving them,
would have duplicated F54-A. The remaining gap the task describes is that
`cs_sim::animated_object` carried its own `AnimationEventId` stopgap instead of
naming the shared type; that is what this task migrates.

## The migration

### `cs_sim::animated_object` (the task's named stopgap)

Before: a private four-field struct, `AnimationEventId { session: u64, tick:
Tick, producer: u32, sequence: u32 }`, with the doc comment recording that
`cs_types` had no shared type yet. `AnimatedObject { session: u64, .. }` and
`AnimatedObject::new(clip, session: u64, producer: u32)` stored the raw number.

After:

- `pub type AnimationEventId = EventId;` — the animation-facing name now denotes
  the shared contract type itself. It is an alias, not a second struct, so an
  `AnimationEventId` compares, orders and hashes exactly as an `EventId` and can
  be passed to any code that names the shared type. The alias keeps the
  animation vocabulary that F20-B/C already refer to.
- `AnimatedObject.session: SessionId` and `new(clip, session: SessionId,
  producer)` — the nonzero rule now applies at the constructor boundary, so an
  animation can never be evaluated under a zero "no session" sentinel.
- Module docs and the alias doc now point at `docs/contracts/IDENTITY-CONTENT.md`
  and record that F20-A follow-up 1 is resolved by #397.

Observable failure this fixes: before the change, an animation event could only
be compared to another animation-scoped struct. A consumer holding the shared
`EventId` (`cs_net`, a replay/reconnect path) had to convert field by field, and
the two definitions could drift. After it, `accept_t397_animation_event_id_is_the_shared_event_id`
passes the emitted `event.id` straight into a `fn(EventId)` and asserts equality
with an `EventId` literal — which does not compile against a look-alike struct.

### `cs_app::animation::playback`

`AnimationPlayback.session` and `AnimationPlayback::new`/`session()` moved from
`u64` to `SessionId`. The playback already had the invariant "no animation plays
without a session"; the shared nonzero type now expresses it in the type rather
than by convention, and `play_animation` passes the stored `SessionId` straight
into `AnimatedObject::new`.

### Tests adapted to the shared type

The existing F20 suites are production-code consumers of these constructors and
had to pass a `SessionId`; each file gained a small
`fn session(value: u64) -> SessionId { SessionId::new(value).expect("a nonzero
session generation") }` helper (test-only, newly authored) and its call sites
were updated:

- `crates/cs_sim/tests/accept_f20_a_animated_object.rs`
- `crates/cs_app/tests/accept_f20_a_animation_boundary.rs`
- `crates/cs_app/tests/accept_f20_b_animation_playback.rs`
- `crates/cs_app/tests/accept_f20_c_01_attachment_hierarchy.rs`

The assertions themselves are unchanged in intent: `id.session == session(2)`
instead of `id.session == 2`. No test was weakened, skipped or deleted.

## What is deliberately **not** migrated, and why

The task says "any other ad-hoc session/tick/producer/sequence stamps … where
applicable". Three nearby shapes exist; none is the same identity domain, and
each is recorded rather than silently conflated.

1. **Damage identity — completed by #442.** When #397 was written,
   `cs_sim::damage` defined `ActorId { session: u64, serial: u64 }`,
   `HitEventId` and `DamageEventId` with the same field shapes, spanning
   `cs_sim::damage`, `cs_sim::targeting`, `cs_sim::weapons`,
   `cs_content::damage`, `cs_content::weapons` and `cs_app`. Task **#442**
   (`T-DAMAGE-IDENTITY`) owned that unification and has since done it:
   `cs_sim::damage::ActorId` is now the shared `cs_types::net::ActorId`,
   `HitEventId`/`DamageEventId` are the damage-facing names of
   `cs_types::net::EventId`, and the node key is the one
   `cs_types::content::DamageNodeKey`. This entry is kept for the audit
   trail; the deferral is closed.

2. **Audio identity — a new follow-up task.** `cs_sim::audio_events` defines
   `AudioEventId { session: u64, tick: Tick, producer: u32, sequence: u32 }`
   (exactly `EventId`) and `AudioEmitterId { session: u64, serial: u64 }`
   (exactly `ActorId`). Unlike the damage case there was no task tracking it;
   one is filed from this task (see "Follow-ups filed"). The F41-A findings
   note the same gap and are updated to point at it.

3. **Scene generation and load-transaction tickets — not applicable.**
   `cs_app::scene::SceneGeneration(u64)`, `cs_assets::vfs::session::SessionGeneration(u64)`
   and `cs_app::loading::LoadIdentity { session: SessionGeneration, serial: LoadSerial }`
   look similar but are a **different identity domain**:

   - They name a *process-local content/scene load generation*, not the
     simulation session epoch. `SceneGeneration` starts at `Default` = 0 and
     counts up (`next()`); `SessionId` is nonzero and is allocated once per
     session by the host. `cs_assets`' `SessionGeneration` comes from a
     process-wide `AtomicU64` starting at 1.
   - Their job is stale-binding invalidation across a reload (a `SceneNodeBinding`
     with an old generation is ignored), not wire/semantic event dedup.
   - `cs_script::runtime::SessionGeneration(u32)` and `cs_script::ir::ActorId(u32)`
     are mission-script-scoped and `u32`-wide; they are a different vocabulary
     again (F30/F33).

   Wrapping or aliasing them as `SessionId` would assert an equivalence the
   engine does not have (a scene generation is not a session, and several scene
   loads happen inside one session). The correct later move, if any, is a
   dedicated content-generation identity — not this task. Recorded here so the
   "where applicable" decision is auditable.

## Tests

New tests, prefix `accept_t397_` (the repo's `accept_t<task-id>_` convention,
matching e.g. `accept_t337_`, `accept_t374`-style, `accept_t383_`):

- `crates/cs_types/tests/accept_t397_identity_contract.rs` (4 tests, synthetic,
  run in CI): drives the shared production constructors — `SessionId::new(0)`
  is `None`, a nonzero id round-trips and displays; `ActorId` is
  session-qualified (same serial, different sessions are different actors);
  `EventId` has the contract shape and orders by tick; `ActorAllocator` starts
  at serial 1 and never recycles. Removing `cs_types::net` fails to compile;
  changing the zero rule, a field, or the allocator's start fails an assertion.
- `crates/cs_sim/tests/accept_t397_animation_event_identity.rs` (2 tests,
  synthetic, run in CI): the evaluator's emitted `event.id` equals an
  `EventId` literal and is passed to a `fn(EventId)`; the `AnimationEventId`
  alias binds to the shared type. Reinstating the old animation-scoped struct
  makes this file fail to compile; changing the field mapping fails the
  equality.

Both files were run together with the ignored tests included (see Commands);
6 `accept_t397_` tests matched and passed.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_t397_ --include-ignored
cargo test --workspace --locked -- accept_f20_
```

The F20 regression run confirms the migrated constructors still drive the
existing animation acceptance suites (F20-A/B/C) to green.

## Follow-ups filed

- **Audio event/emitter identity onto `cs_types::net` — filed as #496
  (`T-IDENTITY-AUDIO`).** Alias/convert `AudioEventId` to `EventId` and
  `AudioEmitterId` to the shared `ActorId`, with a discriminating acceptance
  test. It is the same mechanical migration this task did for animation, and it
  stays out of #442's damage scope.

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**. No
`retail` capability was required or used, so no CLI-EVIDENCE report applies.

## Review

Implemented by `deepseek-1` (DeepSeek V4.1 Flash, session of 2026-10-02T05:37Z).
Review is requested from a different agent instance/model with a fresh context;
the review outcome and identity are recorded on the task at merge time. No agent
review replaces the owner's human approval, and this task does not award
`verified_original` or `release_approved`.
