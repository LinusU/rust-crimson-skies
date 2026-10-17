# F47-C1 — the front end dispatches the paged scrapbook

2026-10-17, task #760. Follow-up of F47-C, the paged screen
(`docs/findings/2026-10-08-f47-c-scrapbook-paged-ui-and-replay.md`): the
screen existed with its own fallible operations; this task is the dispatch
that drives it from the production `FrontEndFlow`.

## What shipped

`FlowSetup` gains a declared `ScrapbookInputs` — the `ScrapbookCatalog` and
the page size — beside the other declared inputs. The flow constructs one
`ScrapbookUi` at setup when the inputs exist; nothing about the catalog or
the page size is guessed when they do not.

- `Cabin → Scrapbook` carries `Request::OpenScrapbook`. The domain reads the
  selected profile's stored records and projects them through the running
  `TextSession` (`ScrapbookUi::refresh_stored`). A stored revision the
  records reader refuses is the screen's own visible reason — the screen
  still opens, unprojected — rather than a press failure.
- `NextPage`/`PrevPage`/`SelectEntry` are new user actions on the scrapbook
  row set, each carrying a domain request. `SelectEntry` is a supplied
  input: `choose_scrapbook(ContentId)` / `select_scrapbook(ContentId)`
  record the choice, the table row asks, the screen selects only what the
  current projection shows (`SelectError::NotVisible` otherwise).
- `Scrapbook → Loading` (`Action::Launch`) carries `Request::LaunchReplay`.
  The domain resolves the selected entry's `ReplayLink` through
  `ScrapbookUi::launch` and stores the `ReplayLaunch`; `prepare` first
  requires the declared plan's `LoadTarget.world` (`LoadError::NoWorld`
  otherwise, before the machine moves). On the transition the flow
  repoints the load's plan at `replay.load_target(world)` — the
  application-declared world group, the entry's own mission scope — saves
  the declared target, and `teardown` restores it on every exit path so a
  replay's scope can never leak into the next campaign launch.
- `FrontEndFlow::set_text_session` swaps the locale and refreshes an
  already-open scrapbook: titles re-resolve under the new chain while the
  saved `ContentId`s — being locale-free — do not move.
- Leaving the screen is the teardown: `close_scrapbook` clears the
  projection, the visible reason, the last refresh report and any stored
  replay, and `close_profile` runs it too.

Every refusal — `NoScrapbookCatalog`, `NoTextSession`, `PageError`,
`SelectError`, `LaunchError`, `PersistError`, `NoWorld` — either stays on
the scrapbook with the reason the screen shows, or (for the two missing
declarations) keeps the machine in the cabin. Nothing is swallowed.

## The consequence for decks

The new actions are user actions, so `ScreenDeck::new`'s `HiddenAction`
rule applies to every deck that carries a scrapbook screen: synthetic
decks must author `next-page`, `prev-page`, `select-entry` and
`launch-replay` hotspots (the preflight deck now does).

The measured retail `ScrapBook_TOC` honestly flips to
`Refused(HiddenAction(NextPage))`: its script binds only `BACK` (the
`MORE` script it carries is not a button target `OriginalScreens` can
bind), and the three self-transitions can never be bound through the
script-target mechanism at all — a self-transition refuses as a script
target by construction. The verdict tests now pin that refusal by name
rather than silently dropping the screen from the covered set. Nothing
about the retail layout is claimed; F47-D owns the audit.

## One ordering worth recording

`consume` must take the stored replay *before* the leaving-the-screen
hook closes the scrapbook — `close_scrapbook` clears `domain.replay` by
design, so taking it after the close would silently drop every launched
replay. The code does `domain.replay.take()` first, then closes, then
retargets before `load.begin()`.

## Verification

`cargo test -p cs_app --test ui` — 70 tests, all passing, including the
seven `accept_f47_c1_` tests in `tests/ui/scrapbook_dispatch.rs`: the
open/page/select/back-out happy path, a locale swap mid-open, the replay
launch's scope-and-restore, the locked/link-less refusal keeping its
selection, the unreadable-stored-revision reason, the three declaration
refusals, and the machine's own nothing-declared refusal. Mutation check:
dropping the retarget call fails
`accept_f47_c1_launch_loads_the_replay_scope_and_restores_the_declared_target`
with the declared (scope-less) target still running.
