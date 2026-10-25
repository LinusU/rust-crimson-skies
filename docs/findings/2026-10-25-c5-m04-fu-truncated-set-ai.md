# C5 M04: the truncated key `SET_AI_` is ignored

Status: static (`$CS_ENGINE_IMAGE`) plus the installation's own record. Nothing
here is `verified_original`.

## Observation

`zbd/c5/m04/zrdr.zbd`, member `objectives.zrd`, block `OBJECTIVE19` (the
census's "objective#18", zero-based) decodes as

```
BEGIN_DORMANT [-1.0]  SET_AI_ [[devastator_1 M4Defend] [devastator_2 M4Defend]
                               [wingman_1 M4Defend] [wingman_2 M4Defend]
                               [wingman_3 M4Defend]]
```

The key is spelled `SET_AI_` with no suffix, followed by a list of
`[actor, node]` pairs: the shape `SET_AI_NET` takes in `zbd/c4/m01`. The
decoder reads the text as stored; nothing was truncated by the reader. The
authored key is simply incomplete.

## Measurement

* The engine image holds exactly three `SET_AI_*` key strings, in the parser's
  key table: `SET_AI_TEAM`, `SET_AI_ATTACK_RADIUS`, `SET_AI_NET`.
* The record lookup `0x57a090` -> `0x579ff0` compares a child text with the
  asked-for key byte by byte up to and including the terminating NUL
  (`mov dl,[eax]; cmp dl,[esi]; ... test cl,cl` at `0x57a039`-`0x57a057`). It
  is a whole-string compare, not a prefix match, so `SET_AI_` equals none of
  the three keys and is never visited.
* So the original does not apply this block's `M4Defend` assignment to the five
  actors. The site is read by the document walk and has no effect, the same
  measured absence as the stray phrase in `zbd/c4/m01`
  (`2026-10-25-c4-m01-fu-stray-phrase.md`).

## Change

`cs_content::mission_control::ignored_directive` lists `SET_AI_`; the lowering
adapter drops the site together with its list (one ignored site) and the row's
other calls bind as spelled.

## Unknowns

None for this site. We do not substitute `SET_AI_NET`: that would invent
behaviour the original does not have.
