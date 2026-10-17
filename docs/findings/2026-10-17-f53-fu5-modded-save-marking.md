# F53-FU5: a modded run's save carries the announced signature

Date: 2026-10-17. Task: #767 / F53-FU5 "Mark a modded run's save" — a
follow-up the `F53-D` review raised against
`docs/findings/2026-10-08-f53-d-source-protection-mod-isolation-and-reproducible-load-order.md`.
Synthetic only — no retail or original-behavior claim.

## The gap

F53-D shipped the whole vocabulary for "a modified session marks its
saves" (F53 non-negotiable 3): `mark_save_document` writes
`fingerprint.content`, `signature_fingerprint` folds the announced
`ContentHash` into the stored `u64`, `save_population` names the
population a mount belongs in, and `save_dependency` reads the mark back
on reopen. What F53-D did **not** ship was a caller: its own
`accept_f53_d_` test produced the marked save by calling
`mark_save_document` by hand inside `commit_with`. In the shipped game
no path ever marked a document, so every save — modded or not — was
`Unrecorded`, and the reopen diagnostic F53-D proved could never observe
a real mark.

## What was wired

`ProfileSession` gained an announced content signature:

- `ProfileSession::with_announced_signature(signature)` declares what the
  run's mount produced — `content_signature(Some(&mounted), base)` for a
  mounted set, `content_signature(None, base)` (the base fingerprint
  itself) for a stock run.
- A private `mark_announced` applies `mark_save_document` at every one
  of the session's write paths — `create`, `persist_confirmed_import`,
  `commit_once`, `write_once` — so a save is marked from its first
  revision, not only after a later commit. The helper replaces the
  `content` entry rather than appending, which is the idempotence the
  F48 schema demands (a repeated fingerprint name is invalid).
- The session's `ProfileKind` stays the caller's choice, paired by the
  caller with `save_population`: a mounted run opens `Modded`, a stock
  run `Production`, and the mark and the population agree.

## The boundary the wiring keeps

The session takes a `ContentHash`, not a `MountedMods`. Deriving *what*
to announce — the mount's signature, the base fingerprint, and whether
the mount marks sessions at all (`ModModification::marks_sessions`, e.g.
a cosmetic-only mount) — is the mount layer's vocabulary; `ProfileSession`
owns persistence. `cs_app`'s production session factory is the front-end
flow's `open_profile`, which already receives the population as a setup
input (`FlowSetup::kind`); announcing is the same shape of input.

`None` is a real state, not a default. A session that announces nothing
writes no `content` claim, which `save_dependency` reports as
`Unrecorded` — honestly unknown, told apart from a save that claims
stock. Treating "nothing announced" as "stock" would have forged a claim
the run never made.

## Measurements

`crates/cs_app/tests/accept_f53_fu5_modded_save.rs` (task prefix
`accept_f53_fu5_`, 3 tests, all synthetic trees under the system temp
dir):

- modded session → stored revision carries exactly one
  `fingerprint.content` equal to `signature_fingerprint` of the mounted
  signature, `kind == Modded`, mark identical across repeated commits,
  reopen under the mount `Matches`, reopen under stock `Differs` naming
  the mod-only blueprint;
- stock session → mark equals `signature_fingerprint(base)`,
  `kind == Production`, reopen `Matches`;
- unannounced session → no `content` entry, reopen reports `Unrecorded`.

Mutation: stubbing `mark_announced` makes both marking tests fail with
`left: []` (no `content` entry written); the unannounced test stays
green.
