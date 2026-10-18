# F49-D.02 follow-up: the original has no Instant Action loadout identity list (measured negative)

Date: 2026-10-18. Task: #1575 `F49-D.02-FU-LOADOUT`. Capability used: `retail`
(read access to `$CS_GAME_DIR`). Nothing here observes the original executable
running; it states only what the installation's files declare.

## Question

Does the original store a list of loadout identities that a custom/Instant
Action scenario selects from, so that `retail_scenario_options` can fill
`ScenarioOptionDimension::Loadouts` with `ContentKind::Loadout` ids?

## Measurement (members of `GOSDATA/ASSETS/crimson.rof`)

* `ASSETS/SCRIPTS/INSTANTACTION.SCRIPT` creates an `ia_b_changeweapons` button.
  Its handler sets the editor's actor selector `@globals@ZQ` to `-1` (the player)
  or `-2` (a wingman) and runs `assets\scripts\ordinancelayout.script`.
* `ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT` edits that actor's weapons **per
  slot**: it asks the engine about the actor and slot by integer index
  (`callback($$E$$, 2030, (ZQ), (BA), …)` for a gun slot, `2031` for rocket
  ammunition, `2036`/`2037` when a dropdown changes). The slot counts are the
  F27-D ones (four gun slots, eight rocket slots).
* Neither member contains the text `loadout`, and no member name in the container
  names one. Nothing enumerates a preset or named loadout.

So the original's Instant Action "loadout" is a **per-slot selection** over
guns, gun ammunition and rocket ammunition, with the player and each wingman
edited separately. It is not an identity chosen from a list.

## Consequence

* `retail_scenario_options` keeps `loadouts` empty and **unmeasured**; the reason
  (`LOADOUT_UNMEASURED_REASON`) now states the measured negative instead of "no
  producer found", and the resolving task is the per-slot selection model
  (`LOADOUT_RESOLVING_TASK`). No `ContentKind::Loadout` id is minted: one derived
  from the slot counts would be a fabricated retail-looking id (AGENTS rule 4).
* The `UnsupportedLoadout` refusal in custom-scenario validation therefore still
  fires for any named loadout, now for a stated reason.
* Not measured: the per-slot choice ranges (which of the five guns and four
  ammunition types each slot offers; engine-side callbacks `2030..2037`), and
  whether a plane has default slot contents. Both need the engine image or an
  original run.

## Test

`accept_f49_d_02_retail_loadouts_are_a_measured_negative_not_an_identity_list`
(ignored without `CS_GAME_DIR`) re-reads both members through the production ROF
reader, asserts the markers above, asserts neither names a loadout, and asserts
the retail table's loadout evidence is the unmeasured record carrying that
reason. Reverting the reason text fails it.
