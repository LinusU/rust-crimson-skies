# #2222 the ground blow's two per-vehicle gates

Date: 2026-11-01. Task #2222, follow-up of #2107
(`2026-10-29-groundblow-assist-model.md`, section 2). Static claims only
(`OWNER-STATIC-2026-10-29`, bytes in the #2107 finding); the original
executable was not run, nothing here is `verified_original`. Implementer:
`sonnet-2`; not independently reviewed.

## Wiring

Both gates are inputs to the law, not state it owns:

* `OriginalInput::status_word` (`V+0x358`): `INCAPACITATED_STATUS` (4) runs no
  ground blow for either seat (`0x48c22b`).
* `OriginalInput::collision_grace_until_s` (`V+0xac`) with
  `OriginalInput::sim_time_s` (`[0x71c470]`): a **non-player's** blow is skipped
  while `sim_time_s < collision_grace_until_s` (`0x48c241`). A player is never
  gated by it. The default `0.0` never gates.

`OriginalFlightModel::rotation_increment` applies `ground_blow_gated` before
the block runs, so a gated vehicle gets neither the torque term nor the
velocity steering. Input fields were chosen over `OriginalState` because the
law keeps no clock and the only exhaustive `OriginalInput` literal outside
tests is `playtest::scene::PlaytestScene::input`, which sets all three to the
disarmed values.

## Still not wired (affected content, resolving task)

Both fields are written now (below). What remains is not a writer but
coverage: `+0xb4` damping stays unmodelled (no site assigns it a non-zero
value), and no retail mission yet reaches either gate, because the mission
ordnance step supplies no blast recipients and no bound loadout carries a
`SONIC`/`FLASH` record (see the #2252 limit in
`2026-10-28-t452-r05-slot-map-and-incapacitation.md`). No constant was tuned.

## Wired by #2251 (`V+0xac`)

Static claims only, not `verified_original`; the bytes are the ones above
(`0x48d31e`..`0x48d395`, `now + 1.0`, both vehicles), no new image read.

* `crates/cs_app/src/playtest/collision_grace.rs` keeps the deadline in the
  app's own `CollisionGrace` component (absent means `0.0`, never gates).
  `write_collision_grace` runs in `FixedLast` and, for every solid contact
  report of the tick whose two bodies are both on `CollisionLayer::Aircraft`
  and both carry a `PlaytestOriginalFlight`, writes
  `session_time_s(report.tick) + COLLISION_GRACE_S (1.0)` on **both**.
  Sensor overlaps and contacts with anything else write nothing. "Class 0/4
  vehicle" is read as the port's aircraft records; the port writes it for
  every aircraft pair (see "Settled by #2255" for what the bytes say about the
  player).
* `drive_original_flight` states `sim_time_s` (fixed ticks times the fixed
  timestep, converted once) and the record's deadline in every record's
  `OriginalInput`; the law alone decides who is gated, so a player stays
  ungated by construction.

## Settled by #2255 (which pairs reach the `V+0xac` writes)

Static claims only (`OWNER-STATIC-2026-10-29`), not `verified_original`; the
byte windows are pinned by `accept_groundblow_grace_pair_*`
(`crates/cs_app/tests/campaign/accept_groundblow_grace_pair.rs`, ignored
without `CS_ENGINE_IMAGE`). The function is `0x48d2c0`, `this` = `esi` = the
stepping vehicle `V`, argument = that vehicle's own sweep result `H`.

* `0x48d2ed 3b3598c27100` `cmp esi,[0x71c298]` (the player); `0x48d2fb
  0f84a9000000` `je 0x48d3aa`. **When `V` is the player the whole block
  `0x48d301`..`0x48d39b` is skipped: the player's own step writes no grace.**
* Otherwise `ebx = H.object` (`[H+0x24]`, `0` skips); `0x48d31e f7432800000040`
  `test [ebx+0x28],0x40000000` (clear skips); the object is looked up in the
  vehicle registry `0x71dab8` (`call 0x4afee0`: first node whose `[node+8]->[+0xc]`
  equals the key), walking `[ebx+0x58]` children while it misses. A miss skips.
* `0x48d360 8b887c060000 85c9 7405 83f904 752c`: the **other** vehicle `O`
  (`eax`) needs `[O+0x67c]` = 0 or 4, else skip. There is **no class test on
  `V`** and **no player test on `O`** in the window.
* `0x48d36f..0x48d395`: `[esi+0xac] = [0x71c470] + [0x6032dc]` and
  `[eax+0xac] = [0x71c470] + [0x6032dc]` (`now + 1.0`), both unconditionally
  once the checks above pass; `[ebp-0xe]` is set to 1 and `[ebp-0xd]` to 0.
* The two callers (`0x48ed8b`, `0x490b08`) are both `mov ecx,edi; call
  0x48d2c0` with `H` a stack local of the stepping vehicle: **each vehicle
  runs the response for its own sweep hits.**

Answer:

* **AI/AI**: any step of either AI whose own hit resolves to the other (class
  0/4) writes both. Port rule matches.
* **Player-into-AI**: the player's step (`V` = player) writes nothing. The AI
  gets `V+0xac` only from the AI's own step, when its own sweep hit resolves to
  the player (the player is a valid `O`: no player test on `O`, and the player
  is a class 0/4 vehicle). The AI side alone is written by that step (the
  write to the player's `+0xac` is dead, the law never gates the player).
* So a player/AI contact takes the branch exactly when the AI's own sweep
  reports it. The port's single symmetric contact report writes the AI for
  every such contact, so **no code changed**.

## Settled by #2256 (is the AI's own sweep blind to the player?)

Static claims only (`OWNER-STATIC-2026-10-29`), not `verified_original`; the
bytes are pinned by `accept_groundblow_grace_sweep_*` (same file as the #2255
pins, ignored without `CS_ENGINE_IMAGE`). `0x45e470` named in #2255 is not a
hit search (it is a lookup in the map at `0x71b1e8`); the sweep is
`0x48d7f0`, called as `mov ecx,edi; call 0x48d7f0` from the stepping
vehicle's own step (`0x48ea17`), and its result is the `H` of `0x48d2c0`.

* `0x48d7fd d90570c471 00 .. d89eac000000 .. dfe0 f6c401 740f`: the sweep's
  first act is `now` against the stepping vehicle's own `[V+0xac]`; inside
  the grace it returns without searching. This is symmetric (no seat test),
  and it makes the `[eax+0xac]` write of `0x48d395` live for a player `O`: the
  player's own sweep is skipped for that second. The law's gate stays
  non-player only (`0x48c241`); this is a separate effect of the same write.
* `0x48da06 e8 65b50300` (`0x4c8f70`) and `0x48da25 e8 f6c80300`
  (`0x4ca320`): the searches take the world `[0x71c2ac]`, the vehicle's
  sweep box and an output buffer; no vehicle or seat argument.
* `0x48da3e a198c2710083c4083bf00f84bb010000`: the only player test in the
  whole sweep (`0x48d7f0`..`0x48e540`), after both searches. `esi` is the
  stepping vehicle; it picks which result post-processing runs, not whether
  a result exists.
* The search routines `0x4c8f70`..`0x4cca30` (containing `0x4ca320`, which
  walks sector nodes filtering on node flags `[n+0x24]&4`, `&0x10`) contain
  no reference to `0x71c298`.

Answer: the bytes show **no asymmetry** in the hit search. The AI's sweep
reports whichever scene nodes are flagged into the search, by the same code
the player's sweep uses, so a player/AI contact reaches the AI's `0x48d2c0`
exactly when the player's node is flagged in. Whether it is (a per-node flag,
data set elsewhere) is not decided by this window and is **not guessed**; the
port keeps the symmetric write, so **no code changed**. Affects the 1.0 s
gate of an AI rammed by the player only if the player's node is unflagged,
which would need a runtime or owner capture to see.

Not wired (affected content: the player after an AI-initiated aircraft hit;
no resolving task yet): the port does not skip a vehicle's own contact
reaction while its `V+0xac` is live (`0x48d807`); it only feeds the law's
ground-blow gate.

## Wired by #2258 (the sweep's grace skip)

The "Not wired" item above is wired (static, `OWNER-STATIC-2026-10-29`):
`collision_grace::sweep_skipped` is `now < V+0xac`, and the player's terrain
contact judge (`fatal_contact::judge_terrain_contacts`) and the aircraft
contact writer skip a vehicle inside its grace (`0x48d7fd`). A contact
renews the grace only while a vehicle whose step can write it sees it: the
sweep is blind inside that vehicle's own grace, and the response writes
nothing when the stepping vehicle is the player (`0x48d2ed`, "Settled by
#2255"), so a fresh player meeting a still-graced AI renews nothing while
an ungraced AI renews both vehicles of the pair. Pins:
`accept_sweep_grace_skip_*`. Only the player and class 0/4 aircraft pairs
have a port consumer; other contact reactions are unported.

Limit (affected content: a sustained aircraft contact that straddles the
deadline; no resolving task yet): `ContactReports` reports a pair once per
contact episode, so a contact whose start met only blind sweeps — or that
outlives the deadline while still touching — is never re-seen. The
original's per-tick sweep would find it again on the first tick after the
grace ends.

## Wired by #2252 (`V+0x358`)

Static claims only, not `verified_original`; the bytes are the ones in
`2026-10-28-t452-r05-slot-map-and-incapacitation.md`, no new image read.

* `playtest::scene::overlay_incapacitation` reads the mission ordnance
  session's status ledger (the one writer of
  `StatusEffectKind::Incapacitation`) and, while
  `incapacitation::incapacitated_mode` reports the mode for the record's
  `DamageActorBinding` actor, states `status_word = INCAPACITATED_STATUS` (4)
  and passes the six control words through
  `incapacitation::effective_controls` (all zero). Otherwise the word is `0`
  and the record's own controls stand.
* `drive_original_flight` applies it after the ground-blow probe overlay, so a
  stunned aircraft runs neither its controls nor its ground blow until the
  measured deadline tick, on which the ledger's expiry pass clears the mode and
  both come back. A playtest with no ordnance session or no
  `DamageActorBinding` (the synthetic and free-flight scenes) overlays nothing.
* The player seat never reads an incapacitated mode: the measured exposure
  hands `0x71c298` a screen/audio effect and no control loss, so
  `overlay_incapacitation` leaves a player's word at `0` and its controls
  intact whatever the ledger holds. The screen and the sound themselves still
  have no presentation consumer.

## Tests

`accept_groundblow_grace_*` in `crates/cs_app/tests/campaign/accept_groundblow_grace.rs`
(two synthetic aircraft in contact through the production headless app, lockstep
against an unprobed run); `accept_ground_blow_gate_*` in `crates/cs_sim/src/flight/original.rs`, calling
`OriginalFlightModel::step`; the existing `accept_ground_blow_` suite passes.
