# #1847 M01-FOG-UPDATE-STATIC: who writes and reads the engine's fog state, and what the launch hand-over changes

Date: 2026-10-23. Static analysis only: `$CS_ENGINE_IMAGE` (read-only, never committed) was disassembled with
`llvm-objdump` (Intel syntax) into `private/`; the original executable was not run, so every claim is `static`
and nothing is `verified_original`. Implementer: `claude-2` (Sonnet 5.5); no independent review yet. Addresses are
virtual addresses in the image. Committed facts are the six `[[fog_state]]` rows of
`data/owner-static/engine-facts.toml`; the ignored test `accept_m01_fog_update_static_rows_and_instructions_match_the_image`
re-reads them and the instruction bytes below.

Prior work this builds on: `2026-10-18-parity-m01-atmo-a-weather-units-and-sky-consumers.md` (zone apply `0x472ea0`,
`CLOUD_COVER`, units). Its sentence "the fog object" is more exactly **the state block (`obj+0x38`) of the world
object (node class 14, global `0x71c0b8`)**: the same block also holds the terrain tile grid (`+0x34..+0x9c`,
read by `0x4da3b0`), the camera clip pair (`+0xb0`, `+0xb4`) and the fog fields.

## 0. Answer for the dependent tasks

* **#1744 (haze thins after launch): no curve exists to implement.** Nothing in the image interpolates fog near/far,
  colour, altitude or the `VIEWING_RANGE` scales over time. Fog changes only in discrete steps (section 2):
  the cinematic-mode exit switches the fog mode from 0 to 1 and multiplies the camera far clip by 0.25, the camera
  crossing the cloud-band midpoint re-applies the zone, and a viewing-tier change re-applies the zone. M01's script
  spells none of the per-frame-capable fog commands (`WorldSetFogState` only). Whether the launch hand-over of M01 runs
  the cinematic exit `0x450e30` is **unknown** statically (section 2.3). So #1744 can be implemented only as the
  step the exit performs, and only after the launch is shown to take that path; a fade over seconds would be an
  invented number.
* **#1745 (cloud layer, in-layer fog): the in-layer effect is not fog.** Fog altitude is a height factor on the
  *same* range fog and is fully on below `FOG_ALTITUDE`'s first number (M01: 9000 m); the cloud band `CLOUD_COVER`
  drives a **full-screen colour fill** with an intensity trapezoid, plus a per-vertex intensity for flagged polygons
  and a precipitation controller (section 3). M01 has no cloud sprite art (atmo-a) and no precipitation block, so the
  only in-layer behaviour M01 can show is the screen fill. Its blend equation (`0x5ca0e0`) is **not read**, so the
  fill's visible strength is still unknown; implementing the fill's *inputs* (colour, intensity) needs no invented
  number, drawing it needs that one read.

## 1. Writers and readers of the fog state

State block layout (written by the setters, `eax = [obj+0x38]`; the first dword is a dirty mask):

| field | setter | dirty bit | renderer global (via the flush) |
| --- | --- | --- | --- |
| `+0x10` mode | `0x4da800` | `0x1` | `0xa06f38` (via `0x54e200`), `0xa06f3c` (via `0x54e350`, never read) |
| `+0x14/+0x18/+0x1c` colour | `0x4da820` | `0x2` | `0xa06f40..48` (via `0x54e360`) |
| `+0x20/+0x24` near/far | `0x4da870` | `0x4` | near `0xa06f4c`, near² `0xa06f50`, far `0xa06f54`, far² `0xa06f58`, `1/(far-near)` `0xa06f5c` (via `0x54e230`, `0x54e280`) |
| `+0x28/+0x2c` upper/lower altitude | `0x4da850(obj, first, second)` stores `second` at `+0x28`, `first` at `+0x2c` | `0x20` | upper `0xa06f60`, lower `0xa06f64`, `1/(upper-lower)` `0xa06f68` (via `0x54e2c0`, `0x54e300`) |
| `+0x30` | `0x4da890` (script `WorldSetFogDensity`) | `0x8` | `0xa06f6c` (via `0x54e340`); **no code reads it** |

* **The flush is event-driven, not per frame.** `0x4da1b0` (reached from the dirty-object update `0x4ce450`, case
  class 14 at `0x4ce62d`, and from the loader `0x4e31d6`) tests the dirty bits (`0x4da23f` colour/range, `0x4da25e`
  altitude), pushes the changed fields to the `0x54e2xx` setters, then clears the mask. If more than one class-14
  object exists it forces the mask to `0x2f`. A field that nobody sets is never re-sent.
