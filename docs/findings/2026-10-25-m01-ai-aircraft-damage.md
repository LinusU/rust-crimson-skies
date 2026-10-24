# M01: scripted AI aircraft can be hit and destroyed (#2100)

Task `M01-AI-AIRCRAFT-DAMAGE`; test prefix `accept_m01_ai_aircraft_damage_`
(`crates/cs_app/tests/campaign/m01_ai_aircraft_damage.rs`). Both tests need
`CS_GAME_DIR`.

## Problem

M01's win route destroys group 1 (`medkestrel_1..3`, block 13 `DEDG 1 <= 0`).
Every scripted aircraft registered an unresolved integrity pool
(`m01-actors.damage-pool`, no table measured, #668) and carried no
`PartSweptBox`, so a gun round had no volume to hit and a routed hit would
never destroy the actor.

## Designed values (not measured, not fitted to a capture)

| Value | Number | Claim | Provenance |
| --- | --- | --- | --- |
| Integrity pool, `airframe` node, lethal | 600 | `m01-ai-aircraft.designed-pool` | `designed` |
| Swept box half extents, metres | `[6, 2, 6]` | `m01-ai-aircraft.designed-box` | `designed` |

* The pool is fifty of the designed gun's twelve-unit rounds, so a few seconds
  of the player's gunfire destroy one aircraft. It is a design choice.
* The box is axis-aligned in the world axes (the sweep vocabulary has no
  oriented box), so it is a rounded-up airframe envelope. It is **not** sized
  from the airframe mesh bounds: none are recovered for the Kestrel.
* The box is a child entity of the aircraft body; its `NodeVisualTransform`
  follows the body every fixed tick (`compose_scripted_aircraft_hull_boxes`),
  the way the player's hull box does.
* Zeppelins keep the unresolved pool: only scripted aircraft take the designed
  one.

## Group count

The composed observation counted live `ScriptedAircraftActor` entities, and a
destroyed aircraft's entity stays in the world. The count now skips an actor
the damage authority holds as destroyed, so group 1 drops 3 -> 2 when
`medkestrel_1` dies, which is what `DEDG` reads.

## Rostering and the wake

A scripted aircraft is entered into the targeting store only when the AI arms
it, which happens when it is awake. A sleeping Kestrel therefore has no
declared relation to the player and the gun step refuses the pair; this change
does not alter that. The retail test releases the Kestrels through the
production `WAKEUP_ENEMIES` consumer, using a one-objective control program
that spells the same site OBJECTIVE23 spells. The real site is reached only
after a flown route (shipwreck, landing, `texdrop`, 10 s nap).

## Still unmeasured

* The original's per-airframe hull pool, armour and hit shapes for any AI
  aircraft (#668). The designed pool and box stand in for them and gate any
  fidelity claim about how long a Kestrel survives.
* Whether the original lets a sleeping aircraft be hit.
* Per-part damage on AI aircraft: only the single `airframe` node exists.
