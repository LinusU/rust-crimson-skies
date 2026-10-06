# F20-C: the AnimationLog multi-consumer seam

Date: 2026-10-06. Task: F20-C-animation-log-single-drainer (#671). Follows
`docs/findings/2026-10-05-f20-c-mission-marker-consumer.md`, which recorded the
hazard. Capabilities used: ordinary build/test only; no original data.

## The hazard

`AnimationLog` holds five record kinds, and its only mutating accessor was
`drain()`, a total `mem::take`. `MissionMarkerConsumer::drain` (#507) used it and
only *counted* the blocked-track and attachment records, so a render or
collision consumer running later in the same tick (as #509's composition could
arrange) would have found an empty log.

## Decision: option 1, per-kind partial takes

`AnimationLog` gains `take_markers()` (events, effect-blocked crossings, refused
advances), `take_blocked_tracks()` and `take_attachments()`. Each leaves the
other kinds in place. `drain()` stays for a host that is the only consumer.

- **Owners by kind.** Mission layer: marker records. Render consumer
  (F20-C.03's diagnostics): `BlockedTrack`. Attachment/collision consumer
  (F20-C.01's diagnostics): `AttachmentRecord`.
- `MissionMarkerConsumer::drain` now calls `take_markers()`. `MarkerDelivery`
  lost `tracks_blocked()`/`attachments()`: with a partial take the mission
  layer neither reads nor owns those records, and a count would be a stale peek.
  `drained()` now counts the marker records only.
- **Why not option 2 (fan-out).** It would make the mission step the hub for
  records it does not own and require a consumer registry nothing needs yet.
- **Why not option 3 (per-kind resources).** It splits one publication point
  into several for the same effect and touches every producer.
- **Once-per-transition rule unchanged.** Taking records never publishes any;
  producers are untouched. The new test advances further ticks after both takes
  and finds the log empty.

## Open consequence

A kind nobody takes accumulates, one entry per published transition (never per
tick). Until a render/attachment consumer exists, a host composing the plugin
(#509) must either run one or call `drain()` after the mission step. This is
unresolved by this task and belongs to #509 and the F20-C.01/.03 consumers.

## Test

`accept_f20_c_marker_consumer_leaves_the_other_record_kinds_for_their_consumers`
runs the real session with a door marker and an unknown-material clip, takes the
mission batch, and shows the second consumer still gets its `BlockedTrack`.
