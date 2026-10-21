# #1752 PARITY4-M01-COLLISION-RESPONSE: the original's contact response (static)

Date: 2026-10-21. Implementer: `claude-2`. Provenance: **`OWNER-STATIC`**
static reading of the owner's decrypted image `crimson.decrypted.exe` (image
base `0x400000`, disassembled with `llvm-objdump`) plus the retail
`player.zrd` values read through `OriginalDocuments`. Nothing here is an
observed behaviour and nothing is `verified_original`. The owner's measured
trajectory (task description, one Drydock run of retail 1.02) is a separate
evidence class and was **not** reproduced by this task (see "Not done").

## What the image does on a contact (player vehicle)

All addresses are virtual addresses in the image. `V` is the vehicle object
(`[[0x64ef78]+0x150]` for the player; the player test is `V == [0x71c298]`).

| VA | role |
| --- | --- |
| `0x48e580` | the vehicle's per-frame integration. Builds `delta = dt * v`, calls `0x48d7f0(V, &delta, &info)`, advances the position by the *clipped* `delta`, then calls `0x48d2c0(V, &info, severity)` when `severity > 0` (`0x48ed7e..0x48ed8b`). |
| `0x48d7f0` | the contact sweep and response. Moves the vehicle's collision-point list (`V+0x6a4..0x6a8`, 36-byte records: local point, new world point at `+0xc`, previous world point at `+0x18`) through the world's collision query (`0x4ca320`, `0x4c8f70`, grid cells of segments against world triangles; 44-byte contact records, up to `0x584` bytes per point). Returns `severity` in `st(0)`. |
| `0x48d2c0` | crash damage and effects for a contact of the given `severity` (below). |
| `0x48bf60` | **ground blow**: a look-ahead segment query along the vehicle (length `groundblow_elev`), returning a push along the surface normal scaled by `groundblow_mag` (`ai_groundblow` for AI). Called from `0x48c2b1`, inside the control law at `0x48c220`. |

### The player response in `0x48d7f0` (`0x48dc09..0x48e517`)

Reconstructed from the instruction stream; each step cites the address that
does it. `n` is the contact normal of the chosen contact record.

1. **Contact choice.** For every collision point with contacts, a score is
   taken against the point list (`0x48dd0a..0x48de96`: a dot product of the
   contact's offset with each list point plus a per-contact term, minimum
   kept) and the contact with the greatest score (`-0x8`, initialised to
   `-FLT_MAX`) wins (`0x48ded3..0x48def5`). The exact score is not decoded
   here. One contact is responded to per sweep.
2. **No response while `V+0x384` is nonzero** (`0x48dfb8..0x48dfc8` jumps to
   `0x48e4ff`). What `V+0x384` is (destroyed or invulnerable) is not
   established here.
3. **Severity.** `severity = -dot(normalize(v), n)` where `v` is the vehicle's
   velocity (`0x48df6e..0x48dfc2`): 1 for a head-on hit, 0 for a graze, negative
   when moving away. It is the function's return value.
4. **Contact point and clipped step.** `p = contact + 0.03 * n`
   (`0x6080c4 = 0.03`); stored at `V+0x8f8` (point) and `V+0x904` (normal). The
   caller's `delta` is overwritten with `p - previous_point[i] - V[0x6b0]`, so
   the position advances only up to the surface.
5. **Response velocity of the point.** `r = new_point[i] - V[0x204]`;
   `u = v + 2 * (r x w)` with `w = V[0x16c]` (`0x48e0be..0x48e18b`, the
   doubling is `fadd st,st`). `t = -dot(n, u)` is the approach speed;
   `Dv = t * n` (`-0x70`).
6. **Angular share.** If `|r|^2 != 0`: `L = t * (r x n) / |r|^2`, carried
   through `V`'s rotation `[0x180..0x1a0]` to body axes, divided
   component-wise by the inertia `V[0x65c], V[0x660], V[0x664]`, and back to
   world axes (`0x48e212..0x48e35e`) giving `Dw` (`-0x3c`); otherwise `Dw = 0`.
7. **Split.** `k = 2.25 * |Dv|` (`0x608108`), `a = |Dw|`. If `k == 0` the
   linear share is 0 and the angular share 1; if `a == 0` they are 1 and 0;
   otherwise `lin = k / (k + a)`, `ang = a / (k + a)` (`0x48e3a3..0x48e429`).