* **Writers of the state** (the call sites of the four setters): the zone apply `0x472ea0` (`0x472f09` range,
  `0x472f1d` altitude, `0x472f35` colour; applies the zone record, with the range multiplied by the active tier's
  `fog_scale`, see atmo-a); the by-name record applier `0x4db9d0` (registered with its dump `0x4db940` at
  `0x4d7896`, a state save/restore pair); the record loaders `0x4e3189..0x4e31d0` and `0x4e8556..0x4e85b1`; the
  script commands, registered at `0x5bc967` `WorldSetFogState`, `0x5bc8f0` `WorldGetFogRange`, `0x5bc87c`
  `WorldSetFogRangeFar`, `0x5bc808` `WorldSetFogRangeNear`, `0x5bc7a1` `WorldSetFogRange`, `0x5bc765`
  `WorldSetFogDensity`, `0x5bc6df` `WorldGetFogColor`, `0x5bc681` `WorldSetFogColor`, `0x5bc618`
  `WorldSetFogAltitude` (setter calls at `0x5bc66a`, `0x5bc6ca`, `0x5bc7f3`, `0x5bc867`, `0x5bc8db`,
  `0x5bc9bd`, `0x5bc9eb`, `0x5bca18`); and the cinematic mode pair `0x450550` / `0x450e30` (mode only, section 2).
  The readers of the stored fields are the flush above and the dump `0x4db940` (via the getters
  `0x4da8b0..0x4da940`) and, for the range, `0x49dcde` (a per-frame consumer that reads near/far of `0x71c0b8` and
  uses `near + 0.8 * (far - near)`, `0x6034b0`, as a distance limit for a camera-relative test; what it gates is not
  traced).
* **Boot defaults** (the renderer init `0x54da..`; replaced as soon as a zone or script writes): near 500, far 700,
  upper altitude 300, lower altitude 200 (rows `fog_default_*`), colour (1, 0, 1), mode 1.
* **Mode semantics** (`0xa06f38` read at `0x551e3b`, `0x5520c9`, `0x554ee3`, `0x554f17`): 0 = no fog test at all;
  1 = fog evaluated only for polygons whose flag word has bit `0x2`; 2 would fog everything, but no writer can
  produce 2 (`0x54e200` is only called with 0 or 1). Mode 1 is the boot value.
* **The fog factor** (`0x578120`, called at `0x551e5c` and `0x5520e5`; two array variants use the squared distances,
  `0x57843a`, `0x5788c8`): with `d` the horizontal distance of the vertex from the camera (`sqrt(x²+z²)` by an
  integer approximation, plus the polygon radius `r`), `h` the vertex's world height:
  `0` when `d <= near` or `h - r >= upper`; the range factor is `(min(d, far) - near) / (far - near)`; when
  `h + r > lower` it is multiplied by `(upper - max(h - r, lower)) / (upper - lower)`; when `d - r >= far` and
  `h + r <= lower` it is `1`; the result is clamped to `[0, 1]`. So `FOG_ALTITUDE` is a **height fade of the range
  fog**: full strength at and below the lower number, zero at and above the upper number. For M01 (9000 / 10000) the
  fade starts 9 km above every flown altitude, so below it the factor is the pure range factor; atmo-a's "do not
  apply `FOG_ALTITUDE` below 9000" is the same statement. Using it as a per-frame altitude fog in M01 is
  correct only above 9 km. **Not measured:** how this factor reaches a pixel (per-vertex colour mix or table), and the
  display-vs-linear colour space (atmo-a 6c).

## 2. The camera hand-over at the end of a cinematic

### 2.1 What the pair does

`0x450550` (cinematic mode entry; wrappers `0x450790`, `0x4507b0`, called from `0x431132` and `0x434862`) sets `0x654120 = 1` (`0x450562`), calls the mode setter with **0**
(`0x45067c`, `push 0 / push world / call 0x4da800`) and multiplies the camera far clip (`+0xb4`, getter `0x4d29c0`,
setter `0x4d2930`) by **4.0** (`0x4506a6`, row `cinematic_enter_far_clip_factor`; the near clip is unchanged).

`0x450e30` (called from `0x423e86`, the exit) calls the mode setter with **1** (`0x450ee1`), multiplies the far
clip by **0.25** (`0x450f0b`, row `cinematic_leave_far_clip_factor`, the exact inverse), runs the cleanup of the
list at `0x64ff4c`, and clears `0x654120` (`0x450f9b`). `0x654120` is the flag the cloud consumers test
(section 3: the camera tint and the `zone2_cloud_floor` toggle run only when it is 0; it is also read at
`0x42e6e6`, `0x46ba89`, `0x48746f`, `0x489cec` and others, whose purpose was not traced).

### 2.2 Consequences

* Fog **range, colour and altitude are not touched** at entry or exit; only the mode (0 off / 1 on) and the far
  clip change. There is no duration, curve or start/end value: both are single assignments.
* On exit the fog turns on and the far clip falls back to a quarter of its cinematic value, which equals the
  zone's `CLIP_RANGES` far times the tier's `clip_scale` provided nothing else wrote the clip in between (the zone
  apply writes it through `0x4d2930` at `0x472f3d..`, so a zone change in between would reset it; not traced).
* `VIEWING_RANGE`'s `fog_scale`/`clip_scale` change only when the tier index changes (`0x4bc320` stores the
  option at `+0x29c` and re-applies the current zone); there is no time dependence.

