# #1648 PARITY-M01-ATMO-A: weather.zrd units and the sky, water and cloud consumers for M01 "The Lost Treasure"

Date: 2026-10-18. Research only. Archive: **`ZBD/C3/M01/zrdr.zbd`** with
**`ZBD/C3/gamez.zbd`** (M01 is `ZBD/C3/M01`, not `C1C`; see
`docs/findings/2026-10-17-m01-archive-decision.md`). Inputs: `$CS_ENGINE_IMAGE`
(read-only, never committed) and `$CS_GAME_DIR` (read-only). The original
executable was not run, so every claim is `static` and nothing is
`verified_original`. Implementer: `sonnet-1`; no independent review yet.

Earlier work measured the same engine on the wrong archive
(`2026-10-14-m01-play-sky-horizon.md`, `2026-10-15-m01-play-weather-clouds-haze.md`,
both about `C1C`, now work order M06) and the unit question on its numbers
(`2026-10-16-f19-weather-units-measure.md`, claims 1–9, still valid and
**not repeated** here). This note adds what they left open: M01's own record,
the `FOG_RANGES` / `FOG_ALTITUDE` / `SUNLIGHT_*` consumers, what the cloud band
does at run time, and which engine code touches the sky, sea and cloud art.
Committed facts are the `[[atmo_weather]]` rows of
`data/owner-static/engine-facts.toml`; addresses below are virtual addresses in
the image.

## 1. M01's `weather.zrd` (read through the production reader)

`ZBD/C3/M01/zrdr.zbd` → `weather.zrd`, four zones, no precipitation block:

| key | value in the file |
| --- | --- |
| `VIEWING_RANGE` | `HIGH`/`SWHIGH` 1.0/1.0, `MED`/`SWMED` 0.85/0.85, `LOW`/`SWLOW` 0.7/0.7 (clip scale / fog scale) |
| `WIND` | `STATIC_VELOCITY (0, 2, 0)`, `RANDOM_MAX_SPEED 10`, `RANDOM_ACCEL 5`, `RANDOM_ANG_VEL 5` |
| `CLOUD_COVER` | `TOP 11000`, `BOTTOM 10000`, `THICKNESS 50`, `TOP_COLOR (32, 56, 72)`, `BOTTOM_COLOR (32, 56, 72)` |
| `ZONE1` | `FOG_COLOR (0.79, 0.79, 0.79)`, `FOG_RANGES 1000 4500`, `FOG_ALTITUDE 9000 10000`, `CLIP_RANGES 5 4800`, sun on, `SUNLIGHT_ORIENTATION (-25, 135, 0)`, diffuse 1.5, ambient 0.3, colours white, static 1, bicolored 0 |
| `ZONE2` | `FOG_COLOR (0.06275, 0.09412, 0.1882)` (= bytes 16, 24, 48), `FOG_RANGES 1000 4500`, `FOG_ALTITUDE 9000 10000`, `CLIP_RANGES 5 5000`, same sun block as `ZONE1` |
| `SW_ZONE1` / `SW_ZONE2` | same fog colours and altitude, `FOG_RANGES 600 1700` / `600 2700`, `CLIP_RANGES 5 2000` / `5 3000`, sun **off** (`SUNLIGHT_ACTIVE 0`, orientation `(-90, 0, 0)`, diffuse 0, ambient 1) |
| `SHADOW_ANGLES` | `(-90, 0, 0)` |

Two things differ from the `C1C` numbers the earlier notes used: M01's cloud
band sits at **10 000–11 000 m**, not 1 055–1 110 m, and its two zones carry
**different** fog colours (grey above `ZONE1`, dark blue `ZONE2`), so the bind
of #1460 rightly leaves `ZONEn.FOG_COLOR` unbound for this mission (it binds a
colour only when every hardware zone agrees).

## 2. Units and scales (answers to the task's list)

