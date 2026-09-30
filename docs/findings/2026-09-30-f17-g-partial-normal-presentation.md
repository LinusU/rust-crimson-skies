# F17-G: presenting a material group with partly stored normals

Date: 2026-09-30. Task: #435 (`F17-G-partial-attribute-presentation`). Capabilities
used: `retail` (reading `$CS_GAME_DIR`) and `gpu` (the F18-D capture test, re-run).
No original run, no `human_play`/`human_review`. Result: at most **checked**.

## The finding this answers

F18-D measured 23 of 24 representative world meshes refused by the F17-B upload
adapter: `material group N carries a normal on X of Y vertices`
(`docs/findings/2026-09-30-f18-d-world-group-audit-and-gpu-capture.md`, limitation 2).

## What the data says (measured)

The GameZ layout stores normal indices **per polygon**, behind the polygon's own
`NORMALS` flag (`2026-09-29-f10-b-gamez-mesh-layout.md`: one normal index per corner
"only if the `NORMALS` bit is set"). A render vertex is keyed on its stored normal
index (`2026-09-29-f10-c-01-render-vertex-splitting.md`), so a vertex never belongs
to both a normal-bearing and a normal-free polygon. "A normal on 65 of 111 vertices"
therefore means **some polygons of the group store normals and others store none**,
not that some corners of one polygon are missing one.

Measured over the 24 representatives
(`accept_f17_g_retail_representatives_upload_under_the_partial_normal_policy`,
retail): **59 material groups** are refused by the strict adapter; **all 59** split
cleanly into a normal-bearing and a normal-free part, every part uploads, each
group's parts add up to exactly the group's stored triangles, and no group is
refused for any other reason.

## The decision (engineering, declared)

`cs_app::render::bevy_mesh::PARTIAL_NORMAL_POLICY = SplitByStoredPresence`.
`upload_groups` (the world path) and the F18-D capture now go through
`upload_group_parts`: a group whose polygons differ in normal presence becomes two
`GroupUpload`s, `GroupPart::NormalBearing` and `GroupPart::NormalFree`, sharing the
same `group()` and `material()`. Every stored value is uploaded unchanged; **no
normal is invented and no stored normal is dropped**. A whole group keeps its old
fingerprint; parts carry a part tag in theirs. `upload_group` (strict) is unchanged
and still refuses a partial attribute, as does `upload_group_parts` for a single
triangle whose own corners mix stored and absent normals, and for partial UV/color
sets.

The alternatives are distinguished by `accept_f17_g_partial_normals_split_by_stored_presence`:
padding would give the normal-free part a normal buffer; dropping would leave the
normal-bearing part without one; the strict adapter must still refuse the same mesh.

## What is **not** decided (evidence status: unmeasured)

`ORIGINAL_NORMAL_FREE_BEHAVIOR = Unmeasured`. The task's candidate readings — the
normal-free polygons were unlit, flat, computed by the renderer, or decal/overlay —
remain **open**. This session obtained no evidence for any of them: no original run
exists, `crimson.exe`'s rendering path was not disassembled here, and the
repository's recorded mech3ax material (`docs/research/SOURCES.md` S02/S06) records
the flag's layout, not its shading semantics. The split is the choice that does not
pre-empt any of them: a later stage can shade the `NormalFree` part unlit, flat or
computed without touching the data path.

## Limitations that survive this task

1. **Shading of `GroupPart::NormalFree` geometry is undecided.** Affected content:
   every normal-free polygon of every world mesh (59 groups among the 24
   representatives alone). Gates any lighting/appearance fidelity claim about world
   geometry. Resolving work: an original-reference capture (`REF-OWNER-FIRST-CAPTURE`)
   or a recorded disassembly of the original's polygon draw path; no task is
   assigned yet.
2. `merge_group_meshes` (F18-B) still drops a normal attribute when merged parts
   disagree, reported in `WorldMesh::dropped_attributes`; a mesh with split parts
   therefore merges without normals for collision. That is unchanged collision-side
   behavior, and visuals that need the normals must consume the parts.
3. `UploadVerdict::Uploaded::groups` (F18-D census) now counts **uploads**, so a
   split group counts twice.
4. Partial UV or color sets are still refused; none was measured on the
   representatives.

## Files

`crates/cs_app/src/render/bevy_mesh.rs`, `crates/cs_app/src/world/gpu_capture.rs`
(capture uses the policy), tests in `crates/cs_app/tests/render/{adapters,fixture}.rs`
and `crates/cs_app/tests/world/audit.rs` (the F18-D GPU test's "some representatives
are refused" assertion became "none is", as the policy lifts exactly that case).