### 2.3 Unknown

* **Whether the M01 launch uses this pair.** Its callers are the camera-mode wrappers; which mission opcode or
  cinematic-end event calls them was not traced (needs the interp opcode table and M01's script, or a runtime
  trace). Until that is shown, "fog switches on at the hand-over" is a candidate, not a finding.
* Whether the cinematic world is drawn with a different fog (mode 0 means no fog) is the visible half of this and
  needs the same trace.

## 3. Altitude, the cloud band and fog

`CLOUD_COVER` (`TOP`, `BOTTOM`, `THICKNESS`, colours at the record `0x71dcd0`: byte `+0` present, `0x71dcd4` TOP,
`0x71dcd8` BOTTOM) is read by exactly these runtime consumers (atmo-a section 3 has the setup):

| consumer | what it does with the band |
| --- | --- |
| tint query `0x42ee40` (outputs colour and intensity; called at `0x42f49e` from the per-frame render function `0x42f480`, itself called from `0x49fc1f`) | when the camera Y is inside `[BOTTOM, TOP]` and `0x654120 == 0`: colour = `BOTTOM_COLOR -> TOP_COLOR` blended by `(y - BOTTOM)/(TOP - BOTTOM)`, intensity = the trapezoid (atmo-a); the tail mixes in the `fogvol.zrd` volume when one is active (`0x64ff30`, `0x64ff40`; M01 has none) and a term driven by a random draw (`0xa20350`) and the clock `0x9ad744` through `0x64efcc`/`0x62154c`, which reads like a flicker; its amplitude and purpose were not decoded |
| screen fill `0x42f4c4..0x42f553` | if the intensity is above 0 (`0x6032c8`): packs colour*255 (`0x60414c`) into the display pixel format (masks `0x9c6800..0x9c680c`) and calls `0x5ca1f0(colour, intensity as f64)` which forwards to `0x5ca0e0` on the 2D object `0x9c8a98`. This is the **only** in-layer visual of the band that is not geometry |
| per-vertex evaluator `0x5782d0` (called at `0x551e8b`, `0x552116`, enabled by `0xa06f70` and polygon flag `0x4`) | cloud trapezoid intensity per vertex (atmo-a) |
| zone selector `0x473580` | zone 1 below the midpoint, 2 above (drives the zone apply; the fog colour change when crossing 10 500 m) |
| `0x42b7c0` (called each frame from `0x489c9b` with the camera object `0x64ef78`) | **not fog**: gated on the weather record being present (`0x4bd040`) and a precipitation object (`0x71df68`); tests camera Y against `BOTTOM` (`0x42b80d`) and `BOTTOM + 0.8 * (TOP - BOTTOM)` (`cloud_inside_fraction`), runs a ray test (`0x4c8f70`, camera position with `+50` added to Y) and calls virtual slots `+0x60` / `+0x64` of that object (the first with `-1.0`); reading it as a precipitation on/off controller is an inference from the weather-record guard. M01 spells no precipitation block, so this controller has nothing to switch for M01 |

Nothing reads the band to change **fog near/far or colour**; the zone fog switches only at the midpoint. The
`zone2_cloud_floor` node (`0x71c2b4`, toggled by the camera function through `0x4cca30` when `0x654120 == 0`) is
absent from M01's `gamez.zbd` (atmo-a section 3). **No code reads `FOG_ALTITUDE` against the cloud band**; it is
independent of `CLOUD_COVER` and only coincides numerically in M01 (9000-10000 below `BOTTOM` 10000).

## 4. Unknowns kept open, with the content they affect

| unknown | evidence searched | content affected |
| --- | --- | --- |
| Which M01 launch event calls `0x450550`/`0x450e30` | callers found (wrappers), their own callers not followed to an opcode | #1744: whether the launch hand-over shows the fog-on / far-clip step |
| How `0x71c0b8` is assigned | no absolute store anywhere in the image (10 byte-pattern hits, all loads); terrain queries `0x44def9`, `0x44df26` dereference it unguarded, so it is set at run time by an indirect path (indexed array or struct copy) | none for values; the zone apply's `test eax, eax` guard (`0x472ee7`) is satisfied at run time |
| Blend of the screen fill `0x5ca0e0` | entry thunk read only | #1745: how strong the in-layer wash looks (colour `(32,56,72)/255`, intensity 0..1 are known) |
| Flicker term of `0x42ee40` | tail read, amplitude constants not decoded | #1745: whether the wash flickers |
| Pixel path of the fog factor | `0x578120` and its array variants read; the call at `0x551e5c` consumes it per vertex | exact drawn fog (colour-space residual of atmo-a stays) |
| What `0x49dcde` gates with `near + 0.8 (far - near)` | operand source only | a draw-distance limit inside the fog range |

Method: `llvm-objdump -d -M intel` of the decrypted image; byte-pattern searches for absolute addresses; the
committed test re-reads the six constants and the instruction bytes named in sections 1-3.