| field | unit / scale | evidence |
| --- | --- | --- |
| `FOG_COLOR`, `SUNLIGHT_COLOR_DIFFUSE`, `SUNLIGHT_COLOR_AMBIENT`, `TOP_COLOR`, `BOTTOM_COLOR` | one reader, `0x4bc350`: a channel **above 1.0** is multiplied by `1/255` (`colour_byte_scale`), a channel at or below 1.0 is kept. So `FOG_COLOR` is a normalized 0–1 triple, and the cloud colours `(32, 56, 72)` are bytes = **(0.1255, 0.2196, 0.2824)**. This **closes the "integer colour of unmeasured scale" residual** on `CLOUD_COVER.TOP_COLOR/BOTTOM_COLOR` in the bind's unbound list. Defaults are 1.0 for every channel (`0x4bc7df`, `0x4bc82c`). | parser `0x4bc7be`–`0x4bc857`; reader `0x4bc350` |
| `CLOUD_COVER.TOP` / `BOTTOM` / `THICKNESS` | raw floats, compared with camera world-Y; metres (claim 3 of the #1460 note). Defaults 280 / 240 / 0 (`cloud_*_default`). | `0x4bc7d7`–`0x4bc871`; compares `0x42eef1`–`0x42ef22` |
| `FOG_RANGES` (near, far) | distances, read raw into zone `+0`, `+4`; at zone-apply time **both multiplied by the active viewing tier's `fog_scale`** (tier record `{clip_scale, fog_scale}` at `tier + 0`, `+4`; M01's tiers are 1.0 / 0.85 / 0.7). Unit: the same camera-space distance the clip planes use, i.e. world metres (the world unit is the measured metre, `engine-facts` `0x48fc40`); not separately proven by a draw. | zone apply `0x472ea0`: `fld [edi+4]` × `[esi+4]` and × `[esi]` → setter `0x4da870` (fog object `+0x20`, `+0x24`) |
| `FOG_ALTITUDE` (first, second) | raw floats, no scale applied (tier scale is **not** applied). **Spelling order is swapped in memory**: the first spelled number lands in zone `+0xc`, the second in zone `+8`; `0x472ea0` then calls `0x4da850(fog, zone+0xc, zone+8)` and the setter stores them at fog object `+0x28` (= second spelled) and `+0x2c` (= first spelled). For M01: first 9000, second 10000. Unit: world Y metres by the same reasoning as `CLOUD_COVER` (the band 9000–10000 ends exactly at `BOTTOM`). **Not measured:** the renderer-side use of fog `+0x28`/`+0x2c` (see §6). | parser `0x4bc490`–`0x4bc4c0`; apply `0x472f0e`; setter `0x4da850` |
| `CLIP_RANGES` (near, far) | raw floats; far is multiplied by the tier's `clip_scale`, near is not; a near value below `6.0` is replaced by `6.0` (`clip_near_floor`). Passed to the camera setter `0x4d2930` (also to the node at `0x71d284`). | `0x472f3d`–`0x472f9f` |
| `SUNLIGHT_ORIENTATION` | **degrees in the file, radians in memory**: three angles in file order, each × `degrees_to_radians`, into zone `+0x28`, `+0x2c`, `+0x30`; default first angle `-π/2`. Handed in file order to the light-node setter `0x4dc610(node, a0, a1, a2)`. Axis meaning of the three angles: **unknown** (the setter body is not read). | `0x4bc53a`–`0x4bc5a9`; apply `0x472fcd`–`0x472fe0` |
| `SUNLIGHT_DIFFUSE`, `SUNLIGHT_AMBIENT` | raw floats (not 0–255), zone `+0x34`, `+0x38`; passed to `0x4dbdb0` / `0x4dbce0`. | `0x4bc5ac`–`0x4bc5f6`; `0x472fe5`–`0x472ffc` |
| `SUNLIGHT_ACTIVE`, `SUNLIGHT_STATIC`, `SUNLIGHT_BICOLORED` | booleans (non-zero → 1 byte) at zone `+0x24`, `+0x54`, `+0x55`. `ACTIVE` goes to `0x4dbcc0(node, byte)`. | `0x4bc51a`, `0x4bc635`, `0x4bc655`; apply `0x472fc3` |
| `SHADOW_ANGLES` | degrees → radians, as above; nothing binds it. | `0x4bc878`–`0x4bc8f6` |
| `VIEWING_RANGE` tiers | `clip_scale`, `fog_scale` unitless multipliers on `CLIP_RANGES` far and `FOG_RANGES`; `SW*` tiers apply to the `SW_ZONEn` array. A global at `0x9be708` picks the software-render zone array (`0x71dd24`) over the hardware one (`0x71de2c`) and the matching tier array (`0x71df34` vs `0x71df4c`, index `0x71df6c`); zone record stride is `0x58` bytes. | `0x472eb8`–`0x472ef0` |

