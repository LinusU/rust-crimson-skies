# PLAYTEST-AREA-FLICKER (#753): the airship drew every variant of every part

Measured with `accept_playtest_area_flicker_*` (retail, GPU). Frames are under
`private/evidence/PLAYTEST-AREA-FLICKER/` and are not committed.

## Cause 1 confirmed: alternatives drawn on top of each other

The `piratezep` subtree (node 517, 793 nodes, 401 mesh bindings) holds:

* sibling `Lod` bands (`l2` 0-500 m / `l3` 400-1500 m, `l28`/`l29`, `healthy`, `lod`, ...);
* `burnpanels` beside `panels` in each of the six `gasbag` nodes (ten scorched
  `burnp…` meshes on the hull plane of the ten light `panel…` meshes);
* `panelleftb1`/`panelrightb1` with `…h` and `…d` children;
* `propstill` beside `spin` and `counterspin`.

The selection now draws one of each; 401 stored bindings split into drawn plus
hidden with a reason each, and the collider count equals the drawn count.

## Pixel instability (flipped share of the frame, worst of 3 later frames, 1 cm camera steps)

| view | all bindings (main) | selected |
| --- | --- | --- |
| flank-far-aft (120 m, z -200) | 0.0394 | 0.0007 |
| flank-near-aft (20 m, 0.2 up, z -200) | 0.0865 | 0.0016 |
| flank-far-mid (120 m, z 0) | 0.0108 | 0.0004 |

Threshold 0.002. Main's behaviour fails it on all three views; the selection
passes on all three.

## Cause 2 (coplanar decals): residual, not fixed

Some views fight identically before and after: for example the grid view `t4d20z0`
(20 m off the `-x` face, 0.4 of the height up, mid-hull) flips 0.081 of its pixels
both ways, where the hull's skull decal visibly changes between frames 1 cm
apart. I could not attribute it to a stored decal binding (the bindings whose
bounds cross the ray are `structureleft`, `panelleft1`, `battlement5`, none of
them the hull skin), so no depth bias is claimed. Follow-up: identify the
skull's layers and, if they are coplanar, apply a labelled designed bias
(`StandardMaterial::depth_bias`, which Bevy 0.19 passes to wgpu as the
pipeline's constant depth bias).
