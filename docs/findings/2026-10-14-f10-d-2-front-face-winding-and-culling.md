# F10-D.2: front-face winding, handedness and culling, measured from the original executable

Date: 2026-10-14. Task: #396 (`F10-D.2`), follow-up of F10-D (#44) and F10-B
deferred item 6 (`2026-09-30-f10-d-private-corpus-face-census.md` deferred
item 2; `2026-09-29-f10-b-gamez-mesh-layout.md` item 6). Owner paths used:
`crates/cs_app/src/render/bevy_mesh.rs`, `crates/cs_app/src/render/bevy_state.rs`,
`crates/cs_app/src/render/additive.rs`, `crates/cs_app/tests/render/` and
`docs/findings/`.

**Claim class: static code evidence, never `verified_original`.** The decision
below was measured by static analysis of the owner-supplied decrypted
executable, performed and accepted by the owner; no original run and no
`human_play`/`human_review` evidence exists. The owner accepted this code
evidence in place of a capture for this task (owner note
`OWNER-STATIC-2026-10-05`, the latest owner note on task #396, which carries
every address and count cited here verbatim).

## Source / provenance

* Decrypted image `crimson.decrypted.exe`, sha256
  `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`
  (decryption of `crimson.icd` `0e3b4724…9833b`). Since the owner note of
  2026-10-08 the image lives at `/Users/linus/coding/rust-crimson-skies/
  crimson.decrypted.exe` — the parent directory of `retail/`, outside
  `$CS_GAME_DIR` so the installation fingerprint is unaffected. It is
  read-only and **never copied into this repository**; this document commits
  addresses and observed behaviour, no executable bytes and no decompiled
  code.
* Analysed with Kuna v1.692; every condition below was checked in the
  disassembly. Addresses are VAs; file offset = VA − `0x400000` below
  `0x643000`.
* Cross-checked on all nine GameZ archives (F10-B layout; every walk ends
  exactly on `nodes_offset`).

## The decision

