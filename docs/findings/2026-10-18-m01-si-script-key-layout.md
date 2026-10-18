# M01 SPLINE_INTERP (.zan) key layout stored inline in `mis_anim.zbd`

Task: Rally #1659 (`PARITY-M01-ATMO-E1`). Feeds #1652 (start pose after the
launch cinematic) and #1660 (pose and velocity at the end of it).

Reader: `cs_formats::zbd::si_script` (`read_si_scripts`, `find_si_script`).
Tests: `accept_parity_m01_atmo_e1_*` in `crates/cs_formats/tests/zbd/si_script.rs`.
Evidence class of everything below: `observed_tool` (measured from the retail
bytes with this repository's reader; no original run, no disassembly read).
Nothing is `verified_original`.

## What is stored

Each `OBJECT_MOTION_SI_SCRIPT` event is followed in `mis_anim.zbd` by

```
path    NUL-terminated  "..\data\common\zrdr\zeps\<script>.zan"
object  NUL-terminated
key*    back to back; no count, no terminator
```

C3/M01 holds 13 scripts (offsets are into `ZBD/C3/M01/mis_anim.zbd`):

| script | object | keys | span |
| --- | --- | --- | --- |
| td_player1 | player | 8 | 867584 + 1364 |
| td_player2 | player | 10 | 868948 + 1008 |
| td_cam | camera1 | 18 | 869956 + 1709 |
| td_chute | chutemanparent | 18 | 871665 + 3010 |
| td_chute | pilot | 18 | 874675 + 2013 |
| pzhookup_cam1a | camera1 | 12 | 877713 + 2020 |
| pzhookup_cam1 | camera1 | 43 | 879733 + 7103 |
| pzhookup_cam2 | camera1 | 48 | 886836 + 7923 |
| pzhookup_cam1b | camera1 | 40 | 894759 + 6612 |
| pzhookup_playerhold | player | 12 | 901371 + 808 |
| pzhookup_player | player | 23 | 902179 + 3824 |
| pzhookup_impact | hook_base | 23 | 906003 + 2155 |
| pzhookup_impact | top_seg | 23 | 908158 + 2381 |

`td_*` are the Klondike drop-off, `pzhookup_*` the launch hook-up (as the task
warns). From `pzhookup_cam1a` on the scripts abut exactly: the next script's
path starts at the previous script's last key's end byte. **There is no
closing record.** 1025 bytes separate `td_chute/pilot` from `pzhookup_cam1a`;
they are not walked here (other event data, undecoded).

## The key record (measured)

| offset | size | field |
| --- | --- | --- |
| +0 | u32 | `tag` in 0..=3: bit 0 position block present, bit 1 orientation block present |
| +4 | f32 | `t0` seconds; the first key of every script has `t0 == 0.0` |
| +8 | f32 | `t1` seconds, `t1 > t0`; next key's `t0` is bit-identical to this `t1` |
| +12 | 76 | position block (tag bit 0) |
| next | 76 | orientation block (tag bit 1) |

Key sizes 12 / 88 / 88 / 164 bytes for tag 0 / 1 / 2 / 3. Tag counts over all
61 animation containers (1068 scripts, 75 461 keys): tag 0 8 429, tag 1 6 088,
tag 2 8 825, tag 3 52 119. Key intervals are 1/3 s in `pzhookup_player`
(23 keys, 0 .. 7.667 s) and 0.25 s in `td_*`.

Each 76-byte block is 19 f32: `start` (3 or 4), `mean_rate` (3), then three
groups `[c0 c1 c2 c3]`, one per axis.

### Position block (tag bit 0)

* `start` = `x y z`, followed by a fourth word that is **uninitialised
  garbage** in retail (`0x7c00_0001`-style denormals, 1.8e31 occur). Kept as
  `padding_word`, never interpreted.
* `P_axis(s) = c0 + c1 s + c2 s² + c3 s³`, `s` seconds after `t0`.
  `c0 == start` on every axis. `P(t1 - t0)` equals the next key's `start`
  to 5e-7 on `pzhookup_player` and to 2.3e-5 relative over all containers.
* `mean_rate = (P(t1) - P(t0)) / (t1 - t0)` (exact to f32 rounding). It is
  *derivable*; it is stored, not needed.

### Orientation block (tag bit 1)

* `start` = unit quaternion `w x y z` (norm 1.0 in all 23 `pzhookup_player`
  keys).
* `r_axis(s) = c0 + c1 s + c2 s² + c3 s³` with `c0 == 0`; `mean_rate =
  r(t1 - t0) / (t1 - t0)` to 7e-9.
* The next key's `start` is `exp(r(t1 - t0)) ⊗ start`, with `exp(v) = (cos|v|,
  sin|v| · v/|v|)` (so `r` is a *half-angle* vector), or its negation.
  Measured: 4.4e-7 over `pzhookup_player`; **9.2e-4 worst over C3/M01** (the
  `td_chute` x-axis rotation, rate ≈ 1.3 rad/s) and **1.6e-2 worst over all
  containers**. The fit being exact only for small rotations leaves two
  readings open: the exporter's cubic is a fit that does not close
  exactly, or the true map differs from `exp` at larger angles. Alternatives
  tried and rejected on `td_chute/pilot`: `normalize(1, r) ⊗ q` (1.2e-2),
  `(sqrt(1 - |r|²), r) ⊗ q` (5.6e-3), half-angle doubled/halved (1e-2 and
  worse). **Not decided.**

## `pzhookup_player` (the launch)

23 keys, all tag 3, 164 bytes each, tiling 902 231 .. 906 003. First key:
`P(0) = (-0.08879, -1.79950, 9.96528)`, identity-ish quaternion
`(0.99995, -0.00105, 0.00058, -0.00962)`. These are the stored start values of
the **script** in its own frame; #1652/#1660 must still establish the frame
(see Unknown).

## Unknown (with the content it affects)

1. **Coordinate frame / parent.** Not stated in the key data. Affects where the
   launch pose lands in the world (#1652, #1660). The object name (`player`,
   `hook_base`, `top_seg`, `camera1`) is the only hint; which node it is
   relative to is the event/animation graph's question.
2. **Behaviour of an absent block.** `pzhookup_playerhold` is tag 0 after its
   first key; `pzhookup_impact/hook_base` is tag 1 after the first; `top_seg`
   moves to tag 2. Whether a channel with no block holds, resets or is left to
   another source is not measured. Affects the launch hold and the impact
   camera.
3. **The position block's fourth `start` word** (garbage here; unknown whether
   the engine ever reads it).
4. **Units and what interpolation the engine evaluates.** The cubic and `exp`
   relations reproduce the stored keys; that the engine evaluates exactly
   these forms is unobserved. For rotation the residual above is unexplained.
5. **What follows the last key of the last script in a container** (e.g.
   `pzhookup_impact/top_seg` ends at 910 539): the reader stops where the chain
   rule fails and does not claim what comes next.
6. **Whether `t1` of the last key is the script's run time** or the event's
   own duration field joins it (the event record is not read here).
