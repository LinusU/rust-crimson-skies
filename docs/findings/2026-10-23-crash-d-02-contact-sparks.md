# Crash D.02: contact sparks from `yellow_sparks_follow` (#1945)

Status: implemented; several meanings stay **unknown** (listed below).

## What is read (retail, `ZBD/zrdr.zbd`, member `pufftrails.zrd`)

Animation `yellow_sparks_follow` (object `yellow_spark_02`). Its sequence
`fly_flare` calls, in order: `sparktrail` and `chiptrail` (no offset),
`stop_chiptrail` at `EVENT_OFFSET 0.15`, `stop_p1trail` at `EVENT_OFFSET 0.25`.
`stop_chiptrail` sets `chippuffer1` `INACTIVE`; `stop_p1trail` sets
`trailpuffer2` `INACTIVE` and then the object `yellow_spark_02` `INACTIVE`.

| field | `trailpuffer2` | `chippuffer1` |
| --- | --- | --- |
| `AT_NODE` | `INPUT_NODE 0 0 -0.5 0 0 0` | `INPUT_NODE 0 0 -1 0 0 0` |
| `TIME_INTERVAL` | 0.03 | 0.03 |
| `LOCAL_VELOCITY` | 3 1 3 | 2 1 2 |
| `WORLD_VELOCITY` | – | 0 0 0 |
| `MIN`/`MAX_RANDOM_VELOCITY` | −5 / +5 per axis | −5 / +5 per axis |
| `WORLD_ACCELERATION` | 0 −9.8 0 | 0 −9.8 0 |
| `FRICTION` | 6 | 6 |
| `SIZE_RANGE` | 0.01–0.08 | 0.02–0.06 |
| `NUMBER` | 16 | 6 |
| `LIFETIME_RANGE` | 0.4–0.7 | 0.3–0.7 |
| `GROWTH_FACTOR` | 0 | 0 |
| `DEVIATION_DISTANCE` | 0.1 | 0.1 |
| `FADE_RANGE` | 100–300 | 100–300 |
| `TEXTURES` | `exp_yel01`, `poleflare` | `smoke101..103` |
| `COLORS` | – | (0: 5 5 5 1) (0.05: 5 5 5 0.8) (1: 0 0 0 0.3) |
| switch-off | 0.25 s | 0.15 s |

The `sound` sequence (an `IF RANDOM_WEIGHT 0.5 … ELSE … ENDIF` choosing among
`snd_ricochet1..4`, most with `EVENT_OFFSET 0.25..0.3`) is not played: the
existing audio path takes no named sound without new decoding, and the
branch structure (several `SOUND`s per branch) is not decoded.

`contact_sparks.rs` reads all of this at stage build
(`read_contact_spark_definition`); the retail test asserts the numbers above.

## What is applied

`NUMBER` particles per emission; `SIZE_RANGE` as metres, `LIFETIME_RANGE` as
seconds; per-axis `MIN`/`MAX_RANDOM_VELOCITY`; `WORLD_ACCELERATION` as metres
per second squared in the world frame; one of `TEXTURES` per particle;
emission at the contact point of every `TerrainContactJudged` (fatal or not).

## Designed (claim `parity4-crash-d-02.designed-spark-motion`)

* Interval versus `NUMBER`: `NUMBER` particles are emitted every
  `TIME_INTERVAL` seconds from the contact until the switch-off time (9
  emissions of 16 for the trail, 5 of 6 for the chips).
* `FRICTION` is a linear drag rate, `dv/dt = a − friction·v`.
* Linear fade over life; camera-facing quads.

## Unknown (not applied)

* Units and frame of `LOCAL_VELOCITY`; whether it adds to or scales the random
  velocity, and whether its frame is the contact normal or the aircraft.
* The six `AT_NODE` words after `INPUT_NODE` (offset along z −0.5 / −1 is
  plausible but unmeasured), so sparks start at the contact point exactly.
* Units of `FADE_RANGE` (100–300 reads as a distance fade) and
  `DEVIATION_DISTANCE`.
* `COLORS` scale (r,g,b up to 5 with retail 0..255 elsewhere) and the blend
  mode, hence no tint and plain alpha blend.
* Whether `TIME_INTERVAL` repeats `NUMBER` or the whole emission is a single
  burst, and how long the object `yellow_spark_02` lives after `INACTIVE`.
* The ricochet sound choice (above).

Affected content: how bright, large and long the contact sparks look in M01;
the `size` of 0.01–0.08 m is tiny as stored, so the sparks are barely visible
at flight distance until the unit of `SIZE_RANGE` is measured.

No framebuffer smoke capture was taken for this task; the sparks are covered by
the production-plugin tests only.