8. **Bounce.** `bounce_factor` is `0x71c35c` (retail `0.6`). The linear
   velocity becomes `v + Dv * (1 + bounce_factor * lin)` (`0x48e429..0x48e486`,
   written to `V+0x924`, with `V+0x930 = |v|^2`, `V+0x934 = |v|`). The angular
   accumulator `V[0x160..0x168]` gains
   `Dw * (1 + bounce_factor * ang) * 0.5` (`0x48e4b9..0x48e4f9`).

There is **no friction term** and no tangential impulse: only the normal
component is changed. With a negligible angular share the linear rule is a
point-mass restitution of `bounce_factor` (`v_n' = -0.6 v_n`).

### Crash damage, `0x48d2c0`

Armed from the `crash` record of `player.zrd` (parsed at `0x473b90..0x473c3e`):

| key | retail value | global |
| --- | --- | --- |
| `armor_damage_range` | `[50, 300]` | `0x71c350` (first), `0x71c34c` (second) |
| `health_damage_range` | `[50, 300]` | `0x71c358` (first), `0x71c354` (second) |
| `bounce_factor` | `0.6` | `0x71c35c` |

(Image defaults, used when the record is absent: `200/15`, `200/15`, `0.8`.)
For a contact of `severity s` (`0x48d4c1..0x48d510`):
`armor = max(second * s^3, first)`, `health = max(second * s^3, first)`; both
are doubled when `-0xe` is set (the player object is the one hit,
`0x48d517`, factor `0x6034fc`). The pair is handed to `0x5abcf0` with the
impact point and normal. For the player a feedback value `min(|v| * s * 0.03, 0.15)` is handed to
`0x42c070` (`0x48d3cc..0x48d409`); its consumer was not followed.
What the pair is subtracted from, and in which unit, is not established here.

### Per-airframe crash keys

`death_min_alt`, `crash_horiz`, `crash_y`, `crash_chord_y`, `crash_min_elev`
are read per vehicle record into the airframe parameter struct at
`+0x3c..+0x50` (`0x42f8b0..0x42f949`). Their consumer was not located.

### Other global keys on the same path

`groundblow_elev` (retail `400`), `groundblow_mag` (`10`), `ai_groundblow`
(`0.5`) are parsed at `0x474308..0x474385` into `0x71c420`, `0x71c424`,
`0x71c428` and used by `0x48bf60` / `0x48c30f..`. Our flight law
(`cs_sim::flight::original`) has no ground-blow term; whether the measured
no-input flight over the sea is affected by it is **unknown**.

## What our build does today

Avian solves aircraft contacts with its default coefficients (no restitution
bound, friction on); `crates/cs_app/src/playtest/mod.rs` only counts them.
M01 has no sea collider: the HUD reports `obstacle 0 / ground 0` at y = 1 m.
So both the skim and the beach impact differ from the original by
construction.

## Answers to the task's open questions

* *Sea and ground response:* one rule (above) for every surface; the sea is a
  contact like any other once the query reports it. What the query returns for
  the water tiles (a triangle mesh at `y = 0`, or a plane) is **unknown**.
* *Bounce restitution, speed loss:* restitution `0.6` shared by the
  linear/angular split; speed loss is the normal component only.
* *Destroyed vs bounced:* contact damage is `s^3`-scaled with a floor of
  `50` (a graze at `s ~ 0.05` costs the floor); the destruction threshold
  against health is **unknown**, as is the `V+0x384` gate.
* *The 60 degree pitch-up:* nothing in the response adds a vertical impulse
  of its own. The response is along the contact normal, so a steep beach
  slope would throw the velocity up by itself; whether the measured 60 degrees
  is the slope's normal or an extra term needs the finer capture the owner
  offered (`V+0x924`, `V+0x8f8`, `V+0x904` every frame across t = 34..36).

## Not done (and why)

* **No code and no trajectory test.** The no-input flight does not yet match
  the measured table (#1746 records the causes: no lift from rest, sink,
  thrust, spawn speed), so a contact comparison from the hand-over would
  compare two different flights. Wiring the response needs (a) the collision
  point list of the vehicle (`V+0x6a4`, its source not located), (b) a sea
  contact source, and (c) the `crash` record imported with a consumer.
* No owner-command smoke frames were taken; nothing visible changed.
* The temporary dump example used to read `player.zrd` was not committed.
