# C4 M01: the unquoted phrase `Change to mobile net` is ignored

Status: static (`$CS_ENGINE_IMAGE`) plus the installation's own record. Nothing
here is `verified_original`.

## Observation

`zbd/c4/m01/zrdr.zbd`, member `objectives.zrd`, block `OBJECTIVE24` (the
census's "objective#23", zero-based) decodes as

```
BEGIN_DORMANT [-1.0]  Change  to  mobile  net  SET_AI_NET [[...]...]  NAP_OBJECTIVE_WHEN_I_COMPLETE [...]
```

The authoring note `Change to mobile net` was written unquoted, so the
original's tokenizer stored four separate text children (`Change`, `to`,
`mobile`, `net`) with no list after them. The decoder reads the same four
nodes; it did not split anything wrongly. The adapter was treating each as a
bare directive and refusing it as an unknown host call.

## Measurement

* The objective record lookup `0x57a090` -> `0x579ff0` scans a record's
  children **element by element** (`inc ebp` at `0x57a070`, no key/value
  pairing) and compares text children with an inline case-sensitive `strcmp`
  against the one key asked for. Extra text children cannot shift the pairing
  of the keys that are asked for, and an unasked key is never visited.
* The objective parser (`0x466b70`..`0x46b200`) holds no immediate that
  addresses a string equal to `Change`, `to`, `mobile` or `net`. `Change` and
  `mobile` appear nowhere in the image as C strings; the single standalone
  `net` string (`0x62b75c`) is referenced only from `0x4bdbff`, outside the
  parser; `to` has no reference from the parser either.
* So the four words are read by the document walk and have no effect, the
  same measured absence as `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE`.

## Change

`cs_content::mission_control::ignored_directive` now lists the four words, so
the lowering adapter drops those sites (counted in `ignored_sites`, 4 of them)
and `SET_AI_NET` / `NAP_OBJECTIVE_WHEN_I_COMPLETE` bind as spelled.

## Unknowns

None for this site. `zbd/c5/m04` (`SET_AI_`) is a different cause and stays
incomplete.
