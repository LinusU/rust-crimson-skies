# T347: sectors and world-object instances stay subordinate keys

Date: 2026-10-27. Task: #400 (T347) "Decide whether sectors and world-object
instances need their own ContentKind namespaces". Follows F18-A
(`crates/cs_content/src/world.rs`). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary build/test
only; no `CS_GAME_DIR` read.

## Decision

**Option 1. `SectorId` and `WorldObjectId` stay validated subordinate keys
inside one `WorldDefinition`. `ContentKind` gains no `Sector` or `WorldObject`
variant.** The catalog row is the world (`ContentKind::World`, "a world group
or one variant of a world"); sectors and placed instances are records of it.

## Reasoning

1. **The contract does not list them.** `IDENTITY-CONTENT` "Required catalog
   collections" names "world groups and variants" and nothing finer, and
   `ContentKind` is documented as the union of that list with F14's
   deliverables. A new variant would claim a namespace the canonical contract
   does not own. Same reasoning as the camera-mode decision
   (`2026-10-02-f21-a-catalog-kind-camera-mode-owner-namespace.md`).
2. **The cost is not local.** `classify_effect` and `classify_validation` in
   `cs_content::mods::overrides` are total over `ContentKind::ALL`; each new
   kind needs an arm in both, plus `label`/`from_label` and catalog readiness
   plumbing, for no consumer.
3. **No consumer needs per-sector readiness or provenance.** F18-B loads
   sectors from a `&WorldDefinition` and keeps object identity across
   streaming by `WorldObjectId`; none reads a catalog row per sector. Object
   instances can number in the thousands per world, so a row each would bloat
   the catalog and its fingerprints without adding a decision anyone makes.
4. **Readiness and provenance already roll up.** A world's readiness covers
   its sectors and instances, because they are parsed and normalized together
   as one `WorldDefinition`. A sector that cannot be built makes the world
   not ready, which is the granularity at which content is launchable.

## Consequences

* `crates/cs_content/src/world.rs` already documents the subordinate-key
  choice in its module docs; no code change is needed. No later stage may
  assume a catalog row for a sector or an object instance.
* `IDENTITY-CONTENT.md` is not edited (protected; no owner authorization).
* Existing `ContentId::parse` behaviour is unchanged for every namespace.

## Revisit trigger

If a later stage needs readiness or provenance reported *per sector* (for
example partial-world streaming where one sector's retail data is missing
while the others are usable), that is a contract change to
`IDENTITY-CONTENT` plus a `cs_types::content::ContentKind` edit, and must be
scheduled as an owner-authorized task. Until then the world is the unit.
