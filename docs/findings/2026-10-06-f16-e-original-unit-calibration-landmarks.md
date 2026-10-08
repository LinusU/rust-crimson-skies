# F16-E: original unit-calibration landmarks from static analysis

**Task:** #390 (`F16-E`), follow-up to F16-D (#68) and to the owner's 2026-10-05
note on #390. **Status of the claim:** implemented, `checked` at best — the
conventions below are measured by static analysis of original bytes, never
observed running, and nothing here is `verified_original`.

* `crates/cs_content/src/coordinates.rs` — the two measured
  [`CoordinateSource`]s, their attached [`UnitCalibration`]s and the
  landmarks behind them.
* `crates/cs_content/tests/accept_f16_e_original_unit_calibration_landmarks.rs`
  — the `accept_f16_e_` suite that pins every convention, gap and claim
  class below.

[`CoordinateSource`]: ../../crates/cs_content/src/coordinates.rs
[`UnitCalibration`]: ../../crates/cs_content/src/coordinates.rs

## 1. Evidence provenance

| What | Value |
| --- | --- |
| Decrypted executable every code landmark cites | `$CS_ENGINE_IMAGE` |
| Image SHA-256 (`ORIGINAL_IMAGE_SHA256`) | `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` |
| Installation hash (`install_sha256`) | `c14a876f4457d8710dee7986333ab636122c9549cf72b646fd69cbe7e72c5352` |
| Content hash (`content_sha256`) | `148a24b7b0506812e8f1ee13d8d3137a05926abebbe10161994e8c4cd300c35e` |
| Retail file the data landmark cites | `ZBD/zrdr.zbd` (`ZRD_READER_ARCHIVE`) |
| Retail file SHA-256 (`ZRD_READER_ARCHIVE_SHA256`) | `76b510d821edd2268040d2ccb18c462ec07ad580cdba571b3066228e2cf592dd` |
| Landmark inside it | member `anim.zrd`, `ANIMATION_DEFINITIONS/GRAVITY`, value at byte 28703 (4 bytes) |

> **Installation note (#798, 2026-10-08).** The two installation digests
> above were recorded while the owner's decrypted image still sat inside
> `$CS_GAME_DIR`, where it was an inventoried file and therefore part of both
> (`c14a876f…` / `148a24b7…`). The owner has moved it out for good, so
> `accept_f16_e_the_recorded_hashes_still_describe_this_installation` now
> re-derives `install_sha256 = b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`
> and `content_sha256 = a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d`
> from the read-only tree — the same values the pre-image evidence recorded
> (`docs/findings/missions/2026-10-08-f50-c-probe-routes.md`, its
> "Installation note", and `2026-09-29-f14-d-retail-baseline-inventory.md`).
> The table above keeps the values this run actually recorded. The image is
> read from `$CS_ENGINE_IMAGE`, never from the installation.

* The owner decrypted `crimson.icd` and supplied the image; the owner's note on
  #390 records the digest, and `accept_f16_e_the_recorded_hashes_still_describe_this_installation`
  re-hashes the file, the installation, the content set and the retail archive
  on every retail run, so a drifted file fails instead of silently backing
  evidence about different bytes.
* Addresses are **virtual addresses**. Below VA `0x643000` the owner's
  `.text`/`.rdata`/`.data` file offset is `VA − 0x400000`; every address
  recorded here is below that bound (the acceptance test asserts it), which is
  how the addresses could be checked against the image's own bytes.
* Cross-checks on the retail data files were read-only; nothing was written
  into `$CS_GAME_DIR`, and no executable bytes, decompiled code or game
  content are committed — only digests, addresses, constants, instruction
  mnemonics, counts and claim labels.
* Method on every landmark is `ObservationMethod::ByteInspection` — the
  schema's word for inspecting stored bytes directly, i.e. static analysis.
  `EvidenceSource::OriginalInstallation` says where the bytes came from and the
  fingerprint says *which* bytes.

### Independent cross-checks performed for this task

The owner's note is the measurement; transcribing it blindly would be
guesswork, so the numeric and instruction-level claims were re-read from the
image at `VA − 0x400000` before recording. All of them matched:

| Claim | Check |
| --- | --- |
| metre → foot factor `3.2808399` at VA `0x60813c` | f32 bits `0x4051F948` |
| m/s → mph factor `2.2369363` at VA `0x6076e4` | f32 bits `0x400F29F7` |
| needle constant `−0.02811017` rad per (m/s) at VA `0x6076e8` | f32 bits `0xBCE6474D` |
| altimeter factor `3.2808399` at VA `0x6076f0` | f32 bits `0x4051F948` |
| gravity normaliser `9.82` at VA `0x6080d4` | f32 bits `0x411D1EB8` |
| π/180 double at VA `0x6040e8` | f64 `0x3F91DF46A2529983` = 0.01745329251994 |
| 180/π double at VA `0x604100` | f64 `0x404CA5DC1A63C0B1` = 57.29577951308 |
| gravity default at VA `0x4ee406` | `MOV [0x9fd164], 0xc11ccccd` — the immediate dword is `0xC11CCCCD` = −9.8f |
| altitude load in the function the owner cites at VA `0x48fc40` | `FLD [esi+0x208]` at VA `0x48fc51`, immediately after that function's prologue, then `FMUL [0x60813c]` — the position's middle word |
| weight application at VA `0x48ff88`–`0x48ff9d` | `FLD [ecx+0xc4]`, `FDIV [0x6080d4]` (9.82), `FMUL [esi+0x674]`, `FSUB [edi+4]` — the `+4` (y) component only |
| position copy at VA `0x491fd8` | `LEA EAX,[esi+0x204]` then three dwords copied — the three-float vector |
| `.zrd` angle conversion in the key block the owner cites at VA `0x4aa705` | `FMUL QWORD [0x6040e8]` at VA `0x4aa708` (and at `0x4aa72c`, `0x4aa73e`), the π/180 double |
| cull mode, caller at VA `0x557381` | `push` + `call 0x5a0de0` at VA `0x55739a`–`0x55739f`, and the callee pushes `0x16` (= 22, `D3DRENDERSTATE_CULLMODE`) at VA `0x5a0df1` before the vtable call |
| `ANIMATION_DEFINITIONS/GRAVITY = −9.8` in `ZBD/zrdr.zbd` | key at byte 28684, f32 bits `0xC11CCCCD` at byte 28703, inside the `anim.zrd` member (offset 28623, length 8335) |

The owner's GameZ-wide data census (nine archives: 29,957 of 31,269 outline
faces and 21,264 of 21,985 strips agree in winding sign, 224 of 229 closed
meshes have positive signed volume; node `scale` = 1.0 in all 51,611 object
records) is quoted as the owner recorded it; re-running it is the census
`accept_m01_lc_world_unit_roles_every_scale_landmark_is_re_observed_over_the_installation`
already performs for the scale landmarks.

