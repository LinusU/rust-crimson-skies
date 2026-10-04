# F57-B: interpolation and bounded local prediction — design notes and limits

Date: 2026-10-04. Task: F57-B. Capabilities: ordinary build/test only; no original
data. Every number is newly authored engine design; no original interpolation
delay, extrapolation rule or correction behavior is measured or asserted. A pass
awards **checked** at most.

## What was added (`crates/cs_app/src/network/physics.rs`)

- `RemoteInterpolator`: bounded per-actor jitter buffer keyed on (actor,
  generation). One generation per actor; a newer generation drops the old track,
  an older or ended one is refused (AC04). Pose blends between the two samples
  around `now - delay_ticks`; position is lerped, orientation nlerped. Rounds,
  boost capacity and integrity are the earlier authoritative record's, never
  blended. `observe` feeds it from an `IngestReport`, retiring destroyed/despawned
  actors so no ghost remains (AC01).
- Bounds in `InterpolationConfig` (defaults 60 Hz, 6-tick delay, 16 samples,
  6-tick extrapolation, 30-tick gap, 1000 m teleport): lost snapshots are bridged
  by blending across the gap up to `max_gap_ticks`; a gap or displacement above
  the bounds holds the older pose until the newer tick; extrapolation stops at its
  limit (`ExtrapolationExhausted`).
- `LocalPredictor`: keeps a bounded history of the pose the single pose owner (the
  body) had, compares it with a server record at the same tick, and returns
  bounded per-tick `PoseCorrection`s (smoothed over `correction_ticks`, or a snap
  above `snap_distance_m` / `snap_angle_rad`). It never owns the pose. Ammunition,
  boost capacity and integrity are written only from a server record
  (`AuthoritativeLoadout`); the predicted boost is a cosmetic flag (AC02).

## Limitations (stated, not hidden)

- Not an exact rollback: Avian state is not rewound. Velocity is not corrected, so
  a persistent error ends in a snap rather than a re-simulation.
- A record whose tick is missing from the prediction history is adopted by snap
  (`error_m` infinite); the caller must apply the record's pose.
- Orientation is nlerped, not slerped: constant angular speed is not preserved over
  large per-sample rotations.
- The estimated server tick (`now`) and the delay are inputs; clock estimation is
  not done here.
- Not wired into the Bevy schedule, no origin-epoch change across snapshots
  (F57-C, AC03), no projectile confirmation (F57-C). Latency/loss measurement under
  real conditions is F57-D (`network_real`).
