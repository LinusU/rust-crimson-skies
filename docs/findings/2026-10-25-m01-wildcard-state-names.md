# Wildcard `NAME1` state names in the animation join (#2095)

Date: 2026-10-25. Follow-up to `2026-10-24-m01-animation-health-consumer.md`. Capability: `retail`
(files only); nothing here ran the original executable and nothing is `verified_original`.

## Measured

`ZBD/C3/zrdr.zbd` (`hydrogen.zrd`) stores the tank records only as the `NAME1` pair
`hydrobombboom*` -> `cargozep1/underneath/hydrogentank*` (no `ANIMATION_NAME`). The payload
records in `C3/M01/mis_anim.zbd` are `hydrobombboom1..4` with objects `hydrogentank1..4`: the
stored name is the state prefix plus a suffix, the object is the path terminal's prefix plus the
**same** suffix.

## Rule implemented (designed, from that one correspondence)

`AnimationDefinitionSite::expands_state`: a definition without `ANIMATION_NAME` declares record
`P<s>` on object `Q<s>` when one of its `NAME1` pairs is `P*` over a path ending in `Q*`, for a
non-empty `s` equal on both sides. Only a lone trailing `*` is read. `resolve_animation` uses it
after the explicit and literal-name joins; `check_agreements` accepts the wildcard object only
under the same pairing. Result: `hydrobombboom1` starts, calls `hydrogen_fire`, and `call_others`
starts tanks 2..4.

## Not covered

Other wildcard shapes (inner `*`, text after `*`) stay undeclared. The `destroy_*` zeppelin
records were not compared against the retail data in this slice (the parts are not ground
targets yet; see #2096), so the suffix-pairing rule is confirmed on the hydrogen tanks only.
