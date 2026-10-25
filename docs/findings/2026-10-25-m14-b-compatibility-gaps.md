# M14-B: M14's control program and its compatibility gaps

Status: static measurement of the installation's own record
(`ZBD/C2B/M04/zrdr.zbd`, census row `zbd/c2b/m04`) plus the engine-image
reading recorded in `2026-10-25-m14-b-fu1-travelers-int-radius.md`. Nothing
here is `verified_original`.

## What the record is

`objectives.zrd` (offset 14630, length 19884 of the 11-member archive,
archive sha256 `1ba5b756…4145f`) is the only member declaring numbered blocks:
35 blocks, 254 directive sites, 39 distinct keys. Every key has a measured
disposition (37 measured, `INSTANTWIN` and `INSTANTLOSS` terminal); none is
refused. After M14-B-FU1 all 254 sites bind, all 35 conditions lower, the
program validates and the row is complete. `campaign_ready()` stays false.

## The one compatibility gap

`OBJECTIVE25`'s `TRAVELERS` spells its radius as the int `1000`. Closed by
M14-B-FU1 (production change in `cs_script`); pinned here by
`accept_m14_b_an_int_travelers_radius_lowers_and_a_text_radius_is_refused`
and the retail record test.

## The sheet's three priorities, located in the record

* **Exposed weapon bays:** `ANIM_STATE` blocks 11/12/13 watch the six
  `deploy_gmzep_lbroad*` animations of `geminizep` for `INVALID`, completing
  on 1/3/5; block 14 waits for `hooked_to_klondike` `EXECUTED`.
* **Capital survival:** `INACTIVE*` health blocks over `geminizep` engines
  (7-10, 14 parts, counts 1/4/7/14) and `piratezep` engines (15-18, 12 parts,
  counts 2/5/10/12) and gas bags (19-21, 6 parts, counts 1/2/3); `DEDG` in
  blocks 2, 4, 27, 29, 30, 31; `TRAVELERS` in 25; loss block 22.
* **Chapter transition:** win block 32 (`INSTANTWIN`) is armed only by block 14
  through a 15 s nap; block 14 is woken only by block 33. Loss block 22 is armed
  only by block 21 through a 15 s nap.

## Unknowns (not guessed)

* The record names no successor mission; the chapter transition after
  `INSTANTWIN` is not in this program. Resolving task: M14-C.
* Runtime behaviour (animation names resolving in the world, zeppelin part
  health, timing) is not exercised here; playthrough evidence is M14-C.
* No `TICK_DEPENDS_ON_OBJ` gate is spelled; ordering rests on wake/kill/nap
  edges alone.
