# #1981 M01 sea: the unit of the `wtr` cycle rate word is frames per second

Date: 2026-10-24. Static analysis of `$CS_ENGINE_IMAGE` (read-only, never committed), disassembled with
`llvm-objdump` into `private/`; the original executable did not run, so every claim is `static` and nothing is
`verified_original`. Implementer: `claude-2` (Sonnet 5.5); no independent review yet. Addresses are virtual addresses
in the image. Committed facts: the three `[[material_cycle]]` rows of `data/owner-static/engine-facts.toml`; the ignored
test `accept_m01_sea_cycle_rows_and_instructions_match_the_image` re-reads them and the instruction bytes below.

## The consumer

`0x55b1a0(material)` is the per-draw update of a cycled material. Its caller sites are the polygon-group draw at
`0x552eac` (guarded by flag bit `0x4` of the group's flag word, `0x552ea6`) and `0x554f71`. In-memory cycle record
(the stored 28-byte `CycleInfoC` plus the frames, as `material+0x24`):

| offset | meaning |
| --- | --- |
| `+0x0` | stored `unk00`; zero means "stop on the last frame", non-zero loops (`0x55b213`) |
| `+0x4` | frame stamp: last frame counter updated at (`0x9be6f8`), so one update per frame (`0x55b1dd`) |
| `+0x8` | the running position, f32 (frames) |
| `+0xc` | the stored `unk12`: the rate |
| `+0x10` | frame count |
| `+0x18` | table of texture indices |

Per update: `frame = table[ftol(position) % count]` is stored as the material's texture index (`0x55b1ec..0x55b1fe`);
then `position += [0x9ad744] * rate` (`0x55b201..0x55b20d`); a zero `+0x0` clamps the position to `count - 1`
(`0x55b213..`); a negative position gets `count * ftol(|rate|)` added (`0x55b23c..0x55b265`).

## The unit

`[0x9ad744]` is the **frame delta in seconds**: `0x59c0c0` reads a millisecond timer difference, multiplies it by
`0.001f` (`0x6075dc`, at `0x59c0aa`), by a time scale `[0x63b16c]` = 1.0 (`0x59c11b`), stores it (`0x59c121`) and clamps
it to `0.0 .. 0.125` (`0x63b15c`, `0x63b158`; `0x59c129..0x59c15f`). So the rate word is **frames per second** (M01's
water: 16 frames at 10.0, a 1.6 s loop). A second writer of `0x9ad744` exists at `0x492afe` (a simulation path, not
traced); the timer path is the render-frame one.

Not traced: the writers of `[0x63b16c]` (read as a constant 1.0), whether `0x492afe` replaces the delta during the
mission (the sea would then advance by that step), and whether the timer is `timeGetTime` (the import at `0xa1fd3c` is
named by the 0.001 factor only).

## Changed

* `render/material_cycle.rs`: the cycle rule (`cycle_delta`, `cycle_frame`, `cycle_advance`) and the system that sets
  each cycled material's base colour texture every frame; `TextureBinder` registers every cycled material whose rate is
  positive and whose frames all resolve (a rate of 0 keeps the first frame, as the engine's update would); the world draw's shaded and sky copies, and the horizon sea's copy, follow
  the original material's cycle (255 water parts, the horizon plane).
* Retail member: M01's water material 220 is a 16-frame cycle at rate 10.0.

## Unmet

The owner-command smoke (AGENTS rule 6) could not be produced on this host: every framebuffer (briefing, scene, HUD)
came back black (26 of 71 checks fail, `frame luminance levels Some(1)`) **with and without this change** (the
unmodified `origin/main` build fails the same 26), so the display is not drawing in this session (screen asleep or
locked). The frames of the cycle (`wtr00001..15` moving at 10 frames/s) therefore are not cited here; a reviewer with a
live display must run the smoke and compare two scene frames about 0.1 s apart.
