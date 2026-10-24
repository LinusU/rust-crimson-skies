# #2039 Cabin: how the engine draws a pane with `scale`, `rotate` and `origin`

Date: 2026-10-24. Static analysis of `$CS_ENGINE_IMAGE` (read-only, never committed; digest `43540fc9…`),
disassembled with `llvm-objdump` into `private/`. The original executable did not run: every claim is `static` and
nothing is `verified_original`. Implementer `sonnet-2`; no independent review yet. Addresses are virtual addresses.
Tests: `accept_parity5_cabin_memento_pane_transform_` in
`crates/cs_app/tests/ui/parity5_cabin_memento_pane_transform.rs` (the image test needs `CS_ENGINE_IMAGE`); model:
`crates/cs_app/src/ui/front_end/pane_transform.rs`.

## Where the statements are

* The statement keywords are a 95-name hash table of name pointers at `0x6402a8` (the pointer table at `0x6402d8`
  in the task is the same table from index 12). Index: `scale` 43 (`0x640354`), `rotate` 44, `origin` 45, `color` 46,
  `alphamode` 47, `render` 48.
* The statement dispatcher (`0x5d3d5e`) lexes the keyword, subtracts two and jumps through the 111-entry table at
  `0x5d9ad0`. `scale` → `0x5d94fd`, `rotate` → `0x5d96b1`, `origin` → `0x5d9741`, `color` → `0x5d97a5`.
  (The 50-entry table at `0x5e3b60` is the *expression* functions, a different keyword list.)

## Units and storage

| statement | handler | stored |
| --- | --- | --- |
| `scale(p) = a, b, c` | `0x5d94fd` → setter `0x5e4e40` | `a * 0.01`, `b * 0.01` as floats at pane `+0x10`, `+0x14` (`0x6032d8` = `0.01f`). `c` is parsed and dropped: **percent**, x and y only. |
| `rotate(p) = a, b, c` | `0x5d96b1` → setter `0x5e4e20` | each `v * (1/360) * 6.28` (`0x60b894` = `1/360`, `0x60b890` = `6.28f`, not tau) at `+0x30` (x), `+0x34` (y), `+0x38` (z): **degrees** → radians. |
| `origin(p) = a, b, c` | `0x5d9741` → setter `0x5e4e60` | three integers as floats at `+0x18`, `+0x1c`, `+0x20`. |

A pane built from a file (`0x5e41e0`) starts with scale 1.0, 1.0, origin 0, 0, 0 and rotation 0, 0, 0. Its
picture is cut into tiles of at most 256x128 whose corners are the picture's own pixels from 0,0 (`0x5e5190`'s
callers `0x5e5739`, `0x5e5aee`, `0x5e5bbb`), so the local origin is the picture's top left.

## The draw

`render p, x, y` (`0x5d9a0d`) calls the pane draw `0x5e4710` with the position as two stack floats. For each
element and each of its four corners (8-dword vertices at element `+0`, `+0x20`, `+0x40`, `+0x60`):

1. `corner.xy = (corner.xy - origin.xy) * scale.xy`, `corner.z -= origin.z` (`0x5e486c` onwards);
2. rotated by the matrix built from x (`0x5ea9e0`), y (`0x5eaa00`), z (`0x5eaa20`) rotations, skipped when all three
   angles are 0 (`0x5e4978` onwards);
3. `w = 1 / (z + 277.0)` (`0x60ee6c`), `xy *= w * 277.0` (`0x5e4bdd`–`0x5e4cc7`): the identity while z is 0;
4. the position is added (`0x5e4cce`–`0x5e4d2d`).

So **the origin is both the pivot and the point of the picture that lands on the render position**; with origin
0 the picture's top left is that point and the picture turns about it. The z rotation matrix (`0x5eaa20`: `[0]=cos`,
`[1]=-sin`, `[4]=sin`, `[5]=cos`) with the point transform (`0x5eaa40`) is `x' = x cos + y sin`,
`y' = -x sin + y cos`: on a y-down screen a positive angle turns the picture **counter-clockwise**.

## The cabin's `PC_MEMENTO`

`ZJ` (`CTL.SCRIPT`) sets `scale(YH) = GK, HK, IK` and `rotate(YH) = 0, 0, JK` in message 10009 and draws
`render YH, AI.x, AI.y` with `AI` = the layout position (`PC_MEMENTO =P,,169,325,…`). `PASSENGERCABIN.SCRIPT` sets
`GK = HK = 20`, `IK = 1`, `JK = 3`; the layout `P` row has no scale column. So the 362x460 picture is drawn at
**20 % = 72.4 x 92**, **3 degrees counter-clockwise (0.052333 rad)** about its top left at **169,325**:

| corner | screen |
| --- | --- |
| top left | 169.00, 325.00 |
| top right | 241.30, 321.21 |
| bottom left | 173.81, 416.87 |
| bottom right | 246.11, 413.09 |

Bounding box 169.0–246.1 x 321.2–416.9. The scale answers the open question of #2035 (a size, not a pixel size);
the 3 degrees is a real turn, not a no-op.

## Unknown, recorded rather than guessed

* **Agreement with the measured key patch.** The patch of `PC_Mementopicframe.png` (169,317, 93x115) was measured
  at about 71x92 at about 180,330 on the project's cabin shot (`2026-10-24-cabin-change-memento.md`). The size
  matches the decoded 72.4x92; the position does not: the decoded top left is 11 px left and 5 px above the
  patch's. Not explained: the measurement is of a screenshot not on the agent machines
  (`private/ref-drydock/cabin/original.png`); the layout row's `X,Y` might be offset by the parent or by `relative`
  (`CTL.SCRIPT` sets `AI` from `location` only when `!relative`); the half-pixel bias below
  is 0.5 px, far too small. Affected content: where #2040 puts the picture. #2040 should compare the decoded box
  with the owner capture before choosing; this task does not settle it.
* **Layout scale override.** `gui_init` replaces `GK`/`HK` when `$$PK$$ || $$QK$$` from the layout callback are set;
  which layout column those are was not decoded. The `P` format has no scale column, so they are presumed 0 for
  `PC_MEMENTO`. Affected content: any pane whose row sets them.
* **x and y rotations.** Their matrices exist (`0x5ea9e0`, `0x5eaa00`) but the composition order at
  `0x5e49be`–`0x5e49f2` was not decoded; the model returns `None` for them. The cabin uses z only.
* **Half-pixel bias.** The draw subtracts `0.5` from the render position when `0x5ce440(2, 0, 0, 0)` returns 0
  (`0x5e47d7`–`0x5e47f7`); what that query asks is not decoded. Not applied by the model.
* **Perspective with z.** Modelled only as the identity (z stays 0 for z rotation and 2D elements).
