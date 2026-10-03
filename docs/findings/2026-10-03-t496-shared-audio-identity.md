# T-IDENTITY-AUDIO: audio event/emitter identity on the shared `cs_types::net` types

Date: 2026-10-03. Task: #496 (`T-IDENTITY-AUDIO`) "Migrate
AudioEventId/AudioEmitterId onto the shared cs_types::net EventId/ActorId" —
the bounded follow-up #397 (`T-IDENTITY-IDS`) filed for `cs_sim::audio_events`.
Owner paths used: `crates/cs_sim/`, `crates/cs_app/` and `docs/findings/`.
Required capability: ordinary build/test only — no `retail`, no evidence
report.

**Verdict: migration complete.** `cs_sim::audio_events` no longer defines its
own event/emitter identity: `AudioEventId` is the shared
`cs_types::net::EventId` and `AudioEmitterId` the shared `cs_types::net::ActorId`
(the neutral shared one, not `cs_sim::damage` vocabulary, as F41-B requires).
Every per-session runtime — router, mixer, radio queue, music director — is
bound to the shared nonzero `SessionId` at its constructor, so the "no
session" sentinel is unrepresentable while a foreign generation is still
refused by name. This document awards at most **checked**.

## Before / after

| Shape | Before | After |
| --- | --- | --- |
| Event identity | `struct AudioEventId { session: u64, tick: Tick, producer: u32, sequence: u32 }` | `pub type AudioEventId = EventId;` |
| Emitter identity | `struct AudioEmitterId { session: u64, serial: u64 }` | `pub type AudioEmitterId = ActorId;` |
| `AudioRouter` / `AudioMixer` / `RadioQueue` / `MusicDirector` | `new(session: u64)` | `new(session: SessionId)` |
| `*Outcome::RefusedForeignSession.session`, `MixerRefusal::ForeignSession.session` | `u64` | `SessionId` |
| `cs_app::audio::AudioMixing::new` | `session: u64` | `session: SessionId` |
| `cs_app::audio::AudioInstall.session` | `u64` | `SessionId` |
| `cs_app::audio::insert_audio_session` | `AudioRouter::new(newest.session.get())` | `SessionId::new(newest.session.get()).expect(...)` — the load's content `SessionGeneration` starts at 1, so the wrap cannot fail; the shared type is what makes "no session" unrepresentable |

The audio-facing names survive as type aliases (the pattern #397 set with
`AnimationEventId`), so `cs_app::audio`, the F41 acceptance suites and every
caller of `one_shot_event`/`bind_loop`/`active_loop` keep reading. The two
audio-scoped `Display` impls are gone with the structs — an alias cannot carry
its own impl — and ids now display as the shared `event …`/`actor …` forms.

## Why the emitter is `cs_types::net::ActorId`

F41-B deliberately keeps audio independent of the damage vocabulary: an actor
is bound to an emitter explicitly. The shared `ActorId` satisfies that — it is
neutral identity from `cs_types`, not a re-export of `cs_sim::damage` — while
ending the duplication. `cs_sim::damage`'s own unification was #442 and was
not touched here.

## What did not change

- Refusal semantics: a foreign-session event or emitter is still refused by
  name (`RefusedForeignSession`, `MixerRefusal::ForeignSession`) before it can
  occupy the dedup ledger or loop registry. The only behavioral difference is
  that session 0, previously a valid `u64` a router could be built for, is now
  unrepresentable — which is the point of the nonzero `SessionId`.
- Scene/content generation vocabulary: `SceneGeneration`,
  `cs_assets::vfs::SessionGeneration` and `LoadIdentity` stay their own domain
  (the T397 findings record why); the audio session merely *binds* to the
  load's content generation, wrapped as `SessionId`, exactly as before.
- `SessionGeneration`/`SceneGeneration`-stamped bindings in `cs_app` are
  unchanged.

## Tests

New file `crates/cs_sim/tests/accept_t496_audio_identity.rs` (3 tests,
synthetic, run in CI), prefix `accept_t496_`:

- `accept_t496_audio_event_id_is_the_shared_event_id` — a one-shot accepted by
  the router equals an `EventId` literal field for field and passes through a
  `fn(EventId)`; reinstating the audio-scoped struct fails to compile.
- `accept_t496_audio_emitter_id_is_the_shared_actor_id` — an `AudioEmitterId`
  binds to `ActorId`, passes through a `fn(ActorId)`, and keys the loop
  registry.
- `accept_t496_session_boundary_is_the_shared_nonzero_session_id` —
  `SessionId::new(0)` is `None`, `router.session()` reports the `SessionId`,
  and a foreign generation is refused as `RefusedForeignSession` carrying the
  shared `SessionId` (not a bare `u64`).

The existing F41 suites were updated to the new signatures — each test file
gained the same `fn session(...) -> SessionId` helper the T397 suites use —
and are unchanged in intent; no test was weakened, skipped or deleted.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_t496_ --include-ignored
cargo test --workspace --locked -- accept_f41_
```

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**. No
`retail` capability was required or used, so no CLI-EVIDENCE report applies.

## Review

Implemented by `devin-1` (Devin, SWE-2, session of 2026-10-03). Review is
requested from a different agent instance/model with a fresh context; the
review outcome and identity are recorded on the task at merge time. No agent
review replaces the owner's human approval, and this task does not award
`verified_original` or `release_approved`.
