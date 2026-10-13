# T-OBJECT3D-TRANSFORM: one rule for a stored object3d transform

Date: 2026-10-13. Task: #645, filed in the #629 review: two production paths
in `cs_content` converted the same stored `object3d` record into a canonical
transform under **different** precedence rules, so one record could in
principle be placed in two different places. Owner paths used:
`crates/cs_content/src/scene.rs`, `crates/cs_content/src/world.rs`,
`crates/cs_content/tests/` (unused in the end — the acceptance tests live as
in-crate unit tests) and `docs/findings/`. Required capability: ordinary
build/test only — no `retail`, no evidence report.

**Verdict: one rule, called once.** The conversion lives in
`cs_content::scene` — `authored_object_transform` (one stored
`RawObject3dData` → one `AuthoredTransform`) plus `canonical_local` (one
`AuthoredTransform` → one `CanonicalTransform`) — and both production paths
call it: `scene_graph_from_gamez` through `SceneGraph::build`, and
`import_world_container` directly. `cs_content::world::canonical_transform`,
the second copy of the precedence rule added by #629, is deleted. This
document awards at most **checked**.

## The two rules, before this task

* **Scene rule** (`scene::canonical_local` + `authored_transform`): returns
  the identity only when the record flags identity **and** really is the
  identity (rotation and translation zero, scale one); otherwise builds the
  linear map from the euler triple — converted through the adapter's declared
  angle unit — unless the stored 3×3 disagrees with it
  (`RawObject3dData::matrix_disagrees`, tolerance `1.0e-4`), in which case
  the stored one wins.
* **World rule** (`world::canonical_transform`, #629): returns the identity
  whenever the record flags identity (discarding whatever else it stores),
  and otherwise always uses the stored 3×3 — never the euler path, no angle
  conversion.

## Measured divergence over all eight world containers (production reader)

| container | world-owned object records | flagged identity | flagged identity but NOT really identity | non-identity records | stored matrix disagrees with the euler triple |
| --- | --- | --- | --- | --- | --- |
| c1 | 412 | 340 | 0 | 72 | 1 |
| c1b | 233 | 154 | 0 | 79 | 0 |
| c1c | 346 | 316 | 0 | 30 | 0 |
| c2 | 282 | 252 | 0 | 30 | 0 |
| c2b | 338 | 304 | 0 | 34 | 0 |
| c3 | 453 | 436 | 0 | 17 | 0 |
| c4 | 401 | 344 | 0 | 57 | 0 |
| c5 | 576 | 411 | 0 | 165 | 0 |

So the divergence was **latent**: the flagged-identity-but-not-identity case
does not occur in retail (0 of 8 containers), and exactly one record in the
whole corpus — one c1 record — has a stored matrix that disagrees with its
euler triple, which is precisely the case where the two rules placed it
differently. Every other record's placement was identical under both.

## The adopted rule

The **scene rule** wins, because it is the rule that never discards the
store's own words: the identity flag is honoured only when the record really
is the identity (a flagged-but-not-identity record keeps its stored numbers,
which the format reader separately reports as `ObjectIdentityNotIdentity`),
the euler triple stays convertible through the adapter's declared angle unit,
and the stored 3×3 wins only when it disagrees with the triple. Both paths
now convert every record through `scene::canonical_local`, so `c1`'s single
disagreement record is placed under the adopted rule by **both** the scene
build and the world import, and the same is true of every agreeing record
(now derived from the euler triple in full precision rather than read from
the stored f32 matrix — a difference below the reader's own `1.0e-4`
agreement tolerance, and only in the last bits).

`WorldImportReport::matrix_disagreements` is unchanged: it counts **every**
imported record whose stored matrix disagrees with its euler triple
(#629's review requirement), so the number stays honest under whichever arm
places the record.

## What this is not

This is a **conversion-precedence** question, not an original-run
measurement: no claim is made here about where the original engine placed any
record. Both rules stay labelled as designed conversion rules with their own
provenance — the stored-matrix precedence and the euler convention
(`Rz·Ry·Rx` over negated angles) come from the pinned reference's reader
(observed-tool evidence, not verified against the original executable), and
the `rotation · scale` composition order is a designed choice (every measured
CS scale is `1.0`, so the order is unobservable in the corpus). The affected
content was latent — no retail record changes placement under the adopted
rule except `c1`'s one disagreement record, which both paths now place the
same way; resolving task: this one (#645). The behaviour is pinned by the
`accept_t645_*` unit tests in `crates/cs_content/src/scene.rs`, which fail if
either arm of the rule is removed or the identity flag starts discarding
stored words again.
