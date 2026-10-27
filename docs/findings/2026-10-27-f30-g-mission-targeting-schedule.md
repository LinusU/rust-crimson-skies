# F30-G: the mission session's targeting runs through the rendered-frame schedule

Rally #2139 (key `F30-G`), follow-up of F30-C
(`2026-10-02-f30-c-hud-spyglass-and-weapon-guidance.md`) and of F30-F
(`2026-10-27-f30-f-occlusion-producer.md`, Rally #535). Implemented by
`swe2-max-1`; reviewed by `deepseek-1` with a fresh context. The F30 sheet has
no `F30-G` stage — this stage adds no original-behavior claim, only wiring, so
it awards at most **checked**.

## What changed

- `mission_session::MissionWeaponsPlugin` installs
  `targeting::TargetingSchedulePlugin`, so the five F30-C entries (teardown,
  roster sync, command edges, damage, consumers) and the `TargetConsumers`
  views they publish now run once per rendered frame in a mission session. The
  cockpit HUD (`ui/hud/frame.rs`) and the spyglass (`ui/hud/session.rs`) read
  exactly those resources, so the reticle, the spyglass window and the
  weapon-guidance aid the frame draws come from the schedule.
- `mission_session::targeting_input::publish_mission_targeting_frame` (new,
  `PreUpdate`, behind whichever key minter ran) publishes the frame's
  `TargetingFrameInputs`: the mission's own observer, the last committed
  `PhysicsTickLedger` tick, the command edges the shared queue has not yet
  served, and the `mission_crosshair` ray (crosshair cone plus the F30-F
  occlusion producer). During the launch cinematic it still publishes — the
  consumer pass is owed a fresh view either way — but carries no edges, the
  same gate the weapon publisher applies. With no session it removes the
  record, so the schedule reports `missing_inputs` and drops a dead session's
  views instead of spending a stale record.
- The tick's direct `apply_selection_edges` call in
  `publish_mission_weapon_input` is gone; `MissionWeaponReport::targeting_refusal`
  now mirrors `TargetingScheduleReport::selection_error`.
- `mission_session::weapons::publish_mission_target_damage` (new,
  `FixedPostUpdate`, after the weapon step and before the damage schedule)
  mirrors the gun step's resolved batches into `PendingTargetDamage`;
  `drive_mission_ordnance` pushes its detonation batches there beside the
  existing `PendingDamageEvents` path. `PendingTargetDamage::push` merges the
  several resolutions one tick can produce, so the frame's damage pass records
  every one of them before the consumer pass publishes.

## The edge queue is served exactly once

`MissionCockpitActions` is shared with the fixed-tick weapon publisher, which
takes the whole queue. The rendered-frame targeting pass must not drain it (an
edge may wait past one rendered frame for its fixed tick), so it **copies**
`actions[targeting_published..]` and advances the cursor; the tick's drain
resets the cursor with the queue it takes. An edge therefore reaches the
selection pass exactly once and the weapon publisher exactly once, whatever the
ratio of rendered frames to fixed ticks.

## A roster record's removal belongs to the pass that wrote it

Installing the schedule runs `sync_targetable_roster` in the mission for the
first time. Its old removal loop unregistered every store record without a live
`TargetableBinding`, and the mission's roster passes (`mission_ai.rs`,
`mission_actors.rs`, `turret_fire.rs`) register their actors directly with no
binding entity — so the first scheduled frame would have unregistered the whole
mission roster. The pass now keeps the `(session, scene generation)` it synced
under and the actors its own `register` calls added, and unregisters only
those whose binding left the world. F30-B's binding semantics are unchanged;
a record another producer wrote is that producer's to keep or drop.

The store's own rules are untouched: `cs_sim::targeting` (and its tests) are
not modified by this stage.

## Known limits (unmeasured, not claimed)

- Whether the original evaluates faction, reveal and selection at render rate or
  at tick rate is unmeasured (F30-E). This stage selects once per rendered
  frame because that is where the command edges are minted; a frame that runs
  several fixed ticks applies no second edge and runs one damage pass.
- The frame's crosshair ray is produced in `PreUpdate` from the camera's last
  propagated transform and the bodies of the last committed physics step: one
  fixed tick behind the pose the frame renders.
- The published views are the F30-C resources; this stage makes no pixel claim
  of its own beyond the owner-command smoke below.

## Checks

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features --locked -- -D warnings` on the reviewed commit: clean.
- `cargo test --locked -p cs_app`: green (every binary and the doctests).
- `cargo test --locked -p cs_app --test cs_app_tests_d -- accept_f30_g_
  --include-ignored`: 2 passed.
- Retail modules the change can reach, run with `CS_GAME_DIR`:
  `accept_m01_convoy` 17 passed, `accept_m01_play_` 100 passed,
  `accept_parity2_m01_target_indicators` 9 passed,
  `accept_m01_destroyed_layout` 5 passed.
- Owner-command smoke on the reviewed commit —
  `cs --cs-path <retail> --mission M01 --smoke-seconds 45 --capture-dir
  private/evidence/M01-OWNER-SMOKE`: exit 0, 4082 frames, 7992 ticks, 73
  checks, failures none. The target checks (`target_selected`,
  `target_in_view_marker`, `target_off_view_arrow`, `target_spyglass_window`,
  `target_spyglass_toggle`, `target_non_aircraft_class`) all pass on it; the
  framebuffers are private and not committed. A scripted run is not
  `human_play` and none of it is `verified_original`.
