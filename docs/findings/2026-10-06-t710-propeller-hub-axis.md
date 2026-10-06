# #710 `PLAYTEST-PROP-SPIN`: the propeller's hub, measured from its own disc

* **Task:** #710 `PLAYTEST-PROP-SPIN` — owner playtest feedback 2026-10-06,
  "the propeller doesn't spin".
* **Status:** measured (`ObservedTool`, container bytes through the production
  readers) except where this document says *designed*. No original run
  happened; nothing here is `verified_original`.
* **Source:** `$CS_GAME_DIR/ZBD/planes.zbd`, the `bloodhawk` airframe, the node
  whose stored slot is `2541` and whose authored name is `staticprop1`
  (`PLAYTEST_AIRCRAFT_PROP_NODE_SLOT`), and the mesh-array slot that node binds.
* **Reproduce:**

  ```sh
  CS_GAME_DIR="$CS_GAME_DIR" cargo test -p cs_app --locked --test playtest_retail -- \
    accept_playtest_prop_spin_hub_is_measured_from_the_drawn_propeller_disc --include-ignored \
    --nocapture
  ```

## Why it had to be measured

The free-flight playtest draws exactly one of the six stored propeller states
(`staticprop1`; `prop1`, `prop1b`, `prop2`, `prop2b`, `nitroprop1` stay undrawn
because which one the original shows when is unmeasured — the node flag bits
that would say are unmeasured). To spin that disc, two things are needed that no
constant in the repository can supply: **which line it turns about** and
**which point it turns around**. Task #709 had already established that this
airframe is a *rear*-propeller layout — the disc sits behind the rudder
(`z = +3.03`) — so a hub assumed at the nose would have made the propeller
orbit the tail instead of turning about its own axle.

## How it was measured

`measure_propeller_hub` in `crates/cs_app/src/playtest_retail.rs`, over the
`RenderMesh` the production F10-E builder produces for that mesh-array slot:

1. every non-degenerate triangle contributes its **area-weighted unit normal**;
   a triangle whose winding disagrees with the largest triangle's is
   sign-aligned first, because a disc has one normal line and a stored mesh's
   winding is a rendering convention;
2. the **pivot** is the area-weighted centroid of the triangle centroids;
3. the **radius** is the furthest vertex from that pivot *in the measured
   plane*, the **thickness** the furthest vertex *out* of it;
4. the normal's sign is oriented **aft**, away from the measured
   `STORED_AIRCRAFT_NOSE_AXIS` (`−Z`, #709) — a **designed** convention
   (`playtest-retail.propeller-spin-sense-is-designed`), because no
   measurement says which way the original turned a propeller;
5. a surface thicker than it is wide is refused by name
   (`PropellerError::NotADisc`) rather than given a plane it does not have.

## The numbers (pinned pair, measured 2026-10-06)

```
PLAYTEST-PROP-SPIN measured slot=2541 name=staticprop1 triangles=16
  axis       = [1.4306629e-9, -3.9087036e-10, 1.0]
  pivot      = [2.2329578e-8, -0.019357027, 4.6815853]
  radius_m   = 1.2529234
  thickness_m= 0.16803837
  composed pivot (airframe frame) = [2.2329578186486287e-8,
                                     -0.019357027485966682,
                                     4.681585311889648]
```

Read off:

* the disc's **16 triangles** lie in one plane whose normal is the stored `+Z`
  — after the node composes it, that is the flight body's own length axis
  (`BODY_FORWARD = −Z`, so the axis is perpendicular to the flight and the disc
  faces along it). The test asserts the composed axis keeps `> 0.9` of `BODY_FORWARD`;
* the **pivot** is at `y ≈ −0.0194`, `z ≈ +4.682` in the airframe frame — a
  hair below the centreline and well aft, behind the rudder at `z = +3.03`,
  consistent with #709's rear-propeller reading and with the disc's own
  triangles spanning `z ∈ [4.514, 4.849]` (its `0.168 m` thickness);
* the disc is **2.506 m across** (`radius_m = 1.2529`) and **0.168 m thick**,
  i.e. `13.4 %` of its radius — flat enough to be a disc, which is what the
  `NotADisc` refusal exists to check;
* the pivot lies **on the disc**: within its own thickness of the plane all
  16 triangles lie in, and within `0.25 × radius` of the centre of the disc's
  own silhouette (the acceptance assertion of criterion 2).

## Cross-check that needs no installation

The same function measured over a synthetic disc authored by the test
(8 triangles, radius `0.5 m`, in the `z = 1.5` plane) returns axis
`[0, 0, 1]`, pivot `[≈0, ≈0, 1.5]` (float residue `< 1e-8`), radius `0.5 m`,
thickness `0` — so the rule reproduces an exactly known hub before it is
trusted on retail bytes.

## What is designed rather than measured

| | | claim |
| --- | --- | --- |
| hub axis **and pivot** | **measured**, from the disc's own triangles | — |
| spin **sense** (the normal's sign, hence which way it turns) | oriented aft, right-hand rule about it | `playtest-retail.propeller-spin-sense-is-designed` |
| spin **rate** | 1 rev/s at idle → 6 rev/s at full throttle, linear in the flight model's engine spool, `0` when the engine is stopped. No airframe record of `ZBD/planes.zbd` stores an rpm: the declared engine curve is idle thrust, maximum thrust and a throttle response rate (`1.5 /s`) | `playtest-retail.propeller-spin-rate-is-designed` |
| which propeller mesh shows when | only `staticprop1` is drawn and spun; no blur-disc swap rule is adopted | recorded in `propeller_spin.mesh_rule` |

Both claims and the rule appear in the startup `playtest sources` line and in
the smoke `report.json` under `propeller_spin`, next to the measured hub.

## Effects on other claims

`docs/PLAYTEST.md` no longer describes the propeller as static: it records the
spin, its measured hub, both designed claims and what stays unmeasured. The
stage remains `PROVISIONAL TUNING` and nothing here is `verified_original`.
