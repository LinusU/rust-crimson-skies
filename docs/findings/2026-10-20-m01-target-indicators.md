# M01 target indicators: the original's selection commands, marker state machine, label and gate

Rally #1727 `PARITY2-M01-TARGET-INDICATORS`. Task: one selected target at a
time; the bracketed marker when it is in view, the edge arrow with its clock
label when it is not; the `Q`/`W`/`E`/`R`/`T` selection controls. This
records what the decrypted engine image (`$CS_ENGINE_IMAGE`) measures so the
implementation does not re-derive it. All addresses are virtual addresses in
the decrypted image; provenance is `static` everywhere — none of this is
`verified_original` behaviour.

Owner playback notes (the captures the implementation is checked against)
are in the task history: an on-screen target draws only the bracketed marker
(`[Examine] - Site #1`, `Kestrel`); an off-screen target draws the
edge-clamped arrow and the `Name N o'clock` label — blue ring included while
the spyglass gate says the window is open — and dead-astern (`6 o'clock`)
puts the arrow at the **top** edge.

## The selection command table (ids 36–46)

The original's input command table carries eleven targeting commands, all
delivering through one selection path that plays `sg_switchtarget`
(handlers `0x488737`–`0x488cd3`). The measured default key rows are the
F22-J table (`docs/findings/2026-10-14-f22-j-original-default-control-bindings.md`);
the modifier matching is exact — the packed scancode+modifier code must name
a row, so `Shift+Q` and `Shift+Ctrl+E` fire nothing.

| id | command | key | implementation shape |
|----|---------|-----|----------------------|
| 36 | next enemy/objective | `E` | `Cycle{Next, HostileOrObjective}` |
| 37 | previous enemy/objective | `Shift+E` | `Cycle{Previous, HostileOrObjective}` |
| 38 | nearest enemy/objective | `Ctrl+E` | `Nearest{HostileOrObjective}` |
| 39 | next ally | `W` | `Cycle{Next, Allied}` |
| 40 | previous ally | `Shift+W` | `Cycle{Previous, Allied}` |
| 41 | nearest ally | `Ctrl+W` | `Nearest{Allied}` |
| 42 | next ground/non-aircraft | `R` | `Cycle{Next, NotClass(Aircraft)}` |
| 43 | previous ground/non-aircraft | `Shift+R` | `Cycle{Previous, NotClass(Aircraft)}` |
| 44 | nearest ground/non-aircraft | `Ctrl+R` | `Nearest{NotClass(Aircraft)}` |
| 45 | target under reticule | `Q` | `UnderCrosshair` |
| 46 | target nothing | `T` | `Clear` |

"Enemy/objective" is the class the original's controls screen names and the
owner observed: `E` cycles the objective sites as well as hostile aircraft.
The ally class is aircraft only (`+0x67c ∈ {0,4}` on vehicle records marks
the aircraft kinds; eligibility code `0x4b5c00`–`0x4b6100`). The cycle
walker `0x4b6490` walks one flat target array with `±1` steps and
wrap-around; `0x4b6480` (a `return true` stub in the measured build) is the
per-entry validity hook. Each handler first sets the class-filter flag bytes
`0x71c478`–`0x71c47b`; a class change clears the current selection.

## The indicator state machine (`0x49d940`)

The per-frame update decides between two drawn states from the target's
projection:

* **In view** (projection lands inside the viewport): the marker only. The
  off-screen arrow triangle degenerates to a point because the arrow's base
  anchor equals the apex — it is drawn but invisible. No label, no ring.
* **Off view**: the arrow is a three-vertex triangle with its apex at the
  edge-clamped point and base offset toward screen centre; the label panel
  anchors at `esi+0x200/0x204`; the ring (the spyglass window border) draws
  when the distance gate opens.

The label is the `0x6ebac0` widget set built by `0x4579e0` (called from
`0x49ff5b`): `targetHelp` gets the verb line from
`'%s [%s] -' / '%s -' / '[%s] -' / '%s'` over the `help_label` verb and the
title, `targetTitle` gets `record+0x14` raw, and `targetPos` gets the clock
text generated at `0x49e228`–`0x49e264` via string id `0x84` and helper
`0x49d8a0`.

### The clock label (`0x49d8a0`)

`hour = floor(wrapped(cam_yaw − target_yaw) * 6/π + 0.5)`, with `0` remapped
to `12` — i.e. `round(relative_bearing_deg / 30)`, the nearest clock hour.
The existing `cockpit_hud::clock_hour` formula is the same computation.

### The marker color (`0x4a5f40`)

* target record `+0x48` (node type) ∈ `{8002, 8006, 8009, 8077}` → red
  `(200,0,0)`;
* other node targets → blue `(0,0,255)`;
* node-less targets → side comparison: different sides red, same or neutral
  green.

### The window gate and hysteresis

The spyglass window opens by distance, not view cone:

```
threshold = min(2000 m, near + 0.8 * (far - near))
if no window currently exists: threshold *= 0.875   // hysteresis, 0x49dd13
```

Constants read at the site: `2000.0` (`0x44fa0000`), `0.8`, `0.875`, plus
the viewport clamps `π/2`, `0.0262`, `15.0`, `0.5`, `1.001`, `3.0` — the
`0.5 + 15 ≈ 15.5` border the ring draws at. When the gate is open the ring
is the window's border; the window's render-to-texture content itself is
PARITY2-M01-SPYGLASS (#1734), which reads the same predicate.

## What the implementation claims

`verified_original`: none. `measured/static`: the table above, the colour
rule, the clock formula, the gate constants, the state split (marker-only
in view; arrow + label + optional ring off view). `designed`: the arrow's
drawn glyph (a directional text stand-in for the original's triangle, aimed
outward along the edge it anchors to) and exact label offsets, the ring's
pixel border width, the crosshair cone (`π/18`, the mission's declared
value — the original's acceptance angle is unmeasured) and the name
fallback chain (airframe table → record name → subject tail → node). The
colour rule's red set is four node ids whose classes are unmeasured, so the
build lets the reticle's declared-hostile flag stand in for it, and the
measured node-less green side-compare branch has no expression: every
registerable roster class is a vehicle node. `unknown`: the original's
under-reticule cone, the arrow triangle's exact pixel geometry, which
classes the four red-set node ids name, and which targets receive
`help_label` verbs outside `targets.zrd` rows.
