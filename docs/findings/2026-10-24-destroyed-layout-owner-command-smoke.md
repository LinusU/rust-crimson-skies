# M01 destroyed-layout owner-command smoke (#2054)

`cs --cs-path <retail> --mission M01 --smoke-seconds 240 --smoke-convoy-kill --capture-dir <dir>`

The convoy-kill mode of #2047, now judged on the *visible node state* it
exposed: `sample_convoy_layout` reads each truck's effective
`InheritedVisibility` on the drawn `WorldSceneryNode` entities that belong to
its intact presentation and to its authored `destroyed` layout, before the
first shot and after the settle frames. The read is the entities' propagated
visibility — what the renderer would draw — never the layout pass's own
report, so a swap that wrote the wrong nodes still fails
`convoy_layout_swaps`.

`apply_ground_target_layout` (`mission_session::world_draw`) derives every
live `MissionGroundTarget`'s drawn state from the session damage authority
each frame: `Destroyed` hides the intact slots and shows the `destroyed`
ones; every other state asserts the authored initial layout. The slot sets
are bound per target in `mission_targets`: a target's intact set is its own
subtree minus the `destroyed` child's subtree and minus every nested bound
target's subtree (the convoy's trucks ride inside `susp_bridge`, so the
bridge's destruction cannot hide a truck that is still in play).

Run on the implementing commit (release build, retail, gpu): 5 checks, 0
failed, 2692 frames.

- `convoy_destroyed_by_fire_key`: 77 guns-key presses; destroyed at frames
  2526, 2550, 2574, 2598 per the damage authority.
- `convoy_observation_follows`: all four rows `in_play` before the first
  shot, all four `no_longer_in_play` at the end.
- `convoy_layout_swaps`: every truck began intact 3-4/3-4 shown, destroyed
  0 shown; at the end `t_truck01`..`t_truck03` read intact 0/3 shown,
  destroyed 3/3 shown, and `bridge_truck01` — which authors no `destroyed`
  child — read intact 0/4 shown with nothing drawn instead.
- `convoy_frames_drawn`: 988 / 989 per mille off black.
- `smoke_shots_delivered`: every requested framebuffer arrived.

Screenshots (private, not committed):
`private/evidence/T2054-CONVOY-KILL/frame_ck02501_convoy_intact.png`,
`private/evidence/T2054-CONVOY-KILL/frame_ck02688_convoy_destroyed.png`;
`report.json` beside them carries the per-node `layout_before` /
`layout_after` rows.

## What the screenshots show

The intact frame draws the convoy whole: three gun carriers and the red
tanker `bridge_truck01`. The destroyed frame no longer draws the intact
vehicles: the tanker is gone entirely (its record stores no `destroyed`
child — see below) and each gun carrier's spot now draws the smaller dark
wreck of its authored `destroyed` layout. This resolves the limitation
recorded in `2026-10-24-convoy-kill-owner-command-smoke.md` (#2047), where
the destroyed frame was pixel-identical to the intact one.

## What is still not the original's presentation (limitations)

- The swap itself is **designed**, never `verified_original`: the
  `healthy`/`destroyed` children and the world's damage couplings are
  measured reads, but the coupling semantics that would drive the swap in
  the original are unmeasured
  (`UnmeasuredFieldFamily::DamageCoupling`), and no original animation or
  timing is claimed. The layout swaps the frame after the damage authority
  reports `Destroyed`.
- `bridge_truck01`, `susp_bridge` and `craterlake` author no `destroyed`
  child. For them the pass asserts only that the intact layout stops
  drawing — `PresentedLayout::Removed`. What the original presents for the
  tanker (`fueltruck.zrd`'s `bridge_destroy01` fireball animation is named
  in `2026-10-24-convoy-kill-owner-command-smoke.md`'s notes) is
  unmeasured, and no wreck or debris is invented: `crate::debris` binds
  only authored debris, which no ground target carries.
- Nothing here is `verified_original` or `human_play`; the frames are the
  reimplementation's own windowed run.
