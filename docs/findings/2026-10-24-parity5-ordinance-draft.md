# The Instant Action draft and the OrdinanceLayout record (#2072)

2026-10-24, follow-up to #2066 (see `2026-10-24-parity5-ordinance-integration.md`).
Implemented by `sonnet-1`. Static readings of the owner-supplied engine image
and the installation's scripts: nothing here is `verified_original`, the
original executable was not run, no disassembly is committed. No independent
review yet.

## What shipped

* `crates/cs_app/src/ui/front_end/ordinance_draft.rs`:
  `PlaneRecord::from_actor(&ScenarioActorSpec)` / `from_template`,
  `PlaneRecord::slot_ordnance()` and `commit_record(actor, &record)`.
* `data/owner-static/engine-facts.toml` `[ordinance_feed]`: the registration
  and store sites of the rocket gate cells (below), checked against
  `CS_ENGINE_IMAGE` by the ignored test.

## The airframe-to-record mapping (decision)

`weapons::ORIGINAL_AIRFRAME_LOADOUTS` is the roster template table at
`0x619f58` that Instant Action copies into the actor's record (see its doc
comment), and each template already holds every field the record needs:

| record field | draft source |
| --- | --- |
| `+0x2c` plane id | index of the airframe's `scene_root` in the table |
| `+0x88` guns | template |
| `+0x34` / `+0x38` box counts | template `rocket_banks()` |
| `+0x98` ammunition | the actor's `SlotOrdnance::gun_ammo`, else template |
| `+0xa8` rockets | the actor's `SlotOrdnance::rockets`, else template |

So the box counts are **not** a draft property: they are the airframe's, and
the draft never writes them (the screens only write `+0x98` / `+0xa8`). The
earlier "unknown" about other planes' counts is resolved by the template table;
it was measured in F27 and F49-D.02-FU-SLOTS, not by this task. The Devastator
check: `player_pfighter` holds 2 / 2, as the fresh-campaign records do.
An unmeasured airframe, or one without a template, has no record
(`DraftRecordError`); nothing is guessed.

## The rocket gate cells (partly resolved)

* `0x647b5c` is the engine variable **`fAllowAll`**, `0x647b68`
  **`fMultiPlayerConstruction`**, `0x647b6c` **`fIAConstruction`**. They are
  registered at start-up (`0x402030..0x4020e0`) by pushing `(name, cell, 1, 0,
  0)` for `0x406ff0`. No instruction stores to them; they are only read and
  compared (`0x40ff00` family, `0x40bcf1`, `0x4100d0..`). Whether `0x406ff0`
  writes through the pushed address (a saved-variable lookup keyed by name) is
  **unread**. No script in `crimson.rof` mentions any of the three names.
* `0x64b678` (progress) has three stores: the larger of itself and `0x64b340`
  (`0x405d84`), the count minus one after a clamp to 24 (`0x40a029`), and the
  new-campaign reset to 0 (`0x411473`, together with `0x64b340`). It reads as
  "highest mission reached" and `0x64b340` as a mission count: a reading.
* Consequence: a fresh campaign has progress 0 and, as far as the code shows,
  all three flags 0, which is what `ProgressGate::default()` is. **It stays**,
  because the Instant Action page's real state (whether the player's campaign
  progress or `fIAConstruction` is what it sees) is not recovered: that needs
  the write path of the flag registry, or an original run. Affected: the
  rocket drop-downs of an Instant Action loadout screen. The draft does not
  pick a gate; the caller supplies one, as for Flight Check.

## Still open

* No presenter yet calls `from_actor` / `commit_record` from a live Instant
  Action page (no such page exists in the shell today).
* No consumer of the committed `SlotOrdnance` in flight (F27, F28);
  `LOADOUT_RESOLVING_TASK` still names the lowering.
* The flag registry's write path (`0x406ff0`, `0x5d39c0`).
