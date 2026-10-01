# F39-A: objective, trigger and spawn semantics (synthetic)

Status: designed behavior, **not** original-verified. Code: `cs_sim::objectives`.

## Designed rules (synthetic only)

- Objective states and the legal transitions between them are a designed
  conservative graph. Which transitions the original program allows, and its
  reveal rules, are **unknown**; they are measured in F39-D.
- Triggers sweep the real per-tick movement segment. A segment that passes
  through a volume with both endpoints outside emits one entry then one exit on
  that tick. A teleport observes only its destination.
- Counters distinguish destroyed, captured and despawned (mapped from
  `LifecycleKind`) from disabled and escaped, which have no lifecycle source
  yet. A pilot bailout and mission removal count toward none.
- Spawns and cues are admitted once per `(session, idempotency key)`; a new
  session generation starts an empty ledger and refuses old-session callers.

## Unknown / deferred

- Terminal precedence, timers and their time domains: F39-B.
- `cs_content::objectives` and `cs_app::objectives` (provenance-carrying
  record, conversion): left for F39-B/C; the F39-A types need no content form.
- Disabled/escaped event sources, trigger shapes beyond sphere/box and the
  original reveal rules: unmeasured; no retail evidence was used.