## 2. The convention each source declares

Both formats land in the same world frame, with an **identity axis map**, and
the two declarations differ in exactly one quantity — the angle unit.

| Quantity | `retail.gamez` (GameZ meshes/nodes) | `retail.zrd` (`.zrd` documents) |
| --- | --- | --- |
| Scale | `meters_per_unit = 1.0` | `meters_per_unit = 1.0` |
| Axis order | `X→X, Y→Y, Z→Z`, all positive (+Y up, X/Z horizontal) | same |
| Handedness | right-handed, `RotationSense::RightHandRule` | same |
| Winding | `Winding::CounterClockwise` front faces against the +Z reference, orientation-preserving | same |
| Angle unit | **radians** | **degrees** (`.zrd` text fields) |

`Origin::Installation` over the span the caller supplies, provenance class
`observed_tool` (claim ids `f18-world.gamez-vertex-unit-is-the-metre` for
GameZ, `f16-e.zrd-document-world-convention` for `.zrd`).

## 3. Landmarks

Every landmark **F16-E recorded** below is `LandmarkKind::Artifact`. **No
behavior landmark was recorded** — owner decision 1: a behavior is what the
running original does, and no original run exists (#358). Each of those
evidence records fingerprints the image (or the retail file) and locates one
address (or one member span). The one row that is not F16-E's own work says so:
`retail.gamez` additionally carries #677's five scale-census records (two
artifacts and three `LandmarkKind::Behavior` records from that tool-run
census), which is what closes its scale quantity; F16-E added nothing to them.

### `retail.gamez`

