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

Resolving tasks: F47-D (audit against retail) and the importer work that
decodes the scrapbook data.
