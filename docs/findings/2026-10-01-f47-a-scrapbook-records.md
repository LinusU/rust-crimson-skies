# F47-A: scrapbook records and unlock predicates

Stage `F47-A` defines typed interfaces and a synthetic fixture only. Nothing
here is an original-game measurement; F47-B persists, F47-C builds the UI and
F47-D audits against the retail data.

## What exists

* `cs_content::scrapbook`: `ScrapbookCatalog` of entries keyed by stable
  `scrapbook_item/...` ids, with `Resolved<Unlock>` rules (a predicate over
  mission/stunt/ace facts, or an explicit unknown that never unlocks),
  `HiddenUntilUnlocked` visibility and `ReplayLink { mission, variant }`.
* `cs_sim::records`: `RecordBook` (best and latest, deduplicated by
  `OutcomeId`), `AchievementLedger`, `MementoSelection`.
* `cs_app::ui::scrapbook`: `record_mission`, `record_stunt`, `project`,
  `resolve_saved`, `replay_request`, `choose_memento`.

## Designed policies (unverified)

* Tie: a score equal to best keeps the earlier run as best and replaces latest.
* Difficulty scope and the better-direction are declared per subject through
  `RecordRule`; the original's choice is unmeasured.
* Only a `Succeeded` outcome notes the mission fact; every outcome records
  latest.

## Unknown, not guessed

* How the original encodes scrapbook pages, unlock rules, mementos and replay
  links, and which file stores them. No entry here is original.
* The original's kill/trophy classification: `EntryKind::KillTrophy` exists
  but carries no class.
* Whether the original has a campaign-finished unlock; `ContentKind` has no
  campaign namespace, so no such fact exists yet.
* How the original names a mission variant; `ReplayLink::variant` is a
  mission-kind content id as a placeholder.
* Whether records are per difficulty, what "better" means per record and the
  tie rule in the original.
* Whether replay of a mission re-awards stunt photos; the stunt fact is
  idempotent, so it cannot.

## Overlaps with what other stages already own

These are not unknowns; they are places where an existing owner already models
part of the scrapbook, so a later stage must not silently build a second one.

* `cs_sim::campaign::state::NodeProgress` already keeps `best_score` and
  `latest` per campaign node, under the same rule that a replay records
  `latest` without erasing `best`. `RecordBook` is a second, profile-level
  store keyed by content id and nothing keeps the two in step: for the same
  mission a campaign node and a record slot can disagree. F47-B, which writes
  the save file, has to decide which one the profile owns (or make one
  authoritative over the other) and record that decision before persisting
  both.
* `cs_content::stunts::StuntReward::media` and `cs_sim::stunts::StuntReward::media`
  already name the `scrapbook_item` a completed stunt unlocks (F42). The
  catalog here keys a stunt photo on the *stunt* instead
  (`UnlockFactKind::StuntCompleted` with a `stunt/...` subject), so the real
  stunt path must call `record_stunt` with the stunt id; nothing in the tree
  joins `StuntReward::media` to a `ScrapbookEntry` yet. F47-C.
* `cs_app::ui::front_end` already owns `Screen::Scrapbook`,
  `Action::OpenScrapbook` and the `Cabin <-> Scrapbook` transition rows. This
  projection is not wired to them. F47-C.

## Not implemented here, recorded so it is not lost

* Sheet behavior 3 (keep imported artwork private; an optional export of a
  user-owned screenshot or media needs an explicit local action and no
  automatic upload) has no code path in this stage. `ScrapbookEntry::image`
  only ever names an `image/...` id, `PageView::image` is present only for an
  unlocked entry, and nothing in `cs_app::ui::scrapbook` opens, writes,
  exports or uploads a file. Whoever builds the paged UI (F47-C) must add the
  export as an explicit user action; there is no automatic upload here to
  remove.
* `ReplayRequest` names a mission and an optional variant but resolves
  nothing: there is no content database in this stage, so a link to a mission
  that is absent, not authored or not ready is not detected until the normal
  loading path refuses it. F47-C owns resolving and launching it.
* `EntryKind::KillTrophy` is declared so the importer has a namespace, but it
  carries no classification and no predicate can name a kill. A kill/trophy
  unlock rule therefore cannot be authored yet. F47-D and the importer work.

Resolving tasks: F47-D (audit against retail) and the importer work that
decodes the scrapbook data.
