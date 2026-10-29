# T452-REG-TANGLER-ORIGIN: the tangler field is created at burst, at the projectile's position

Task #2201. Static measurement only (Capstone over `$CS_ENGINE_IMAGE`, base
`0x400000`); no original executable was run and nothing here is
`verified_original`. Rows: `[[tangler_origin_fact]]` in
`data/owner-static/engine-facts.toml`; test prefix `accept_t452_tangler_origin_`
(`crates/cs_content/tests/accept_t452_tangler_measurement.rs`).

## Measured

* `0x4b94e0` has exactly one direct caller in the image: `0x4ba6d1`, inside the
  TANGLER hook `0x4ba660` (closed by the ignored image test).
* The hook is the callback stored at `weapon+0x20c` by `0x5aec90`. Its caller
  is the impact/burst function `0x5ac7a0(weapon, record, rocket, override)`
  (`0x5ac7c7..0x5ac7d8`: `push rocket; push record; push weapon; call [weapon+0x20c]`),
  so the hook arguments are `(weapon, record, rocket)`.
* Constructor arguments: owner = the actor found by id `[rocket+4]` in the actor
  table at `0x71dab8` (`0x4afee0`); weapon = the hook's first argument; centre =
  the pointer `rocket+0x48` (`add edi, 0x48`, `0x4ba6ca`), whose three floats
  the constructor copies to `field+0x08`. `rocket+0x48` is the projectile node's
  position (the flak burst finding calls it the burst position).

## Consequence

The field's centre is the **projectile's position at the burst**, not the
launcher's actor record and not the launch origin, and it is created when the
projectile bursts or impacts, not when the launcher fires. The runtime still
opens it on the launch tick at the launch origin, so the origin cannot be
expressed in the launch order: that would encode a timing the image refutes.
`t452_reg.limit.tangler_field_origin` therefore stays named on `wep_12`, now
with the measured source recorded and a new resolving task,
`T452-REG-TANGLER-BURST-OPEN` (#2203), which moves the open to the burst.

Not measured here: which of the five callers of `0x5ac7a0` (`0x5ac2b1`,
`0x5ac410`, `0x5b0713`, `0x5b0bec`, `0x5b147c`) a given `wep_12` shot reaches;
the task above names that.
