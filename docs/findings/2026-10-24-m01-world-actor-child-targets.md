# M01-WORLD-ACTOR-CHILD-TARGETS (#2096): ground targets that are nodes of a world actor

Date: 2026-10-24. Capability used: `retail` (read-only `$CS_GAME_DIR`) and ordinary
build/test. No original executable ran; nothing is `verified_original`. Test prefix:
`accept_m01_world_actor_child_targets_` (`crates/cs_app/tests/campaign/m01_world_actor_child_targets.rs`).

## Measured (installation read)

`hydrogentank1..4` bind through `mission_targets` with the authored path
`world1.cargozep1.move_zeppelin.tilt_zeppelin.rock_zeppelin.underneath.hydrogentankN`.
Their composed transform in the static C3 container is only the offset below the
zeppelin node (tank 1 origin `(15, -72.985, -70)`, target point `(15, -66.985, -70)`,
half extents `(6.24, 6, 16)`). `cargozep1` itself is placed by the mission program at
`(-6288, 300, -7701)`; before this task the tanks spawned at the container offset,
thousands of metres from the actor, so no round could reach them.

## Implemented

* `BoundTarget::frame` (`TargetFrame`): the node's composed world transform, its stored
  box and the composed world transform of each ancestor.
* `TargetFrame::under_anchor`: the target's pose with an ancestor standing at a given
  pose: `pose ∘ inverse(ancestor world) ∘ node world`, box corners carried through it.
  This is the composition the drawn subtree uses (`build_world_node_visual`:
  `inverse(root world) ∘ node world` under the actor entity).
* `mission_actors::reseat_under_zeppelin`: the spawn picks the nearest ancestor that is a
  spawned zeppelin's node and re-seats the target on that actor's pose. The ground
  target carries `MissionGroundTarget::anchor`.

## Designed / open

* The pose is read at spawn. Nothing writes a spawned zeppelin's transform after the
  spawn, so no per-tick follow exists; if a zeppelin is later made to move, the targets
  must follow (re-run the re-seating on the actor's pose).
* **Allegiance is untouched and blocks play.** The node resolver answers `neutral` for
  the tanks (subtree walk, `2026-10-18-m01-target-roster.md`), a neutral node is
  rostered with no relation, and the weapon step does not admit an undeclared pair under
  `HostileOnly`. The gun-burst test therefore declares player↔`neutral` hostile itself and
  pins that the *geometry* reaches the damage node. Whether the original opposes the player to
  the tanks (they are objective targets under an enemy zeppelin) is unmeasured; a
  follow-up task is filed. Affected content: whether a player can destroy
  `hydrogentank1` in retail M01 and so win "The Lost Treasure" (#2083).

## Side effects on recorded facts

* `rock_zeppelin` and `pzhookpoint` (nested under `piratezep`) are re-seated too. The
  convoy test's recorded overlap `rock_zeppelin x pzhookpoint` was an artefact of the
  container-origin placement and is gone; the four `x susp_bridge` pairs were already gone
  since #2061 dropped the bridge's enclosing box. `accept_m01_convoy_destruction_ground_box_overlaps_are_the_recorded_ones`
  now records no overlap.
* Three other retail tests fail identically on `main` and are not touched here:
  `accept_m01_lc_world_unit_roles_every_container_unindexed_split_is_measured`,
  `accept_m01_music_lazy_prepare_names_tracks_and_decodes_on_request` (13 tracks, 19 expected) and
  `accept_m01_play_ai_hostiles_close_and_betty_holds_station_within_n_seconds`.
