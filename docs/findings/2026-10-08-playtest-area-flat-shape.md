# PLAYTEST-AREA-FLAT-SHAPE (#795): the flat grey shape is the landing card `sphere`

Owner report (human_play, owner, 2026-10-08, main `1c50a74e`, `cs --playtest
--cs-path "$CS_GAME_DIR" --world c1c`): "there is something 2-dimensional
sticking out from the back of the airship" — in the screenshot a large flat
grey triangle, untextured-looking, roughly a third of the hull long, off the
underside of one end of the `piratezep` hull.

Measured with the production readers over every one of the area's 401 stored
mesh bindings (`accept_playtest_area_flat_shape_*`, retail + GPU; frames under
`private/evidence/PLAYTEST-AREA-FLAT-SHAPE/`, not committed).

## The census: what the 295 drawn bindings are

| class | count | detail |
| --- | --- | --- |
| normal meshes (word0 = 0) | 265 | hull skin, panels, structure |
| billboard meshes (word0 = 1) | 30 | all small textured cards (`g161`…`g186`, ≤ 10.8 × 2.4 stored units) |
| **flat-colour cards** | **2** | `sphere`, `half_cone` — the shape in the screenshot |

The billboard hypothesis from the #396 owner note (`OWNER-STATIC-2026-10-05`:
mesh word0 = 1 is a camera-facing billboard the D3D path draws unculled) does
**not** explain the shape: the area's 30 drawn billboards are all ≤ 10.8 units
long and textured, nothing like the owner's a-third-of-the-hull grey triangle.
They are drawn fixed here (their camera-facing mode is still unimplemented),
but that is a separate, small artefact, not this task's shape.

## The two bindings, by stored attributes

Exactly two of the 295 drawn bindings carry flat-colour (untextured) material
groups — measured over the whole subtree, no third binding matches:

| binding | node slot | mesh | parent chain | polygon | corners | extent (stored units) | materials |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `sphere` | 3063 | 786 | `piratezep(517) > move_zeppelin(2162) > tilt_zeppelin(2163) > pz_auto_land(3008)` | 1 (planar) | 3: (0,0,0) (0,0,500) (0,−125.25,500) | 0 × 125.3 × 500.0 | 84 |
| `half_cone` | 3066 | 787 | `piratezep(517) > move_zeppelin(2162) > tilt_zeppelin(2163) > pz_manual_land(3009)` | 1 (planar) | 3: (0,0,−32) (0,0,64) (0,−32,64) | 0 × 32.0 × 96.0 | 84 |

Both meshes store word0 = 0 (normal type), one planar triangle each, and every
material group resolves to material **84**, a record whose textured flag is
clear (`MaterialKind::Colored`): it names no texture, so the binder draws it in
the neutral development colour (`WORLD_MATERIAL_COLOR` 0.74/0.70/0.62) — the
uniform mid-grey of the screenshot. Their parent names (`pz_auto_land`,
`pz_manual_land`, under the zeppelin's `move`/`tilt` nodes) say they are landing
corridor cards.

### The owner's question: a neutral material with an empty `unresolved` list

The startup line's `neutral_materials 1, flat_materials 1, unresolved: []` is
consistent, not odd: `TextureBinder::bind` counts **two different** cases as
neutral. A record that *names a texture* which fails to resolve is pushed into
`unresolved` with its reason; a record whose textured flag is clear names no
texture at all, so it is counted as `neutral_materials` **and**
`flat_materials` and drawn neutral **without any lookup having failed** — it is
never pushed into `unresolved`. C1C's one neutral material is exactly that: the
flat-colour material 84, used by these two cards (the container holds eight
flat-colour records — 0, 84, 97, 100, 103, 150, 152, 158 — but only 84 is
referenced by the area's bindings). An empty `unresolved` therefore means "no
texture lookup failed", not "no neutral material exists".

## Treatment: hidden with the reason, provisional

What the original did with these cards is **unmeasured**: the material record
stores no render class this reader decodes, and whether the 2000 engine drew
them additive, translucent, or only while a landing sequence ran is unknown.
The cards are therefore hidden from drawing **and** from collision (colliders
follow the drawn set) by `flat_card_reason` in the area selection, listed with
that reason in `area_undrawn`, and the treatment is labelled provisional in
`docs/PLAYTEST.md` and `docs/PLAYTEST-RETAIL.md`. The decision is keyed on the
stored attributes in the table above — one planar polygon whose every material
group is flat-colour — not on the authored names; the names travel in the
reason as evidence. `PlaytestConfig::hide_flat_colour_cards` (documented
default `true`) turns the rule off, which is the baseline its removal is
measured against.

The drawn set is now **293 bindings / 8 108 triangles** of the 401 stored
bindings (main drew 295 / 8 110), and the collider count still equals the drawn
binding count (`accept_playtest_area_flat_shape_retail_…`).

## GPU before/after capture (hashes only; PNGs stay under `private/`)

View `flat-shape-underside-aft`, derived from the two cards' composed geometry
(eye `[500.0, −138.0, 350.6]` canonical metres, 500 m off the card plane,
looking at the cards' joint extent centre), one 640 × 480 frame of the scene
spawned with `hide_flat_colour_cards = false` (main's behaviour), the two card
entities hidden and re-rendered through the same production render path
(`capture_visibility_delta`; the aircraft is hidden in both frames so the delta
is the cards' own footprint):

| frame | measured |
| --- | --- |
| before (cards shown) | covered pixels 65 362, PNG sha256 `b597f561f37b1a66214f3f81f77487d5166fbe0f9afdd99df9810967b4a15278` (75 434 bytes) |
| after (cards hidden) | covered pixels 22 526, PNG sha256 `49329f4855520eeab9ec15049a96914a72e27312f2519c41a45b72c7aaf9ab42` (31 862 bytes) |
| differing pixels | 43 213 of 307 200 (14.1 % of the frame) |

The two cards cover 42 836 pixels of sky-or-hull that the hidden frame does
not, i.e. the flat shape is gone from that end of the airship once the rule
hides it, and what remains (22 526 covered pixels) is the hull end itself. The
test `accept_playtest_area_flat_shape_gpu_…` asserts the same numbers as
thresholds on the real GPU, so the capture is a measurement, not a claim.

## What stays unknown

* The original render class of the landing cards (additive, translucent,
  opaque) and the colour the original drew them in: the material record's
  `color`/`rgb` words are not established as display colour (F10 non-negotiable
  #5), and no original run has been watched. Resolvable by a capture of the
  original landing sequence (needs the owner), or by static analysis of the
  original executable's material path (task lane #396 / `OWNER-STATIC-2026-10-05`
  style).
* Whether the original gated the cards by landing behaviour (`pz_auto_land` /
  `pz_manual_land` are script-driven zeppelin states): the mission/script
  semantics are out of this task's scope.
* The area's 30 drawn billboard meshes are still drawn in a fixed orientation;
  camera-facing presentation (word1 modes 0/1/2/3 of `OWNER-STATIC-2026-10-05`)
  is unimplemented and is follow-up work, not this task's shape.
