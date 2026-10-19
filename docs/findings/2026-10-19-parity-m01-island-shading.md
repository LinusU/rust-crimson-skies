# PARITY-M01-ISLAND-SHADING: the island tiles' stored corner colours (#1706)

Task #1706. The comparison that opened it (#1697,
`docs/findings/2026-10-19-m01-smoke-first-control-frame.md`): #1697's smoke
took `frame_02410_first_control.png` (tick 4818, position (-1426, 250, -1813)
m, yaw 40 deg) beside the owner capture `private/ref-drydock/m01/orig25.png`
(about +25 s of the original flight — **not** the same elapsed-time point).
Ours showed low, dark, jagged flat-shaded islands; the original shows large
hazy green islands under a blue sky. Captures never committed; #1697's
frame lives under its own evidence directory and is not in this checkout —
the pre-fix look is reproduced below by this task's own baseline capture.

## Measured — `ZBD/C3/gamez.zbd` terrain tiles carry a complete shading channel

The island tiles are world-space meshes whose corners store a colour triple
on the engine's byte scale, effectively 100 % coverage:

| Stored mesh | Node | Bounds (m) | Corners | Coloured | UV'd | Normal | Textures |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 762 `g28982` | 2882 | near the start pose | 202 | 202 (100 %) | all | ~49 % | terrain set |
| 781 `g28614` | 2976 | (-6144, 0, -4096)..(-5120, 342, -3072) | 197 | 197 (100 %) | all | ~75 % | `forest1`, `cliff1`, `sand128` |
| 770 `polys` | — | island tile | — | all | all | partial | `forest1`, `cliff1`, `rocktop128` |

Corner colour means sit between roughly (120..255) per channel and greenish
(e.g. mesh 770's mean ≈ (170, 160, 135)); the per-corner spread is the
authored shading — grass tops brighter, cliff bases darker.

**Stored normals are partial** (49–75 % of corners). The colour channel is
the one complete per-corner field: it is the surface's authored shading
input, not a decoration.

## Measured — the original engine's stored-colour rule

`0x4bc350`, `data/owner-static/engine-facts.toml` `colour_byte_scale`
(static, never `verified_original`): a stored colour channel above `1.0` is
scaled `1/255`; at or below `1.0` it is already a unit channel. The same
rule `render::horizon`'s `zone_colour` applies to the `weather.zrd` sky
records' colours.

## The defect — two losses on the production path

1. `playtest_textures::slice_groups` rebuilt each material-group part with
   positions, normals, UVs and indices but **dropped
   `Mesh::ATTRIBUTE_COLOR`**: the complete stored shading channel never
   reached any drawn part (sky records lost it the same way). `world/meshes`
   merges it into the upload mesh; the slice threw it away.
2. With the colours restored the parts still drew **lit**: the scene's
   designed directional light shaded the texture alone while the authored
   shading sat multiplied into `base_color` — and on the parts whose stored
   normals are absent (the ~25–50 % normal-free groups, split into their
   own parts) Bevy fills flat normals, which is the jagged facet look the
   #1697 frame shows.

## The bounded fix

- `slice_groups` copies each group's slice of the stored colour buffer into
  its part (`crates/cs_app/src/playtest_textures.rs`).
- `normalize_corner_colours` applies the measured `>1.0 → ×1/255` rule to
  the RGB of every drawn part's colour attribute (sliced and whole-mesh
  paths alike); alpha rides unchanged, and the upload-side representation
  keeps the stored bytes verbatim for audit.
- `stored_shading_materials`
  (`crates/cs_app/src/mission_session/world_draw.rs`) gives a
  vertex-coloured world part an **unlit** copy of its bound material —
  `texture × stored colour`, the authored shading channel doing the
  shading — while `fog_enabled` and every other binding stay. An
  uncoloured part keeps the shared lit material; an already-unlit part
  (horizon subtrees) is untouched.

Vertex colour in this Bevy version multiplies `base_color` and the sampled
texture, so the byte-rule normalization is what keeps the multiplication a
colour rather than a 200× blow-out.

## The frames — before, after, original; words only, no pixel-parity claim

- **Before** (baseline binary without the fix):
  `private/evidence/PARITY-M01-ISLAND-SHADING-baseline/baseline/frame_02451_world.png`.
  The islands are near-black jagged flat-shaded masses; one green hill shows
  behind the right-hand island and a flat white patch sits where a beach
  texture belongs — the stored shading channel never reached the drawn
  parts, so the designed sun lit whatever normals survived.
- **After** (this commit's smoke, 21 checks, 0 failed):
  `private/evidence/PARITY-M01-ISLAND-SHADING/frame_02410_first_control.png` —
  the same tick 4818 / script frame 2410 as #1697's capture. The islands now
  draw green and textured, the stored forest/cliff/sand detail and the
  authored per-corner shading visible; the black facets are gone.
- **Original** `orig25.png`: large hazy green islands across the horizon,
  blue sky with clouds, dark smooth sea, the "[Examine] - Site #1" marker
  over the middle island.

Remaining differences, in words only: our sky is still a paler grey and our
sea a flatter teal than the original's blue sky and smooth dark sea (the
atmo/sea bindings own those); the original's islands sit deeper in
atmospheric haze and read larger — how much of that is fog density, FOV or
the ~25 s of extra flight is unmeasured; our contacts list names the
zeppelin and Kestrels the original frame does not show. Heading agrees as
before (`NW` in both compasses) and our frame also carries the Site #1
marker, off to the right at "5 o'clock" rather than over the middle island —
an elapsed-time/position difference, not a binding claim.

## Unknowns — recorded, not guessed

- **`MaterialUnknown::VertexColorMeaning`**: how the original *applied* the
  corner colours (modulate, replace, or ignore under its own lighting
  model) is unmeasured. On mesh 781 the stored colour brightness does not
  correlate with the stored normals or with a sun-direction `N·L` model
  (R² ≈ 0.001–0.03) — the channel is authored shading/AO, not a simple
  directional bake. The `unlit` draw is this build's declared presentation
  (`PLAYTEST_TEXTURE_PRESENTATION_IS_PROVISIONAL`), chosen because the
  colours are the one complete shading field, not because the original is
  known to skip lighting.
- **Lit-vs-unlit in the original**: the GameZ material records carry no
  lighting flags; nothing in the stored data settles it.
- **Sun-orientation axis semantics** stay unknown (unchanged by this task).
- **Scale/haze/elapsed-time**: the original's larger-looking islands sit in
  denser atmospheric haze at a later flight point; how much of the size
  difference is fog, FOV, or flight position is unmeasured.
- Sky and sea differences from the #1697 comparison (pale grey sky, flat
  teal sea) are separate bindings — atmo tasks own them; this task changed
  no horizon, fog, sunlight or sea code.

Nothing here is `verified_original`.
