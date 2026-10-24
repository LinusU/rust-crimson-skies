# M01 / C3: the `do_approach1..6` records are declared by unnamed definitions (#2098)

Date: 2026-10-24. Capability: `retail` (read-only `$CS_GAME_DIR`). Static reading plus a
composed run; not `human_play`, nothing `verified_original`. Tests:
`accept_m01_do_approach_declaration_` in
`crates/cs_app/tests/campaign/m01_do_approach_declaration.rs`.

## The refusal

`2026-10-24-m01-smoke-success-b.md` found every `do_approach1..6` landing activation
`Refused` with `PlayRefusal::Undeclared`, so `texdrop` (OBJECTIVE21) never ran.

## Where the names are declared

`zbd/c3/m01/zrdr.zbd` carries, right after the `enable_dropoff` / `disable_dropoff`
definitions, one more `ANIMATION_DEFINITION` per node:

```
ANIMATION_DEFINITION  NAME do_approachN  ACTIVATION ON_CALL  RESET_TIME
  SEQUENCE_DEFINITION  OBJECT_ROTATE_STATE (NAME do_direction)  CALL_ANIMATION (NAME texdrop)
```

These carry **no `ANIMATION_NAME`** field, only `NAME` (the object). The resolver matched
only `ANIMATION_NAME`, so the six names had no declaring site. The records exist in
`zbd/c3/m01/mis_anim.zbd` (indices 4..9) and decode (`OBJECT_ROTATE_STATE`, then
`CALL_ANIMATION texdrop`); `texdrop` itself is declared (`tex_drop.zrd`, `ANIMATION_NAME
texdrop`) and resolves.

## Measured rule

Over every mission scope of the installation (`zbd/c1..c5/m01..m05` that exist), for each
definition with no `ANIMATION_NAME` and exactly one `NAME` object that has a record in the
scope's carriers: the record's `anim_name` and `object_name` both equal that `NAME`.
C3/M01 has 128 such definitions, 50 with a record, 50 of 50 agree; every other scope
likewise agrees on every record it has (e.g. `sputter_firetrail`, `large_firetrail`,
`fuel_trail`). No counter-example in the installation.

`resolve_animation` (`crates/cs_app/src/animation/programs.rs`) therefore treats such a
definition as declaring the animation spelled like its object, **only when no definition
states the name by `ANIMATION_NAME`**. Several objects, wildcards and the `NAME1`
state-binding shape declare nothing by this rule; two unnamed definitions of one object stay
`Ambiguous`. The join's object-name and sequence-name agreements still run on the match.
This is `static` evidence over data, not a reading of the engine.

## Result

* All six `do_approachN` bind a single declaring site in `zbd/c3/m01/zrdr.zbd` with no
  refusal.
* A landing at `do_approach1` through the production landing path `Started` the record,
  its `CALL_ANIMATION texdrop` was `Started` by the call ledger, `texdrop` reached
  `EXECUTED` and OBJECTIVE21 (block 20) reached `Done` (composed headless run, block 20
  declared awake; not the owner-command smoke).

## Not established

* `do_direction` (the object of the record's first statement) is a node, not an animation:
  no record carries it as `anim_name`, so `resolve("do_direction")` stays unbound. Nothing
  calls it by name; it is the rotate target of the record's own `OBJECT_ROTATE_STATE`.
* The owner-command smoke (`--smoke-success`, #2082) has not been rerun on this change.
* Other names the rule newly binds in other missions were not run. One is known: C1C/M01's
  `letterbox` (called by `wv_initiate_hookup`) was refused `Undeclared` and now binds and
  starts; `accept_m01_play_triggers_anims_land_a_retail_landing_runs_the_row_and_the_gated_state`
  pinned the refusal and now pins the start.