## 3. What `CLOUD_COVER` does at run time

The setup at `0x473636` (the mission world setup, 8 670 bytes) derives, from the
parsed record at `0x71dcd0`:

- `mid = BOTTOM + (TOP − BOTTOM) / 2` → `0x71c2cc`. M01: **10 500**.
- `lo = mid − THICKNESS/2` → `0x71c2d0` (10 475), `hi = mid + THICKNESS/2` →
  `0x71c2d4` (10 525). `THICKNESS` is therefore the **full width of the
  plateau**, centred on the midpoint (the #1460 note's "half-width" is the
  half each side).
- It feeds the five numbers to a trapezoid profile (setters `0x54e4b0`,
  `0x54e4f0`, `0x54e530`, `0x54e570`, intensity cap `0x54e5b0`) used by the
  per-vertex evaluator `0x5782d0` (two sibling evaluators near `0x5786d0`
  and `0x578b70` read the same globals; the callers of `0x5782d0` are
  `0x551e8b` and `0x552116`): intensity is **0 at and outside `BOTTOM`/`TOP`**, rises
  linearly `BOTTOM → lo`, holds the cap on `lo … hi`, falls linearly
  `hi → TOP`. The altitude is a dot product of the vertex with a view-matrix
  row plus an offset, i.e. world height.
- It registers `0x473580` as the **zone selector** (`0x54e6d0`): for an
  object position it returns **1 when its Y is below `mid`, otherwise 2**
  (255 when the zone state at `0x64ef6c` is 3).

The per-frame camera function `0x42ee60` does the same band for the viewer:
when the camera is inside `[BOTTOM, TOP]` (and `0x654120 == 0`) it blends
`BOTTOM_COLOR → TOP_COLOR` linearly over `(y − BOTTOM)/(TOP − BOTTOM)` into a
colour and computes the same trapezoid as an intensity (`(y−BOTTOM)/(lo−BOTTOM)`
below, 1 on the plateau, `(TOP−y)/(TOP−hi)` above), then folds in the fog
volume (`fogvol.zrd`) when one is active. A second consumer (`0x42b7c0`)
tests camera Y against `BOTTOM` and `BOTTOM + 0.8·(TOP−BOTTOM)` (`cloud_inside_fraction`).
It also toggles the world node named `zone2_cloud_floor` (`0x4cca30`,
argument 1 when `0x654120 == 0`). **`ZBD/C3/gamez.zbd` has no node of that
name**, so for M01 the lookup returns null and the engine skips the toggle.

So for M01 the cloud band is a **colour-and-intensity volume at 10–11 km**
that tints a camera or vertex inside it. It is **not** a drawn sheet, and
nothing in this code draws a cloud texture.

## 4. Zones are an altitude split of what gets drawn

`ZONE1` and `ZONE2` are not only fog blocks. The camera's zone (1 below `mid`,
2 above) is computed from the camera position (`0x42f249`–`0x42f262`) and
applied through `0x472ea0` when it changes (zone array entry, fog colour via
`0x4da820`, ranges via `0x4da870`, altitude via `0x4da850`, clip via `0x4d2930`,
sun via `0x4dbf70`…`0x4dcb10`). The renderer's per-node test `0x56c430` then
passes a node when the feature is off, its stored **zone id is 255**, the
allowed-zone list (count at `0xa070a8`, ids from `0xa070a9`) is empty or holds
255, or it holds the node's id. The GameZ archive stores that id per node
(`zone_id` of the 208-byte record). **Not traced:** who fills the allowed-zone
list from the camera zone; the rule above is what the test accepts, not a
proved pairing with the camera's zone.

For M01 that means: the dark-blue `ZONE2` fog is what the viewer sees **above
10 500 m**, and the grey `ZONE1` fog is what it sees below. A mission whose
aircraft stays under about 2 km sees only `ZONE1`'s colour, range and sun.