**The front face is the side from which the first three stored corners
P0→P1→P2 run counter-clockwise**, i.e. the right-hand normal
`(P1−P0)×(P2−P0)` points at the viewer. The original world is right-handed,
`+Y` up, and there is no mirror in the content→screen chain (owner note on
#390 §3, restated in `OWNER-STATIC-2026-10-05`).

In `docs/contracts/IDENTITY-CONTENT.md` / `cs_content::coordinates` terms:
the conversion applies the **identity axis map** (`X→X, Y→Y, Z→Z`, a signed
permutation with `det(M) = +1`, i.e. orientation-preserving —
`AxisMap::is_orientation_preserving`), the world is right-handed with `+Y` up,
and the front-face rule is **CCW**: the stored positions and stored corner
order upload bit-exact (the F17-B adapter's existing invariant) and
counter-clockwise is front. That is exactly Bevy's default
`FrontFace::Ccw`, so no index triple is reversed and no axis is remapped.

## What the original renderer does

**1. D3D cull state.**

* `0x5a0de0` = `IDirect3DDevice3::SetRenderState(22 CULLMODE, v)`.
* Everything is drawn with **D3DCULL_NONE** (1), **except triangle strips**:
  a polygon with dword bit 13 (`0x2000`, F10-B `TRI_STRIP`) gets
  **D3DCULL_CW** (2) at `0x557356`–`0x5573a3`. Exception: a runtime batch
  flag `[batch+0x1c] & 0x100` (set at `0x555630` on a per-vertex/clip path)
  keeps NONE.
* NONE is restored after each mesh (`0x5576f4`) and at `0x416cb9`/`0x56b94a`.
* CW culling means clockwise-on-screen triangles are back faces — the same
  front face (CCW) as the per-polygon test below.

**2. Per-polygon back-face elimination for outline (non-strip) polygons**,
in both paths: D3D `0x554bf7`–`0x554ce9` and software `0x552e1f`–`0x552f19`.

* The corners are copied in stored order and transformed by the 3×4
  model-view matrix into camera space: x right, y down, z into the screen,
  eye at the origin.
* `N = (P1−P0)×(P2−P0)` (the code computes the identical
  `(P2−P1)×(P0−P1)`); `d = N·P0`.
* `d < −tol` → front, drawn.
* `d > +tol` **and** polygon dword bit 10 (`0x400`) → the back side is drawn
  with `N` negated (two-sided). Front flag `0xa07148` = 0 is passed to
  lighting.
* Otherwise → **culled** (counter `0x63b1f4`). That includes `|d| ≤ tol`
  (edge-on, or a zero normal), **even for two-sided polygons**.
* `tol = 0x637a98`, default `0.005`. `support\init.gw` runs
  `SetBFETolerance 0.00005` on every world load (handler `0x5bb980` →
  `0x551240`), so the effective value is **0.00005**. `N` is unnormalised,
  so `d` is 2·area·(distance from the eye to the plane), in m³.
* D3D path only: the test is skipped for strips (`0x554bee`) and for whole
  meshes whose record word0 == 1 and word1 ∈ {0,1} (`0x554a6b`–`0x554a84`).

**3. Mesh word0 (F10-B `file_ptr`) is a mesh type**, from `0x553700`:
0 = normal; 1 = camera-facing billboard (word1 picks the facing mode: 0 →
`0x5408e0`, 1/2 → `0x5398b0`, 3 → `0x539c70`; word2 bit `0x80` pivots about
the centroid at `0x552180`); 2 = another path. Retail (word0, word1) counts:
(0,0) 15 759 meshes, (1,0) 396, (1,1) 944, (1,2) 40. Billboards hold 1–2
polygons each.

**4. Primitives.**

* Outline polygons → `D3DPT_TRIANGLEFAN` (6) over the stored (near-clipped)
  corners, **fanned from corner 0**: EBX=6 at `0x557334` → `[0x9be7d4]` at
  `0x5573f6` → `DrawPrimitive` (vtable +0x70, FVF 0x1c4) at e.g. `0x5a336c`.
* Strips → `D3DPT_TRIANGLESTRIP` (5) at `0x5573df`. The first triangle
  (v0,v1,v2) sets the facing, with normal strip parity after it.
* **The original never ear-clips or triangulates.**

**5. Software renderer only.** After projection, a polygon whose
`|shoelace sum over all corners| < 0x9fddfc` (4.0, set at `0x54da64`) is
dropped (counter `0x63b214`). The polygon is then filled by `zrndr_draw.c`
(`0x57afe0`/`0x57b200`/`0x57b330`/`0x57b610`).

## Retail cross-check (nine archives, F10-B layout)

* Sign of the right-hand normal of corners 0,1,2 vs the mean stored vertex
  normal: outlines with normals **29 957 agree / 1 312 disagree** (C4
  contributes 740 of the disagreements; excluding C4: **25 478 / 572**);
  strips **21 264 / 721**.
* Closed meshes (every edge paired): **224 of 229** with non-zero volume
  have positive signed volume, i.e. outward right-hand normals.
* Bit `0x400` (F10-B flag-field bit 2, `UNK2`) = **two-sided**. It is on
  **52 621** outline polygons and **0 of 22 124 strips**.

## What the F17-B adapter now does (this task)

* `crates/cs_app/src/render/bevy_mesh.rs` records the resolved decision:
  `FRONT_FACE = FrontFace::Ccw` and `CullingClass::cull_face()` — one-sided
  → `Some(Face::Back)`, two-sided (`0x400`) → `None`, billboard → `None` —
  each citing this document and `OWNER-STATIC-2026-10-05`. The adapter still
  uploads stored positions and corner order unchanged; the winding decision
  is the culling choice, not a geometry edit.
* `crates/cs_app/src/render/bevy_state.rs` reads its cull face from that
  decision. A surface whose two-sidedness is declared draws accordingly; a
  surface whose two-sidedness is unmeasured is still **refused**
  (`StateError::CullFaceUnknown`) — the rule is measured, but which stored
  surface carries `0x400` is per-polygon data nothing has plumbed yet, and
  the refusal keeps that gap explicit.
* `crates/cs_app/src/render/additive.rs` writes `front_face = FrontFace::Ccw`
  into the specialized pipeline explicitly instead of relying on wgpu's
  default, so the measured winding reaches a real pipeline state.
* `crates/cs_app/tests/render/winding.rs`
  (`accept_f10_d_2_front_face_winding_and_culling_…`) fails if the CCW front
  face, the one-sided back cull, the two-sided or the billboard choice is
  removed or changed.

## Lighting, normals and two-sided claims (same evidence)

All three cite `OWNER-STATIC-2026-10-05` and this document only:

* **Two-sided lighting:** the back side of a `0x400` polygon is drawn with
  the face normal negated and front flag 0 passed to lighting (§2 above).
  The negated-normal path is *not yet replicated* in this repository: no
  per-polygon `0x400` data reaches the adapter (see below), so no lighting
  claim about back sides is made here.
* **Normals:** stored vertex normals pass through bit-exact (F17-B
  invariant). The retail agreement counts above are evidence that stored
  normals and stored winding usually point the same way (outward); they are
  not re-derived or renormalised anywhere.
* **Two-sided:** the *rule* (`0x400` → both sides, back side with negated
  normal) is measured; the *data* (which polygons of which surface carry
  `0x400`) is not yet read from the stored flags by any production path.

## Explicitly still unknown or unreplicated

* **Which stored surfaces are two-sided.** `0x400` is per-polygon stored
  data (`RawPolygon::raw_flags`, unread by design in F10-B); it has not been
  plumbed into `RenderMesh`/`MaterialFacts::two_sided`. Until it is,
  per-surface two-sidedness stays `MaterialUnknown::TwoSided` and
  `StateError::CullFaceUnknown` refuses undeclared surfaces. **Resolving
  task:** F10-F.1 (#1406).
* **Billboard meshes.** The mesh record words that identify a billboard
  (word0 == 1, word1 ∈ {0,1}) are not surfaced in the render IR, so nothing
  yet produces `CullingClass::Billboard`; the decision is recorded so the
  plumbing task only has to feed it. Word1 == 2 (40 retail meshes) does
  **not** skip the test in the original, so it is not exempt: it draws under
  the normal per-polygon rule, and whether one of its polygons is two-sided
  is the same unread `0x400` data as above — nothing here assumes such a
  polygon is one-sided. **Resolving task:** F10-F.2 (#1407).
* **Strip culling parity.** The original draws strips with D3DCULL_CW and
  never per-polygon-tests them; this repository's reader triangulates
  everything into plain triangle lists (F10-B). How D3DCULL_CW parity maps
  onto each stored strip triangle after that triangulation is **not**
  re-derived here.
* **The edge-on / zero-normal rule.** The original never draws a polygon
  whose first-three-corner normal is exactly zero, and culls `|d| ≤ 0.00005`
  — a *view-dependent* band. This repository does not implement that test;
  every uploaded triangle reaches the GPU. Only "exactly zero normal is
  never drawn" is unconditional, and it is unreplicated. **Resolving task:**
  F17-F.3 (#1408).
* **The software-renderer shoelace drop** (`|sum| < 4.0`) is specific to the
  original's software path and is not replicated.
* **The batch flag `[batch+0x1c] & 0x100`** that keeps D3DCULL_NONE on a
  clip path is recorded, not replicated.
* **Why C4 disagrees more often** (740 of the 1 312 outline disagreements)
  is not settled by the note; the C4 cluster stays an open question about
  that archive's stored normals, not about the winding decision.

The same owner note carries two side findings for other tasks (loader
`0x560dc0`'s `uvs_ptr`/`colors_ptr` names being shifted by one word, and
node flag bit `0x4` gating the display traversal); they are recorded in the
task history for F10-B/F11 and are not acted on here.

## Gates this closes — and what it does not

This closes F10-B deferred item 6 and F10-D deferred item 2 **as far as the
decision goes**: the winding/handedness/culling rule is no longer unknown; it
is measured static code evidence accepted by the owner. The claim this
document and the adapter make is at most **checked**. It does **not** award
`verified_original` for the mesh path, and any claim that this
reimplementation *culls exactly like the original* remains gated on the
follow-ups above (two-sided data plumbing, billboard identification, strip
parity and the zero-normal/edge-on elimination).
