# F45-C: construction, flight check, loading and return flows wired

Date: 2026-10-07. Task: #194 / F45-C "Wire construction, flight check, loading
and return flows"
(`specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`, section
`### F45-C`). Shared contracts: `docs/contracts/UI-NETWORK.md` and
`docs/contracts/STATE-TRANSACTIONS.md`. Required capabilities: ordinary
build/test only; no original file enters the product and no evidence report is
claimed.

Implemented by `bunny-2` (agent `bunny-2`). This file is the implementer's
record; it is not a review and awards no `verified_original`/`release_approved`.

## One observable failure (listed before editing)

F45-A built a table that *asked* for domain transactions and F45-B presented it
through authored artwork, but nothing consumed either: `Effect::Request` was
collected in an `Outcome` and dropped on the floor, and `Effect::Acquire`/
`Effect::Release` only *declared* what the application should hold. So the
flight check's **Launch** moved the machine to `Loading` while the campaign,
the profile and the loader were untouched — a screen advanced over a
transaction that never happened.

The one observable failure this stage fixes: with the wiring removed, pressing
**Launch** on the flight check must still commit the loadout and start a real
load. Concretely, in
`accept_f45_c_a_failed_load_is_retried_after_repairing_its_dependency_without_restarting`
(the stage's minimum scenario, AC03) the load has to fail on a dependency the
fixture installation does not carry, land the machine back on the flight check
with the draft intact, the world released and the campaign untouched, and then
— after the dependency is written and the content session is remounted, in the
same process — load it and reach the flight.

## Files and what changed

Owner paths only; no protected path, no wiring edit outside them.

- `crates/cs_app/src/ui/front_end/flow.rs` (new, owner path): the wiring.
  - `FlowSetup` — the profile population, campaign graph/run/difficulty,
    airframe roster and construction rules the application supplies once at
    boot, so a profile can be closed and reopened without restarting.
  - `ResourceLedger` + `ResourceProblem` — the consumer of the resource
    effects. It is fed the `Release`/`Acquire` stream itself (never read back
    off the machine), and refuses an acquire of something held, a release of
    something unheld and an input context bound while another is still bound.
  - `LoadPlan` / `LoadFlow` / `LoadVerdict` / `LoadError` — the loading
    screen's real load: one `crate::loading::LoadingSession` pumped through
    the production `SessionIo` over a `ContentSession`, begun when a
    transition enters `Screen::Loading`, torn down (cancelled, private cache
    returned) when one leaves it, and refused before the machine moves when no
    closure/content session/cache is available.
  - `FlowDomain` (private) + `FlowDomainView` — where each request lands:
    `OpenProfile`/`CloseProfile` → `ProfileSession` (F48-C) + `CampaignRun::open`
    (F43-C); `CommitBlueprint` → the attached `ConstructionScreen::commit_saved`
    (F44-C) followed by a run re-read; `CommitLoadout` →
    `AirframeRoles::resolve_launch` (F25-A); `ApplyOutcome` →
    `CampaignRun::report_outcome` (F43-C); `AbandonMission` → nothing, by
    design.
  - `FrontEndFlow` — the object an application drives: **plan → domain →
    transition → consume** for every action, click, activation, discard answer
    and load-failure report, plus `pump_load`/`drive_load` for the loading
    screen.
- `crates/cs_app/src/ui/front_end/machine.rs` (owner path): `Plan` and
  `FrontEnd::plan`/`plan_focus`/`plan_pending` — the transition run on a
  throwaway copy of the machine, reporting the screens it would move between
  and the request it would ask for, with every refusal and guard intact.
- `crates/cs_app/src/ui/front_end/screens.rs` (owner path):
  `ScreenSession::action_at` (the hit-test `click` runs, exposed so a click can
  be planned before it happens) and
  `ScreenSession::report_load_failure` (the producer's failure through the
  machine's own `LoadFailed` row).
- `crates/cs_app/src/ui/front_end/mod.rs` (owner path): `mod flow;`, the
  re-exports and the module-doc paragraph.
- `crates/cs_app/tests/ui/wiring.rs` (new, owner path): the 9
  `accept_f45_c_*` tests.
- `crates/cs_app/tests/ui/main.rs` (owner path): `mod wiring;` and the F45-C
  target docs.
- `crates/cs_app/tests/ui/screens.rs` (owner path): `preflight_deck`,
  `button_point` and `SURFACE` made `pub(crate)` so the F45-C tests drive the
  same authored deck rather than a second one.
- This file.

## The design decisions a reviewer should check

1. **Plan before the domain, the domain before the screen.** `press` asks the
   machine what it *would* do (`FrontEnd::plan` = the same `apply`, on a
   copy), runs that request against the domain, and only then lets the real
   machine move. A refused campaign save, an airframe the hangar may not
   select or a load that cannot start therefore leaves the screen, the focus
   and the ledger exactly as they were. `FrontEnd::plan` is the transition
   itself rather than a re-derivation, so the plan cannot drift from what
   `apply` really does.
2. **The ledger is fed, not read.** `FrontEnd::held()` is the machine's own
   set; `ResourceLedger` is the application's record, built from the effect
   stream the way a renderer/input/audio owner would build it, seeded with the
   install selection's resources through the same acquire path. The ledger test
   asserts the two agree after every walk, which is what turns "returning to
   the menu cannot leave the old world simulating" into a checked property.
3. **The load is the real one.** `LoadingSession` over `SessionIo` over a
   `ContentSession` mounted on a fixture directory: the missing dependency is a
   genuine `ResolveError::NotFound` → `RecoveryPath::MissingDependency` from
   the production resolver, not a fixture that pretends to fail. The repair is
   a file written and a remount (a new content-session generation), which is
   exactly what `LoadingSession::retry`'s documentation says a mission retry
   is: a new request, not a replay.
4. **Two views, one blueprint id.** The machine's `ConstructionDraft` and the
   F44 `ConstructionScreen` are separate objects; the flow holds the id the
   screen was attached under and refuses a `CommitBlueprint` that names another
   one (`ConstructionMismatch`), so the two can never quietly drift apart.
5. **A refused domain transaction is reported, never swallowed.** Every
   producer's own error type survives to the caller (`FlowError::{Campaign,
   Construction, Launch, Profile, Load, Resource, Screen}`), and the tests
   match on the specific variant rather than on "some error".
6. **`AbandonMission` applies nothing.** The pause screen's abort is the
   campaign's *not* happening: the test pins currency and revision unchanged
   and the world released. This is STATE-TRANSACTIONS' "a failed load does not
   consume campaign money or progress" extended to an abort the player chose.
7. **The outcome record is the mission's, the verdict is the screen's.** The
   front end only knows Success/Failure; the flow takes the campaign
   `MissionOutcome` record as a domain-side input and refuses a mismatch
   instead of resolving it — and keeps the record on a mismatch, because a
   refusal changes nothing.

## Non-negotiable behaviour this stage encodes

1. *Every screen has valid Back/Cancel and focus; a visible button works* —
   driven through `FrontEndFlow::click` on the authored flight-check artwork
   as well as through `press`, so the wired path is the same path a pointer
   takes.
3. *Do not replace the cabin/construction flow with a debug picker* — the
   construction commit runs the real F44 validator, economy and profile save;
   its refusal test asserts the campaign is byte-for-byte unchanged.
4. *A loading failure returns to a coherent state without losing the draft or
   corrupting the profile* — AC03's test asserts the selection, the open
   profile session, and an unchanged campaign currency **and** revision.
5. *Transitions acquire/release input, audio and world explicitly* — the
   ledger consumes them and the tests assert one input context, no held world
   at the menu, and ledger == machine after every step.

## Test selection and sensitivity

`cargo test --workspace --locked -- accept_f45_c_ --include-ignored`
discovers and runs **9** tests in the `ui` target, all passing (every other
target reports `0 passed … filtered out`, so the selection is non-empty and
attributed). The full four checks pass locally: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --all-features --locked -- -D
warnings`, `cargo test --workspace --locked` (exit 0; every target green) and the task
selection above.

- `accept_f45_c_a_failed_load_is_retried_after_repairing_its_dependency_without_restarting`
  (the minimum scenario, AC03): fail → name the dependency → back to the
  flight check with the draft, the world released, no campaign write → repair
  → second attempt (attempts == 2) → `Ready` → flight, in one process.
- `accept_f45_c_cancelling_a_running_load_tears_it_down_and_keeps_the_selection`
  — teardown of a live attempt (transaction gone, cache returned, world
  released, campaign unchanged) and a second attempt afterwards.
- `accept_f45_c_a_construction_commit_reaches_the_economy_and_the_profile_save`
  — currency moves, revision advances, and closing/reopening the profile in the
  same process reads the committed state back (so it reached the disk).
- `accept_f45_c_a_refused_construction_commit_changes_nothing_and_stays_on_the_screen`
  — the overweight blueprint is refused by the validator: no screen movement,
  no currency, no revision.
- `accept_f45_c_a_commit_for_another_blueprint_is_refused_before_the_screen_moves`
  — the two construction views have come apart.
- `accept_f45_c_the_flight_check_commits_one_loadout_and_resolves_the_launch`
  — `NotHangarSelectable` and `UnknownAirframe` refused before any load
  starts; the good selection commits player/wingmate/ammunition as one request
  and resolves `LaunchSource::HangarSelection`.
- `accept_f45_c_the_return_flow_saves_the_outcome_and_reopens_the_profile_without_a_restart`
  — abort changes nothing; a verdict/record mismatch is refused and the record
  kept; the real outcome applies and saves; the cabin's escape closes the
  profile and it reopens in the same process with the same revision; the menu's
  quit sets the exit flag.
- `accept_f45_c_a_refused_action_reaches_no_domain_at_all` — a refusal from the
  table itself never reaches the domain, and planning changes nothing.
- `accept_f45_c_the_ledger_and_the_machine_agree_on_every_held_resource`.

Sensitivity was checked by mutation and then reverted (each run:
`cargo test -p cs_app --test ui -- accept_f45_c_`):

| mutation | tests that fail |
| --- | --- |
| `FlowDomain::apply` returns `Ok` without running any request | 7 of 9 (all but the two that never transact) |
| `pump_load` does not report a `Failed` load | the AC03 minimum scenario |
| `consume` stops feeding the ledger | the ledger test and the AC03 scenario |
| the construction blueprint-id cross-check is disabled | the mismatch test |
| the outcome verdict/record check is disabled | the return-flow test |

## What remains unknown (recorded, not guessed)

- **No retail mission declares its dependency closure anywhere in this
  repository.** `LoadPlan` is supplied by the application; which assets a
  retail mission's load requires (and in what criticality order) is unread
  here, so this stage invents none: a flow with no plan refuses to enter the
  loading screen (`LoadError::NoPlan`) instead of loading a guess. Resolving
  the closure belongs with the mission-content stages (F14/F50/F63).
- **The front-end construction draft's `components` are not translated into
  blueprint slot edits.** `ConstructionDraft { blueprint, saved, components }`
  is a flat `Vec<ContentId>`; which component belongs to which armor zone or
  hardpoint is unmeasured (F44's slot semantics are declared per blueprint,
  not derived from a list). The commit therefore runs against the
  `ConstructionScreen` the application attached under the same blueprint id,
  and the id cross-check above is what keeps the two views honest. A faithful
  projection — or making the machine's draft a projection of the F44 screen —
  is follow-up work, filed as **#747** (`F45-C.1`).
- **The wingmate and ammunition slots have no consumer yet.** The flight check
  commits all three atomically (non-negotiable 2) but only the player's
  airframe is resolved, through `AirframeRoles::resolve_launch`; no roster,
  briefing or mission-start object in this repository consumes a wingmate
  airframe id or an ammo id chosen at the flight check. The F33-B finding
  assigns "the option set a selection must be drawn from" to F43-C/F45-C;
  nothing readable here states that set, so none is invented
  (follow-up task **#748** (`F45-C.2`)).
- **The campaign's profile identity is supplied, not derived.**
  `cs_types::profile::ProfileId` is a persistent slot number and
  `cs_sim::campaign::ProfileId` a stable string; nothing in the repository
  derives one from the other (`CampaignRun::open` is only ever called with a
  hand-built id). `FlowSetup::campaign_profile` makes the application's choice
  explicit and identical across close/reopen (follow-up task **#749** (`F45-C.3`)).
- **The construction commit refreshes the run by re-reading the profile.**
  `ConstructionScreen::commit_saved` needs `&mut CampaignState` while the
  return flow needs `CampaignRun`, and `crates/cs_app/src/campaign.rs` is not
  an owner path of this task, so there is no shared save/state accessor to use;
  after a successful commit the flow reopens the run from the profile. Both
  writes land in the same stored revision, and the test that reopens the
  profile proves it. Giving `CampaignRun` a construction-commit path (or
  exporting its persist) would remove the re-read (follow-up task **#750** (`F45-C.4`)).
- **The profile screen's text input is setup, not a transition.** The machine
  has no text field, so the name a new profile is created with
  (`FlowSetup::new_profile`) and the profile an existing choice names are
  supplied by the application; the wording and binding of the original's
  profile-name entry are unread.
- **The session generation a flight check belongs to** is setup-supplied
  (`FlowSetup::session_generation`); an unforced launch is not compared
  against it, so nothing here claims what the original's generation means.
- **No rendering, no input device, no audio.** `click` still takes a surface
  point and the ledger only says what *should* be bound; drawing the authored
  artwork and mapping real devices are F45-D's (`retail,gpu`), which is also
  the stage that can capture the original front end at all.

Every limitation above is named so a later fidelity claim cannot quietly
inherit it: nothing in this stage is `original-verified`, and its fixture data
is authored synthetic data that proves the wiring, never the original front
end.

## Review addendum (review of the same date)

Identities, per AGENTS.md's review policy: **implemented by** `bunny-2`
(session of 2026-10-07 17:44–19:12), **reviewed by** `bunny-2` in a separate
session with a fresh context that did not write the code it reviewed. The two
roles share an agent identity, so this is a fresh-context review, not an
independent-model one; no review here awards anything beyond `checked`.

What the review changed (this addendum supersedes the "9 tests" count above —
the target now runs **11** `accept_f45_c_*` tests):

1. **Stale-state defect fixed.** `FlowDomain::close_profile` cleared the
   profile, the run, the construction screen, the committed loadout, the
   launch and the pending mission outcome, but *not* `applied`: after closing
   a profile the flow kept reporting the previous campaign's applied outcome
   as if it belonged to the next one. It is now cleared with the rest, and
   `accept_f45_c_the_return_flow_saves_the_outcome_and_reopens_the_profile_
   without_a_restart` asserts it is `None` after the reopen.
2. **Coverage gap closed: the refusal that must not happen at all.**
   `FrontEndFlow::prepare` → `LoadFlow::check_begin` (no plan, no content
   session, no cache) had no test, so removing it left no failing test even
   though the machine would then reach `Loading` over a load that cannot
   start. Added
   `accept_f45_c_a_launch_without_a_usable_load_plan_is_refused_before_the_
   machine_moves`, which pins all three `LoadError` variants, the untouched
   screen/ledger/campaign, and — after the cache is supplied — the same flow
   launching in the same process.
3. **Coverage gap closed: the ledger's own refusal paths.** The walks only
   proved that a *well-formed* stream is taken. Added
   `accept_f45_c_the_ledger_refuses_an_impossible_resource_stream`, which pins
   `ReleasedUnknown`, `AcquiredTwice` (audio scope and world) and both faces
   of `InputBoundTwice`, plus the release-then-acquire switch a real
   transition emits.
4. **Sensitivity re-checked by mutation** (each run
   `cargo test -p cs_app --test ui -- accept_f45_c_`, then reverted):
   `prepare`'s check removed → the new no-plan test fails with the machine on
   `Loading`; `self.applied = None` removed → the return-flow test fails;
   `ResourceLedger::apply` made infallible → the new ledger test fails.

Two partial-failure windows the review found and deliberately did **not**
patch, because both need a domain API this task does not own:

- `FlowDomain::open_profile` writes to the population (`ProfileSession::
  create` writes the save and persists the registry immediately —
  `crates/cs_content/src/save/library.rs`, and `select` persists the active
  pointer) *before* `CampaignRun::open` can refuse, e.g. `CampaignSaveError::
  WrongRun` when the stored snapshot names another campaign profile/run
  (`crates/cs_app/src/campaign.rs`). A refused `ConfirmProfile` can therefore
  leave a created profile — or a moved active pointer — behind while the
  screen does not move. The session itself is dropped, so the population claim
  is released and a retry is possible; only the population's contents differ
  from a strict "a refusal changes nothing" reading.
- `commit_blueprint` writes through `ConstructionScreen::commit_saved` and
  *then* re-reads the run with `CampaignRun::open`. If that re-read fails, the
  commit is already on disk while the screen stays where it was and
  `FlowDomain::run` still holds the pre-commit state. This is the cost of the
  re-read already filed as **#750**; a `CampaignRun` commit path that cannot
  fail after the write removes the window.