## 5. The sky, sun, clouds and sea in `ZBD/C3/gamez.zbd`

Measured with the production GameZ readers (`read_gamez_nodes`,
`read_gamez_materials`, `read_gamez_meshes`) on the owner's archive; no art is
committed.

**Texture table** (465 names, 483 materials). Present: `Sky1.tif` (270),
`cloud1.tif` (266), `cloud2.tif` (267), `cumulus1.tif`, `cumulus2.tif`,
`Sun.tif` (265), `skywal01.tif` (254), `SPACE.tif` (76), `wtr00000.tif` …
`wtr00015.tif` (216, 447–461). **Absent: `water1` and `cloudlayer`** (these
two names exist only in `ZBD/C1C/gamez.zbd`, so they are not M01 art, and the
parity tasks must not look for them).

**The `horizon` node** (slot 3234, tag 5 object, `zone_id` 255, flags
`0x3080584`, no mesh, parent `world1`) has two children, both empty containers:

| node | zone id | children |
| --- | --- | --- |
| `zone2` (3235) | 2 | none |
| `zone1` (3236) | 1 | `sun` (3237), `g1155` (3238), `h_zone2scroll` (3239) |

So above the band (`ZONE2`) the horizon draws **nothing**; the whole sky is
`ZONE1`-tagged geometry. The geometry (all node translations identity unless
noted):

| node | mesh | what it holds |
| --- | --- | --- |
| `h_zone2scroll` | 493: 48 verts, 72 polys | all `Sky1.tif`; Y 499.8 … 1646.4 m, radius in XZ 4372 … 8744 m; per-corner colours 167–242 (0–255 scale); a banded dome wall. The word "scroll" matches the engine strings `Scroll` / `ScrollAlways` (`0x63e8a7`…), but the scroll consumer for this node is **not traced**. |
| `g1155` | 492: 74 verts, 25 polys | 14 untextured polys (Y −5544 … 500, radius 3645 … 8744: the skirt under the dome), 1 `Sky1.tif` cap at Y 1646.4, 8 polys `cumulus1/2.tif` at Y 678 … 1901 on the 4.4–4.9 km ring (the sprite-like cloud bank on the horizon), and two quads `cloud1.tif`, `cloud2.tif` of 22 m at Y −245 near the origin. Corner colours 176–255. |
| `sun` | 491: 4 verts, 1 poly | `Sun.tif` quad about 170 × 170 m, node translation `(1462.7, 1420.7, −1471.4)` (about 2.5 km from the horizon origin, elevation about 34° if the node is at the origin). |

Other facts: the `sunlight` node (slot 5302) is a **tag-9 (light) node** at the
archive root, found by name class 10 (`sunlight`), which is where
`SUNLIGHT_*` goes; M01 has **no** `cloudparent` / `cloudsprite*` / sprite-cloud
nodes (the `C1C` archive has ~40 `cloudparent` and two `cloudsprite` slots), so
the sprite cumulus art of the earlier notes does **not exist in M01's world**.

**Sea.** There is no `water1` and no single sea plane. 347 meshes of the
archive use `wtr00000.tif`: **251 are direct children of `world1`** (the
terrain tiles, `zone_id` 1, flags `0x308831c`, 1024 m cells) covering
x, z ∈ [−16 384, 0] at Y 0 … 13.2 m, **96 sit in the groups `swtr01` … `swtr05`**
(16, 16, 12, 28 and 21 meshes; their role is unmeasured), and the rest are two
`craterlake` meshes (`waterlevel`, `waterbottom`) and one more. Material 220
(`wtr00000.tif`, alpha 255, flags `0x15`) carries a **16-frame cycle**
`wtr00000 … wtr00015` with raw rate word `10.0` (`unk12`; the cycle unit is
unmeasured).

## 6. What the image does *not* name, and what stays unknown

- The image holds **no texture names** (`Sky1`, `cloud1`, `cloud2`, `water1`,
  `cloudlayer`, `Sun`), no `horizon` node name and no `zone1`/`zone2` node
  names: the sky is data-driven GameZ geometry reached through ordinary node
  rendering and the zone id, not a hard-wired dome.
