# F20-c: `AnimationEvent` carries the crossing's clip time (#673)

**Decision:** `AnimationEvent` gains `clip_time: u64`, the absolute clip time at
which the marker was authored to fire (`marker.tick + pass * duration`,
unwrapped across loops like `TickOutcome::clip_time`). `AnimatedObject::advance_to`
stamps it from the candidate instant it already computed.

**Why:** `AnimationEventId.tick` is the *session* tick of the publishing
advance, so a skip past a marker reported the tick it jumped to. The new field
reports the marker's own position, so a mission can say which frame of the
cinematic fired.

**Not in the id:** `clip_time` is a field of the event, not of `EventId`, which
is the shared contract identity. The id's encoding and the activation identity
(live instance + marker) are unchanged.

**Ripple:** the only construction site outside `advance_to` is the restamping in
`accept_f20_c_marker_consumer`, which uses struct update and keeps
the field. `RaisedMarker` exposes it as `clip_time`; the skip test now asserts
it equals the marker's authored tick alongside the unchanged session-tick `at`.
