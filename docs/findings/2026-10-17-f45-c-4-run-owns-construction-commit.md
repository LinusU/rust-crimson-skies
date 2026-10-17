# F45-C.4 — `CampaignRun` owns the construction commit; the re-read is gone

2026-10-17, task #750. Follow-up of F45-C (#194), closing the workaround
recorded in `docs/findings/2026-10-07-f45-c-flow-wiring.md` and pre-announced
by `save_committed`'s own comment ("when a task owns [campaign.rs], a
`CampaignRun` method can wrap this same transaction").

## Before

Two persist transactions existed:

- `CampaignRun::persist` — clone the live state, run the transition on the
  copy, write it under a base-revision check inside `commit_with`, adopt the
  copy only after the save returned.
- `construction::screen::save_committed` — a verbatim restatement for a
  caller holding a bare `CampaignState`, because F44-C did not own
  `campaign.rs`.

The flow therefore committed construction onto a *detached clone* of the
run's state and then re-opened the run from the profile so the live state
caught up with what the screen had just written. Correct — both writes
landed in one stored revision — but the re-read was a workaround: two
owners of "the live campaign" that could not be allowed to diverge.

## Decision

The task offered two shapes — a construction-commit method beside
`report_outcome`/`purchase`/`sell`, or a shared `state_mut` + exported
persist. Neither is what shipped, for one reason each:

- A named `commit_blueprint`/`commit_construction` method would make
  `campaign.rs` import `ConstructionScreen`/`ConstructionContext` — inverting
  the existing dependency direction (construction already depends on
  campaign for `CampaignSaveError`).
- A raw `state_mut` would let a caller mutate the live state *without* the
  base-revision check — the exact hole the run exists to close.

Instead the shared persist itself is exported, generic over the caller's
transition error:

```rust
pub fn transact_with<T, E>(
    &mut self,
    session: &mut ProfileSession,
    transition: impl FnOnce(&mut CampaignState, &CampaignGraph) -> Result<T, E>,
) -> Result<T, E>
where
    E: From<CampaignSaveError>;
```

It is the same body `transact` always ran — `transact` is now a thin
adapter that maps `CampaignError` into `CampaignSaveError` and delegates,
so `report_outcome`, `advance_interludes`, `purchase` and `sell` are
unchanged. One owner, one base-revision check, one adoption point.

`ConstructionScreen::commit_saved(ctx, run, session)` keeps the screen's
staging and its `ScreenSaveError` channel: the session's commit runs inside
`transact_with` (its `ConstructionError` maps to `ScreenSaveError::Refused`;
a persist failure arrives as `CampaignSaveError` → `ScreenSaveError::Save`).
On success the screen re-bases on `run.state()` — already the committed
revision. `save_committed` and its duplicated persist are deleted.

`FlowDomain::commit_blueprint` passes the run itself; the `CampaignRun::open`
re-read after a construction commit is gone. `ctx` borrows `self.setup.graph`
— the graph the run was opened from — so `run` is free for the mutable
borrow.

## Consequences and residual notes

- The stale-screen protection is unchanged: `transact_with` persists under
  the same "stored revision must equal the base the transition computed
  from" check, so a construction commit racing a concurrent writer still
  refuses with `CampaignSaveError::Stale`.
- A no-op construction commit now writes nothing (the revision check in
  `transact_with` skips the save when the revision did not move); the old
  path rewrote the snapshot unconditionally. Harmless: a clean screen has
  nothing to commit.
- `accept_f45_c_4_a_construction_commit_and_an_outcome_commit_share_the_runs_revision_line`
  walks both commits in one session and re-reads the profile: the stored
  revision is the outcome's own, proving both writes landed in one line.
  Mutation check: removing `persist`'s adoption makes the live run show no
  spend and the test fails.
