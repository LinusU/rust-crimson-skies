# M01 world: gating of the multiplayer-named roots and the per-frame LOD pick (#1815)

Task #1815, follow-up of `2026-10-21-m01-missing-terrain.md` (#1735). Capability
used: `retail` (static reading of `$CS_ENGINE_IMAGE`; image sha256 as in
`data/owner-static/engine-facts.toml`). Provenance of every row below is `static`;
no original run happened and nothing is `verified_original`. No disassembly is
committed: addresses are virtual addresses in the image.

## What the image says about the six roots

`multiplayer1zep`, `multiplayer2zep`, `ctf_1`, `ctf_2`, `cs_flag_1`, `cs_flag_2`
(world record slots 931, 1455, 3090, 3092, 3091, 3096 in `ZBD/C3/gamez.zbd`).

1. **No name-keyed exclusion exists in the image.** The strings `multiplayer1zep`
   and `multiplayer2zep` do not occur in it at all; `ctf_1`, `ctf_2`, `cs_flag_1`,
   `cs_flag_2` occur only as the formats `ctf_%d` (0x629100) and `cs_flag_%d`
   (0x62909c, 0x629108). A loader cannot skip a node by a name it does not hold, so
   whatever keeps these roots out of single player is not a name filter in the
   engine; the roots are ordinary world nodes the generic world load builds.
2. **The `ctf_%d` / `cs_flag_%d` names are only looked up, after load, by
   multiplayer game-mode setup.** Both formats are referenced from one routine
   (0x495d40; references at 0x495d7f, 0x496259, 0x49643d). It is called from the
   mission-brief routine at 0x495727 only when the global at 0x71c194 equals 3; when
   it equals 4 the sibling routine 0x496490 runs instead (0x495733). Inside, the name
   is looked up in the node table (0x46e0f0 on 0x71c138); when the node is absent the
   routine skips to its end (0x495dba, `je 0x49644e`). For a found node it attaches
   flag logic (it builds the `turnon_flite%d`, `turnoff_flite%d` and `flg_on%d`
   animation names). So the setup *consumes* the nodes; it does not create or remove
   them.
3. **Single player never takes that path.** 0x71c194 has exactly three writers, all
   in one method (0x413918, 0x413930, 0x41393a inside 0x4136e0, a virtual method
   present in three vtables at 0x60338c, 0x6033bc, 0x6033cc) that derives it from the
   lobby game-type setting at 0x642f90: 0 gives 3, 1 gives 1 or 2 (by the mission
   record's field at +0x28 being positive), 2 gives 4. Every reader (0x4959bd,
   0x495a08, 0x495a75, 0x495a9a, 0x496a6e, 0x496cab, 0x496db3, 0x498fa1, 0x4995c4,
   0x49b930, 0x49b988, 0x4a1edb) compares it against 2, 3 or 4 for flag, rearm or
   zeppelin-mode game logic (`rearm_node_%d`, `zep_rearm_node_%d`).
4. **Zeppelin identity is data, not code.** The image holds `zeppelin_type`,
   `_zeppelin`, `piratezep`, `cargozep1`, `vostokzep` but no `multiplayer*zep`; which
   zeppelin a node is comes from `zeppelins.zrd` (see
   `2026-10-08-t574-zeppelins-zrd-placed-zeppelin-carrier.md`), so
   `multiplayer1zep`/`multiplayer2zep` are not distinguished by the engine.

**Result.** The image gates the *logic* of these roots on multiplayer game modes 3/4
and carries no rule that removes the *nodes* in single player. The measured
answer to "does single-player M01 instantiate them" is therefore: the engine has no
code that would stop the generic load from building them. That is an absence
result from static reading, not an observation of a running original.

## Still unknown (rule 4)

* Whether the nodes are *visible* in single player: `flg_on%d` / `turnon_flite%d` /
  `turnoff_flite%d` suggest a flag node's visible state is animation-driven, and the
  initial state of those animations is not recovered. Affected content: `cs_flag_1`,
  `cs_flag_2` (and possibly `ctf_1`, `ctf_2`). Only an original run (owner
  `human_review`) can settle it. The scenery therefore keeps these six roots undrawn;
  that choice is unchanged by this finding.
* Whether the zeppelin hulls `multiplayer1zep` and `multiplayer2zep` carry a
  data-side disable in `ZBD/C3/gamez.zbd` or the mission scripts that this reading
  did not look at.

## Per-frame LOD pick

Measured only as far as existence: the camera object stores a `lod_multiplier`
(+0xd0) and an `inverse_lod_multiplier_squared` (+0xd4) (its debug dump at
0x5bcf1f/0x5bcf35); scripts set them with `CAMERA_STATE LOD_MULTIPLIER` and
`CAMERA_FROM_TO LOD_MULTIPLIER_FROM_TO`; the host exposes `CameraSetLODMultiplier`
(0x5b8688) and `CameraSetDynamicLOD` (0x5b8547); LOD nodes are built by `LODSetRange`
/ `LODAddChild`; options `ObjectLOD_SW` / `ObjectLOD_HW` exist. The inverse-*squared*
name shows the comparison is made on squared distance scaled by the camera multiplier,
which the designed single band at the player's distance does not model. **Not
recovered:** the function that walks LOD nodes per frame, its range inclusivity, the
effect of `CameraSetDynamicLOD`, and the multiplier's value in M01. Affected content:
every LOD group in the scenery, concretely gun turrets behind a 0-500 m band (for
example slot 1969). Resolving task: filed as a follow-up (static read of the LOD
node traversal that consumes camera +0xd4).
