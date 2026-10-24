# M01-TANK-ALLEGIANCE (#2099): is a player's gun round admitted against `hydrogentank1..4`?

Date: 2026-10-25. Capability used: `retail` (read-only `$CS_GAME_DIR`) and the committed
static facts (`data/owner-static/engine-facts.toml`) plus earlier findings. No original
executable ran; nothing here is `verified_original`. No code changed.

## Result

**Unknown. Not implemented.** Nothing measured says whether the original gates a gun
round's damage on a team relation at all, so nothing measured says the player is opposed to
(or merely allowed to damage) the tanks.

## Evidence read

* The tanks' node allegiance is `neutral` (team 0): the resolver's terminal zero
  (`2026-10-18-m01-target-roster.md`). The roster declares no player relation for a neutral
  node, and the designed `HostileOnly` gun rule (`mission_session/weapons.rs`,
  `designed_interaction_rules`, "design, not measurement", F27 non-negotiable 4) refuses an
  undeclared pair. So in retail M01 today no round damages them.
* `HostileOnly` is itself **designed**, not measured. The only measured statement about the
  original's friendly fire is `2026-10-14-f476-measured-per-mode-rules.md`: multiplayer
  friendly fire is *on* by scoring evidence (a same-side kill scores `score_suicide` to the
  killer, `0x46ecd0`), "the damage routine itself was not traced". That points away from a
  hostility gate on guns, but it is a multiplayer scoring observation, not a trace of the
  damage path, and it says nothing about a team-0 target.
* `engine-facts.toml` `target.accept_*` (`0x4a5b20`..`0x4a5c10`) require "teams differ and
  both non-zero". These are the **turret target-acceptance** predicates (what a turret may
  choose to aim at). They are not the projectile-damage admission, and must not be read as
  one. They do suggest team-0 objects are not *chosen* by turrets; that is a different
  question from whether a round damages them.
* `2026-10-24-m01-hydrogen-fire-trigger.md` / `-animation-health-consumer.md`: the tanks are
  objective targets (OBJECTIVE38 marks `hydrogentank1`) and carry `HEALTH 15`; the engine
  code that lowers that health is not located either.

## What is not measured (affected content: winning "The Lost Treasure", #2083)

| Unknown | Resolver |
| --- | --- |
| Whether the projectile hit/damage path (gun round vs world-object node) consults a team relation, and for team 0 | static trace of the gun-round hit handler to the animation object's `HEALTH` decrement (same trace as #2088's open item: the animation object's runtime class built by the parser `0x51e7ec..0x51fb03`) |
| Whether the original treats objective-flagged or enemy-actor-child targets as hostile | same trace, or an original-run capture of a burst on a tank (owner) |

Choosing `NonFriendly`/`Everyone` for M01 guns, or declaring player↔neutral hostile, would
be a guessed rule (AGENTS rule 4) and is not done here.