- **`pfhorizon` is not the sky.** It is one of the player vehicle's part names
  beside `compass`, `rtracks`, `ltracks` (`0x627cb0`…, referenced once at
  `0x476bd3`). `CameraSetHorizon` / `CameraSetHorizonXZ` (`0x63f520`,
  `0x63f534`, registered at `0x5b863e`) are script commands whose texts are
  `horizon = %s` / `horizonXZ = %s`; their body is not read.
- **`ZBT_CAMDYN_CLOUDHACK` is a label only.** It is a name field of a
  0x114-stride record table next to `cam_dyn_render` and `ZBT_ZVID_*`; nothing
  reads it as a cloud mechanism (`camdyn_cloudhack_label`).
- **Unknown, with the affected content:** (a) how the renderer uses fog object
  `+0x28`/`+0x2c` (the `FOG_ALTITUDE` pair): the 9000–10000 m band is above
  every M01 flight altitude, so this is a high-altitude effect in M01, but the
  curve is unmeasured; (b) how the fog near/far become a per-pixel or
  per-vertex factor (linear is the only form the fixed D3D fog offers, but no
  draw call was read); (c) whether the colour triple is packed as display or
  linear colour (carried over from the #1460 note, unchanged); (d) the
  position rule of the `horizon` node (camera-following or fixed), the
  `Sky1` scroll and the sun's relation to `SUNLIGHT_ORIENTATION` (the three
  angles' axes); (e) the water-cycle rate unit; (f) the `zone2_cloud_floor`
  node's art (absent in M01); (g) the allowed-zone list fill (§4).

## 7. Consequences for the dependent tasks

- **#1649 ATMO-B (fog, gradient, haze).** M01 has two zone fogs. Below 10 500 m
  (all of M01's flight) use `ZONE1`: colour `(0.79, 0.79, 0.79)`, near 1000,
  far 4500 m, times the tier `fog_scale`; clip far 4800 m times `clip_scale`;
  near clip 5. Do not apply `FOG_ALTITUDE` below 9000 m (its renderer use is
  unmeasured). The `ZONE2` dark blue is the fog above the band; reaching it
  needs a 10.5 km climb.
- **#1650 ATMO-C (sea).** Draw the 251 `wtr00000` terrain tiles under `world1` (and the 96 `swtr0n`
  meshes once their role is measured) at their own
  geometry with the 16-frame `wtr0xxxx` cycle; there is no horizon-fade
  mesh in the archive other than the untextured skirt of `g1155`, and no
  `water1`.
- **#1651 ATMO-D (clouds).** There are no sprite clouds in M01's archive.
  What exists is the band at 10–11 km (colour `(32,56,72)/255` volume, §3)
  and the `cumulus1/2` ring plus `cloud1/2` quads in `g1155`. A designed
  sprite cloud field is **not** original for M01.
- **#1643 ATMOSPHERE.** The sky is `h_zone2scroll` + `g1155` (+ `sun`), all
  `ZONE1`, vertex-coloured; draw them from the archive and keep them
  `ZONE1`-gated.
- Residuals for the bind (`bind_weather`): `CLOUD_COVER.TOP_COLOR/BOTTOM_COLOR`
  may now be bound as byte-scaled colours; `THICKNESS` is the plateau full
  width; the unit of `FOG_RANGES` is metres and the tier multipliers apply to
  it.

## 8. Method and verification

- Disassembly with `radare2` 6.2 over the decrypted image (kept in
  `private/`, never committed); GameZ reads with throwaway examples over the
  production readers.
- `accept_parity_m01_atmo_a_rows_parse_and_agree_with_the_finding` parses the
  committed `[[atmo_weather]]` rows under the production loader and checks the
  numbers this note quotes; `accept_parity_m01_atmo_a_rows_match_the_image`
  (needs `CS_ENGINE_IMAGE`) re-reads every row from the image and the
  instruction bytes the claims rest on; `accept_parity_m01_atmo_a_retail_m01_weather_and_sky_graph_match`
  (needs `CS_GAME_DIR`) reads M01's `weather.zrd` through the production
  reader and the `horizon` subtree of `ZBD/C3/gamez.zbd` through the GameZ
  readers and asserts the tables in §1 and §5.