| Quantity | Locator | What was observed |
| --- | --- | --- |
| Scale | VA `0x48fc40` | aerodynamics converts altitude `[plane+0x208]` and airspeed `|v|` to feet by ×3.2808399 (const VA `0x60813c`), feeding the two-band atmosphere and the ½ρv² dynamic pressure: the world unit — the unit GameZ node translations are stored in — is the metre |
| Scale | VA `0x453aa2` | HUD airspeed ×2.2369363 (const VA `0x6076e4`); needle constant VA `0x6076e8` = −0.02811017 rad per (m/s) = 2π/500 × 2.2369363, one turn per 500 mph |
| Scale | VA `0x453d3b` | HUD altimeter ×3.2808399 (const VA `0x6076f0`): a stored altitude is metres |
| Scale | *(plus #677's five landmark-census records: 61 animation containers' −9.8 gravity word, pilot figure, airframe subtrees, LOD bands, world bounds)* | |
| Axis order | VA `0x491fd8` | the three-float position vector is copied from `[plane+0x204]` and altitude is its middle word `[plane+0x208]` — +Y is up |
| Axis order | VA `0x48ff88`–`0x48ff9d` | weight acts on −y: `force.y −= (gravity/9.82)·[plane+0x674]`, only the `+4` component |
| Axis order | VA `0x53df30` | matrix→Euler reads y as vertical: pitch = asin(m7), yaw = atan2(m6, m8) from x/z, roll = atan2(m1, m4); matrices are 3×4 floats, rows = images of local x, y, z (layout used at VA `0x552915`) |
| Handedness | VA `0x53afc0` (+ `0x53d880`, `0x540fa0`, `0x4d3010`) | view matrix is inverse(R·diag(1,−1,−1)), a proper rotation; projection `sx = Cx + Kx·x/z`, `sy = Cy + Ky·y/z` with both cot factors > 0; near clip keeps z ≥ near; camera node looks along local −Z with +Y up — right-handed as displayed |
| Handedness | VA `0x552915` (+ `0x554bf7`) | back-face rule `((P1−P0)×(P2−P0))·P0 < −tol` in camera space, agreeing with retail mesh winding only when content → screen carries no mirror (the census above) |
| Handedness | VA `0x557381` → `0x5a0de0` | `D3DRENDERSTATE_CULLMODE = D3DCULL_CW` (`SetRenderState 22`) — the hardware path culls clockwise screen triangles, the same side rule |
| Angle unit | VA `0x53b9a0` | the Euler view builder passes stored angles straight to fsin/fcos; zero angles look along world −Z |
| Angle unit | VA `0x4d2b31`–`0x4d2b7d` | the FOV half-angle goes into FPTAN with no degree conversion: radians natively |
| Angle unit | VA `0x604100` | 180/π is used only to produce degrees for display, while π/180 (VA `0x6040e8`) serves `.zrd` text — GameZ euler triples are never converted (owner's corpus check: the maximum is exactly π) |

### `retail.zrd`

| Quantity | Locator | What was observed |
| --- | --- | --- |
| Scale | `ZBD/zrdr.zbd` member `anim.zrd`, byte 28703 (4 bytes) | `ANIMATION_DEFINITIONS/GRAVITY` is the f32 −9.8 (bits `0xC11CCCCD`): Earth's acceleration in m/s², an SI value only under the metre |
| Scale | VA `0x520420` | the `.zrd` reader stores that field straight into the engine's gravity variable at `0x9fd164`, unconverted |
| Scale | VA `0x4ee406` | the compiled default for the same variable is −9.8f (`MOV [0x9fd164], 0xc11ccccd`) |
| Axis order | VA `0x491fd8` | the frame the document's values are read into is +Y up (position's middle word is altitude) |
| Axis order | VA `0x48ff88`–`0x48ff9d` | weight acts on −y, `+4` component only |
| Axis order | VA `0x53df30` | the matrix→Euler decomposition treats y as vertical |
| Handedness | VA `0x53afc0` (+ `0x53d880`, `0x540fa0`, `0x4d3010`) | view/projection are right-handed as displayed; the unrotated camera looks along −Z |
| Handedness | VA `0x552915` (+ `0x554bf7`) | back-face rule with a right-hand normal; outside-out rendering needs no mirror |
| Handedness | VA `0x557381` → `0x5a0de0` | `SetRenderState 22` = `D3DCULL_CW`, the same side rule |
| Angle unit | VA `0x6040e8` | every `.zrd` angle field is multiplied by π/180 (123 references): degrees in the file |
| Angle unit | VA `0x4aa705` / `0x4aa72c` / `0x4aa73e` | the turret keys INACCURACY, PITCH, YAW convert through that factor |
| Angle unit | VA `0x4d2b31`–`0x4d2b7d` | after conversion the engine's trig takes radians, so the text had to be degrees |

Engine-level code evidence legitimately backs both sources: a source's
convention is the frame *its* values are expressed in, and both formats feed
one world frame. Independence is per calibration — within each one, every
landmark has its own address or span, which is the rule
`UnitCalibration::record` enforces and the acceptance test re-checks by
comparing observations rather than descriptions.

## 4. Shape change and derivation path (reviewer note #1488)

* `CoordinateSource` now carries a `calibration: UnitCalibration` field.
  `CoordinateSource::with_calibration(label, …, calibration)` refuses a
  calibration whose `source()` is not that label
  (`SourceError::CalibrationSourceMismatch`), so source A's record can never be
  reported under source B's gaps. `CoordinateSource::new` still builds an
  **empty** calibration, so the F16-A fixtures and
  `accept_f16_a_source_adapters.rs` are untouched, and
  `CoordinateSource::record_landmark` now records straight into the field.
* **Derivation is by hand-transcription**: the measured values (metres, +Y
  up / right-handed / identity axis map, radians for GameZ, degrees for `.zrd`
  text) are typed into `SourceConvention::new` and pinned by the
  `accept_f16_e_` tests. No typed `Landmark` payload is needed — a landmark's
  description stays free text, and nothing in production reads a description to
  build a convention.

## 5. What stays unmeasured

| Gap | Why | Where it is recorded |
| --- | --- | --- |
| Aircraft body forward axis | the camera's −Z is measured; the airframe's is not | this note; `retail.gamez` convention declares the *content* frame, not the airframe's own forward axis |
| Compass / heading zero | not measured by static analysis | this note |
| **Any behavior landmark, for any quantity, in either source** | needs a run of the original (#358) | `UnitCalibration::gaps()` on both sources: every reported gap has `0/1 behaviors` with all three artifacts already recorded |

Consequences the tests pin:

* `retail.gamez` gaps = `[handedness, axis-order, angle-unit]` — the scale is
  closed by #677's *observed-behavior* landmarks (tool-run census), the rest
  have their three artifacts and lack a behavior.
* `retail.zrd` gaps = all four quantities, each at `0/1 behaviors`.
* `is_complete()` is **false** for both, so `claim_status()` is `unknown` and
  can never read as `verified_original`; `quantity_status(scale)` on GameZ
  stays `observed_tool`.
* `accept_f16_d_artifacts_alone_never_calibrate_a_quantity` and the other
  `accept_f16_d_` tests are unchanged and still pass: F16-E records artifacts
  only, and the rule that a quantity additionally needs a behavior is what
  keeps these sources honestly incomplete.

### Relation to `docs/findings/2026-10-05-m01-lc-world-unit-roles.md`

That note (task #677) says the axis map, handedness, angle unit, rotation sense
and front-face rule are "DECLARED, not measured". This task measures them by
static analysis, so for those three quantities the sentence is superseded
here — **the scale measurement itself is unchanged**, and its gap-based
reporting is unchanged (a gap for those quantities still exists, now because
the behavior landmark is missing rather than because no landmark exists).

### Statements outside this task's owner paths that F16-E supersedes

These are **not** edited here (owner paths for #390 are
`crates/cs_content/src/coordinates.rs`, `docs/findings/` and the new
`accept_f16_e_` test); a follow-up task is filed to sync them:

* `crates/cs_app/src/world/retail.rs` — "whose scale is pinned to the metre by
  task #677's landmark census **while its other quantities stay declared**":
  the other three quantities are now measured by static analysis; what stays
  open is the *behavior* landmark, not the measurement.
* `crates/cs_content/src/world.rs` — the `WORLD_UNIT_UNMEASURED` doc says the
  rest of the convention is unmeasured because "the container family stores no
  handedness, axis-order or angle-unit declaration anyone has tied to an
  original behavior": still true of the *container family* and still true of
  "tied to a behavior", but the quantities themselves are now measured from
  the executable, so the sentence understates the record.
* `crates/cs_app/tests/evidence_report_m01_lc_world_unit_roles.rs` — the
  generated `review.method` text of the committed
  `docs/findings/evidence/M01-LC-WORLD-UNIT-ROLES.json` says "the axis map,
  handedness and angle unit are unmeasured". That report is a record of the
  run at its own candidate tree; regenerating it belongs to M01-LC's harness.

None of these affect any assertion: every gap list, `quantity_status` and
`unit_class` those files report is unchanged (a gap for those quantities still
exists — now because the behavior landmark is missing rather than because no
landmark exists). Filed as **#719** (sync the prose above), together with
**#720**: `retail_zrd` is a declaration without a runtime consumer yet — the
first `.zrd` angle field the project parses must convert through its
`SourceAdapter`, not through a hand-rolled π/180.

## 6. Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f16_e_ --include-ignored
cargo test --workspace --locked -- accept_f16_d_ --include-ignored
cargo test --workspace --locked -- accept_m01_lc_world_unit_roles --include-ignored
```

Results are recorded in the handover summary for task #390.

## 7. Limits of this evidence

1. **Retail is file access.** No original executable ran. Static analysis of a
   decrypted image plus byte censuses over retail files is `observed_tool`
   evidence at best; it never becomes `verified_original` without an
   owner-supplied original run (#358, REF-OWNER-FIRST-CAPTURE).
2. The measurements are about the **program's** conventions; whether the game
   *as played* behaves under them (a turn, a compass reading, an aircraft body
   axis) is behavior evidence that does not exist yet.
3. The installation hashes describe this owner's installation at this state. If
   `$CS_GAME_DIR` changes, `accept_f16_e_the_recorded_hashes_still_describe_this_installation`
   fails loudly and this note must be re-measured, not reworded.
