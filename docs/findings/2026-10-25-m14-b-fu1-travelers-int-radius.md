# M14-B-FU1: TRAVELERS reads an int radius (M14 OBJECTIVE25)

Status: static (`$CS_ENGINE_IMAGE`) plus the installation's own record. Nothing
here is `verified_original`.

## Measurement

The TRAVELERS parse at `0x467a70` reads child 3 by type tag: tag 2 copies the
float bits (`0x467ca6`-`0x467cae`); tag 1 converts with
`fild dword [eax+0x24]; fstp dword [ebp+0x5b0]` (`0x467cb6`-`0x467cbe`), i.e.
i32 -> f32; tag 3 logs `Expecting real value, found string`; anything else logs
the reader-array variant. The value is squared in place (`0x467d11`). An int
radius is therefore the same as the float of the same value (up to f32
rounding for |n| > 2^24).

## Change

`cs_script::conditions::travelers` now accepts `Value::Int` for the radius,
converted `i32 -> f32 -> f64`, in subject and counting mode. Text, lists and
non-finite floats are still refused.

## Effect on the census

`zbd/c2b/m04` (M14, `objectives.zrd`): block 25 spells `1000`; with the change
all 35 conditions lower, all 254 calls bind, the program validates and the row
is complete. `campaign_ready()` stays false.

## Other incomplete rows: different causes

* `zbd/c4/m01`: objective 23 calls 1-4 are refused as unknown host calls
  `Change`, `to`, `mobile`, `net` — an unquoted phrase split into words, not a
  radius. All 37 conditions lower.
* `zbd/c5/m04`: objective 18 call 1 is refused as unknown host call `SET_AI_`
  — a truncated key. All 63 conditions lower.

Neither involves TRAVELERS; follow-up tasks record them.
