# F43-V2: campaign best/latest reconciled with the F47-A `RecordRule` direction

Task #626 (`F43-V2`). Found while reviewing #175 (F43-B): two subsystems
keep a best/latest record per mission and had diverged in the *direction and
scope* of "best":

* `cs_sim::campaign::state::NodeProgress` (F43-A) hardcoded higher-is-better:
  `record.best_score = record.best_score.max(outcome.score)`, no per-subject
  rule, no difficulty scope.
* `cs_sim::records::RecordBook` (F47-A,
  `specs/F47-scrapbook-records-mementos-and-mission-replay.md`) keeps
  best/latest under a declared `RecordRule { better: BetterIs, scope:
  DifficultyScope }`.

The F47-A findings entry (`2026-10-01-f47-a-scrapbook-records.md`) already
flagged this and assigned the decision to whichever stage owns persistence.
This entry is that decision, implemented in `cs_sim` (owner paths: task #626).

## The decision

1. **They must agree — in direction.** Both stores record the same score of
   the same mission, so one declared `RecordRule` governs both. The campaign
   transaction gained `CampaignState::apply_outcome_with_rule`, which judges
   `best_score` with the subject's `BetterIs` through the one shared
   comparison `cs_sim::records::BetterIs::is_better` — the same call the
   `RecordBook` makes. A `BetterIs::Lower` subject (a time) now makes the
   campaign gate keep the *smallest* victory score as best, exactly like the
   scrapbook slot does.

2. **`apply_outcome` keeps the F43-A pin.** It is now a thin wrapper over the
   rule-aware method using `RecordRule::DEFAULT`
   (`Higher`/`AllDifficulties` — literally the old bare-`max` behavior, made
   explicit). Every caller that declares no subject rule behaves exactly as
   before; `accept_f43_a_`, `accept_f43_b_` and `accept_f43_c_` pass on their
   own terms, AC02 ("preserve best while recording latest") included. The
   pinning did not change, so nothing needed re-recording here.

3. **Scope stays where it can mean something.** A `CampaignState` is one run
   under one difficulty, so its per-node record is per difficulty *by
   construction* and deliberately ignores `DifficultyScope`. The
   profile-level `RecordBook` is where the scope is honored: `AllDifficulties`
   keeps one shared slot across runs, `PerDifficulty` one slot per difficulty.
   `accept_f43_v2_difficulty_scope_lives_in_the_book` pins both halves.

4. **An outcome writes to both stores, through one entry point.**
   `cs_sim::mission_records::apply_mission_record` is the single production
   path: it runs the campaign transaction (which owns currency, unlocks,
   `current` and the run's per-node record) and the book submission (which
   owns the player-facing best/latest and the achievement facts) on copies,
   and commits both or neither. A refusal touches neither store, and the
   declared subject must be the mission the outcome's node is bound to
   (`SubjectMismatch` otherwise). `accept_f43_v2_*` tests pin each of these;
   they fail if either store is fed or the direction is removed (verified by
   mutation during implementation).

5. **Replay repairs a half-applied outcome.** The book is submitted even when
   the campaign ledger answers `AlreadyApplied`. A campaign-ledger hit is
   exactly the shape a crash between the two stores' persistence commits
   leaves behind (they are committed as separate profile revisions today; see
   limitations), and the book's own `(OutcomeId, subject)` ledger makes a
   replay of a complete application a no-op. Pinned by
   `accept_f43_v2_campaign_ledger_hit_repairs_the_scrapbook`.

## What is designed, not measured

The original's record directions per subject, its tie rule, its difficulty
scoping, and whether the original kept one store or two are **unmeasured**.
Nothing here claims otherwise; the direction and scope stay declared per
subject through `RecordRule` and the default is the documented F43-A
behavior, not an observed original rule. F47-D's retail audit may replace any
of these policies with measurements.

## Known limitations, with affected content and resolving tasks

* **The results screen still feeds only the campaign store.** The shipped
  wiring (`cs_app::ui::front_end::flow::apply_outcome` →
  `CampaignRun::report_outcome`) calls `CampaignState::apply_outcome`, not
  `apply_mission_record`, so campaign mission results do not yet reach the
  scrapbook in the running game (nor do they name a `RecordRule`). Affected
  content: campaign mission records and mission-succeeded facts in the
  player-facing scrapbook. Resolving task: F43-V3 (#1389), filed with this
  change (`cs_app` results path switches to
  `cs_sim::mission_records::apply_mission_record`); this task's owner paths
  (`cs_sim`) exclude `cs_app`.
* **Two profile revisions, not one.** The campaign save
  (`CampaignRun::persist`) and the scrapbook save (F47-B `persist_mission`)
  commit separate profile revisions; one atomic revision for a mission result
  is `cs_app` work. The crash window between them is covered by the
  replay-repair rule above, per `docs/contracts/STATE-TRANSACTIONS.md`'s
  replay-safety story. Resolving task: F43-V3 (#1389).
* **Kind-policy asymmetry.** `RecordBook::submit` takes no `Outcome` kind —
  the book is kind-agnostic by design (a *run* record) — so a `Failed` run's
  score can raise the book's best, while the campaign's `best_score` counts
  victories only. Affected content: non-victory mission scores shown as
  best in the scrapbook. This is neither fixed nor invented here; the
  resolving task is F47-D (retail audit of what the scrapbook considers a
  record) or a follow-up threading the outcome kind into the book.
* **0-score sentinel.** Under `RecordRule::DEFAULT` a first victory with
  score 0 still raises nothing (exact F43-A comparison preserved); a
  lower-is-better subject's first victory sets the record regardless of
  score, because 0 is a sentinel, not a time. A degenerate restored record
  (a `best_score` with no victories behind it, which `restore` does not
  reject) is never lowered without being beaten in either direction: the
  comparison is always against the stored best, and only a genuinely
  record-less `(victories == 0, best_score == 0)` state lets a first
  victory establish one.

## Where the code and tests live

* `crates/cs_sim/src/campaign/state.rs` — `apply_outcome_with_rule`,
  direction-aware `NodeProgress::best_score` / `best_score_raised`.
* `crates/cs_sim/src/records.rs` — `BetterIs::is_better` (the shared
  comparison), `RecordRule::DEFAULT`.
* `crates/cs_sim/src/mission_records.rs` — the reconciling entry point.
* `crates/cs_sim/tests/accept_f43_v2_reconciled_records.rs` — the six
  `accept_f43_v2_` acceptance tests.
