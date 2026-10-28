# T452-REG-SMOKE-RADIUS: the smoke emitter does have a gameplay radius

Task #2180. Static analysis of the owner-supplied decrypted engine image
(base `0x400000`) and a read of the retail `player.zrd`. Provenance `static`;
nothing here is original-run behavior and nothing is `verified_original`.

## What T452.R02 said, and why it was wrong

`docs/findings/2026-10-28-t452-r02-ordnance-effects-fuses-and-swarm.md` §1
stated "no code reads a gameplay radius" for `SMOKE_SCREEN` after following the
node constructor `0x4b8d50`. That covered construction only. The node is kept in
a list (`0x71dbbc`, count `0x71dbc0`) and every list reader is in
`0x4b5840..0x4b9423`. One of them, the per-step update `0x4b8fd0`, is the
consumer.

## The consumer (`0x4b8fd0`)

Per step it subtracts the step time from the node's remaining time (`+0x10`),
then walks the actor list (`0x71dabc`). For each live actor other than the
node's owner it:

1. takes the owner-to-actor vector (`0x4b90cc`..`0x4b90fa`) and its length
   (`0x422690`, a square root), and skips the actor when the length is not below
   the global at `0x71c3dc` (`0x4b9106`);
2. dots that vector with the owner's forward axis (`owner+0x198`) and skips the
   actor when the result is not above the global at `0x71c3e0` (`0x4b914d`);
3. for a non-player actor, calls `0x4200d0` with the global at `0x71c3e4`
   (`0x4b9171`); for the player, calls `0x42e9d0` with a fixed colour and a 2 s
   re-arm stored at node `+0x14` (`0x4b91d3`).

## Where the radius comes from

Not from the weapon record. The loader at `0x473e91..0x473f1b` looks three keys
up in the `player.zrd` global record (`0x57a090`) and stores them:

| key | stored at | image default if absent | retail value |
| --- | --- | --- | --- |
| `smokescreen_stun_range` | `0x71c3dc` | 200.0 m (`0x43480000`) | **600.0 m** |
| `smokescreen_stun_angle` | `0x71c3e0` as `cos(angle * pi/180 * 0.5)` | cosine 0.8 | 170.0 deg |
| `smokescreen_stun_interval` | `0x71c3e4` | 3.0 s | 5.0 s |

The retail values sit beside `nom_gravity` in the first `player.zrd` entry and
are read by `OriginalDocuments::smokescreen_stun` (`cs_content`).

## Resolution

- `t452_r02.limit.smoke_radius` is **resolved as a measured value, not as an
  absence**: `wep_13`'s area radius is the `smokescreen_stun_range` global
  (`MeasuredAreaSource::GlobalKey`), 600 m in retail. `T452_04_AREA_RADIUS` is
  resolved for `wep_13`; no measured record keeps the old claim.
- The registration of `wep_13` still carries no `AreaEffect`, and now says why
  accurately: the area does not apply damage. It stuns actors in a cone
  (`0x4200d0` for AI, `0x42e9d0` for the player). The runtime has no variant for
  either, so a new claim `t452_r02.limit.smoke_stun_effect` is kept, resolved by
  follow-up task `T452-REG-SMOKE-STUN`.
- Not measured here: what `0x4200d0` and `0x42e9d0` do to the struck actor, and
  whether the 170 deg angle is a full or half cone (the image halves it before
  the cosine, so the test is against a 85 deg half-angle, which is what the
  measured rule states).

## Tests

`accept_t452_smoke_` in `crates/cs_content/tests/accept_t452_smoke.rs`: a pure
measurement test, a retail test (`CS_GAME_DIR`) pinning 600 / 170 / 5, and an
image test (`CS_ENGINE_IMAGE`) re-reading the consumer windows and key strings.
