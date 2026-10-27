# F30-F: the crosshair's occlusion set has a producer

Task F30-F (Rally #535), follow-up of F30-D.

## What changed

- `cs_app::targeting::produce_crosshair_occlusion` asks an authoritative sight
  oracle about each eligible actor (never the observer) and returns an
  `OcclusionReport`. `OcclusionReport::crosshair_query` spends it on the
  session's `TargetStore::crosshair_query`. The store's rules are untouched:
  `CrosshairQuery::occluded` is still evidence the producer supplies and the
  query never invents.
- A foreign session generation is refused at both ends: a producer run for an
  observer of another session (`TargetError::ForeignSession`), and a report
  spent against another session's store. A cone refusal surfaces as the new
  `TargetingError::Crosshair`.
- `mission_session::mission_crosshair` now takes the observer and runs the
  producer over the physics world (`SpatialQuery`): a static body between the
  camera and an actor blocks it.

## Known limits (unmeasured, not claimed)

- Whether the original's reticle select tests occlusion at all, and against
  what, is unmeasured (F30-E). The physics oracle is the same answer the
  turret sight ray gives: static bodies other than scenery nodes
  (`WorldSceneryCollider`); colliders enclosing either end of the segment are
  ignored so an actor is not hidden by its own mount. Scenery therefore does
  not occlude.
- The segment runs camera to the actor's pose point, not to its extent.
- The mission publisher (`mission_session::weapons`) still called
  `apply_selection_edges` directly; it did not publish `TargetingFrameInputs`
  for `TargetingSchedulePlugin`, so the per-frame consumer pass was not run by
  the mission session. Filed as a follow-up task. **Resolved by F30-G (Rally
  #2139)**: the mission session installs the plugin and publishes the frame's
  inputs itself; see
  `2026-10-27-f30-g-mission-targeting-schedule.md`.
