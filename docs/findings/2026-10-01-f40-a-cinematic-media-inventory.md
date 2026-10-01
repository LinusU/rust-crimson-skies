# F40-A: cinematic/media inventory and semantic boundaries

Task #162. Runtime contract `cs_sim::cinematic_state`; declared schema
`cs_content::cinematics`; lowering boundary `cs_app::cinematics`. Tests are the
`accept_f40_a_*` unit tests in those three modules (the `tests/` owner path
holds no file: every test needs crate-private fixtures).

All behavior is **designed**, synthetic and not original-verified. Awards at
most *checked*. No original video, camera track or cutscene data was read: the
fixture is `Origin::SyntheticFixture`, with designed provenance on every
`Resolved`.

## One observable failure

Skip the synthetic scene at the start, midpoint or one tick before the end:
the applied semantic actions must equal those of a full playback. A player that
treated skip as an abort (or dropped the final `ReturnControlToPlayer`) leaves
`applied()` shorter and `accept_f40_a_skip_at_start_midpoint_and_final_frame_matches_full_playback`
fails.

## Designed semantics

- **Semantics apart from media.** `SemanticAction`s (objective events, control
  hand-back) belong to the script; the video/camera track is a
  `PresentationPlan` the player never inspects (non-negotiable behavior 1).
- **Exactly once.** Actions apply in `(tick, id)` order through a cursor, so
  play-out, skip, a repeated skip request and failure recovery can never apply
  one twice. `cancel` is the only transition that leaves actions unapplied.
- **Explicit states** `Start, Playing, SkipRequested, Completed, Failed,
  Canceled`; every other call is a `WrongState` error.
- **Failure is never completion.** Missing file or decoder gives `Failed`
  with the useful `MediaFailure`; the script's `FailureRecovery` decides whether
  the remaining semantics still apply (`semantic_end_reached`).
- **Control returns to the aircraft flown at that moment** (AC03 data half).
- **Letterboxing** `fit_letterboxed` never stretches (behavior 4).
- Pause policy and master clock are explicit per script (behavior 3); drift
  measurement is F40-B.

## Unknowns (not guessed)

Recorded as `Resolved::Unknown` where an importer cannot supply them; lowering
refuses by claim:

- The original cinematic inventory: which scenes are prerendered video, which
  are in-engine, their container/codec, frame size and duration.
- Whether each original scene is skippable, its pause policy, its master clock.
- Which original scene events map to objective events or control changes.
- The video format and an approved decoder (F40-B needs one or a documented
  private transcoding cache).
- The tolerance for audio/video drift (AC02).

Resolving tasks: F40-B (decoder, drift), F40-D (retail inventory and
verification, needs `retail, gpu, audio`).
