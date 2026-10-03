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
is bound to an emitter explicitly, through `AudioEmitterBinding`, and the audio
router/mixer never consults a damage path. The shared `ActorId` satisfies that
— it is the neutral identity `cs_types` owns, named here directly rather than
imported from `cs_sim::damage` — while ending the duplication.

Note the consequence of #442 having already landed on `main`: because
`cs_sim::damage::ActorId` is itself now a re-export of the same shared type,
`AudioEmitterId` and `damage::ActorId` are *literally the same type*. That is
the intended end state of the identity unification (one id definition, one
comparison and hash across subsystems), not a new coupling: audio names it at
its declaration site and the audio path stays reachable without touching
damage. No damage code was touched here.

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

### The discrimination was checked, not assumed

Reinstatement of a second definition was actually performed and reverted. Two
variants were tried:

1. Restoring the original structs verbatim (`session: u64`) — 299 errors, all
   inside `audio_events.rs`, because the module body compares the ids against
   the shared `SessionId`. This fails the build but says nothing about the
   test file, so it is not the interesting result.
2. Restoring second structs with the **same** field types (`session: SessionId`)
   plus the two audio-scoped `Display` impls the aliases had to give up. The
   library and every other test target then compile, and the *only* errors
   anywhere are five in `accept_t496_audio_identity.rs`, all at the
   shared-type boundary:

   ```
   accept_t496_audio_identity.rs:68:26: expected `AudioEventId`, found `EventId`
   accept_t496_audio_identity.rs:69:29: expected `EventId`, found `AudioEventId`
   accept_t496_audio_identity.rs:77:35: expected `AudioEmitterId`, found `ActorId`
   accept_t496_audio_identity.rs:78:35: expected `ActorId`, found `AudioEmitterId`
   accept_t496_audio_identity.rs:80:29: expected `ActorId`, found `AudioEmitterId`
   ```

   That is exactly the intended discrimination: `shared_event`/`shared_actor`
   and the `let through_shared: ActorId = emitter` binding are what fail, not
   incidental trait plumbing. The file was restored (`git checkout --`) and the
   tree is clean.

## Commands

Run on the rebased tree; exit codes as recorded.

```sh
cargo fmt --all -- --check                                                       # 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings  # 0
cargo test --workspace --locked                                                 # 0: 3287 passed, 0 failed, 360 ignored
cargo test --workspace --locked -- accept_t496_ --include-ignored               # 0: 3 passed
cargo test --workspace --locked -- accept_f41_                                  # 0: 52 passed
```

One caveat a reviewer should know: several runs aborted with
`error: test failed, to rerun pass -p cs_xtask` and the cause
`could not execute process .../cs_xtask-<hash> (never executed) / No such file
or directory (os error 2)`. That is a shared-`target/` artifact race with other
agents on this machine — the test binary was absent from disk while its object
files were present — not a test failure. `cargo test -p cs_xtask --locked`
passes on its own and a re-run of the whole workspace suite completed green.

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**. No
`retail` capability was required or used, so no CLI-EVIDENCE report applies.

## Review

Implemented by `devin-1` (Devin, SWE-2, session of 2026-10-03). Resumed and
re-verified by `bunny-alpha-1` on the same branch after the implement lease
expired: the four checks above were run again on the rebased tree, the
discrimination experiment was performed and reverted, and the two findings
statements that had gone stale were corrected (the "realized for audio" wording
in the F41-A findings, and the claim that the emitter type is distinct from
`cs_sim::damage::ActorId`, which stopped being true when #442 landed).

That re-verification is **not** an independent review: the same work, the same
branch, and a session that read the implementer's own notes. Review is still
requested from a different agent instance/model with a fresh context, and the
review outcome and identity are recorded on the task at merge time. No agent
review replaces the owner's human approval, and this task does not award
`verified_original` or `release_approved`.

### Review of record: `bunny-alpha-1`, 2026-10-03 (Rally review claim)

A later session of `bunny-alpha-1` reviewed the branch with a **fresh context**
(it re-derived every claim from the tree rather than from the earlier notes),
re-ran all four project checks on the pushed head `f1e3dc81`, reproduced the
discrimination experiment independently and reverted it, and made one fix of
its own: entry 2 of `docs/findings/2026-10-02-t397-shared-identity-ids.md`
still described the audio struct definitions as present, which had stopped being
true when this branch landed, and is now written the way that document's entry
for #442 already reads ("has since done it … the deferral is closed").

Two facts the review checked directly, because the earlier notes asserted them:

- `SessionGeneration` cannot be 0, so the `expect()` in
  `cs_app::audio::handoff::insert_audio_session` cannot fire: `cs_assets::vfs`
  hands generations out from a process-wide `AtomicU64` starting at 1 and the
  type has no other constructor.
- With a second `AudioEventId`/`AudioEmitterId` struct definition reinstated
  (same field types, plus the two `Display` impls the aliases gave up),
  `cargo check -p cs_sim --all-targets` fails in **only**
  `accept_t496_audio_identity.rs` (five `E0308`s at the shared-type boundary)
  and `cargo check -p cs_app --all-targets` still exits 0. The acceptance test
  is the discriminator, and no other target depends on the aliasing by accident.

This review is by the **same agent instance** that resumed and re-verified the
work, so it is **not** the independent review the review policy asks for on a
format/semantics migration of this kind; the context was fresh but the identity
was not. A different agent instance or model should still review before this
counted as independently reviewed.
