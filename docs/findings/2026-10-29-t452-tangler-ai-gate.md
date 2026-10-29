# T452-REG-TANGLER-AI-GATE: a tangled actor is not offered its nitro maneuvers

Task #2202. Static measurement only (Capstone over `$CS_ENGINE_IMAGE`, base
`0x400000`, image SHA-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`); no
original executable was run and nothing here is `verified_original`. Rows:
the `tangler_ai.*` `[[tangler_fact]]` entries in
`data/owner-static/engine-facts.toml`; test prefix `accept_t452_tangler_`
(`crates/cs_content/tests/accept_t452_tangler_ai_gate.rs`).

This closes the open question left by
`docs/findings/2026-10-28-t452-tangler-effect.md` ("What the AI path at
`0x4202e3` skips (`0x420695`)"). The answer is not a different action: the
branch drops one maneuver from the tick's candidate list and continues.

## The function the gate sits in

`0x4201a0` is an actor's **maneuver chooser** (`__thiscall`, `this` = the actor;
`ret 4`). It has three direct callers in the image, checked as a closed list by
the ignored image test:

| caller | argument |
| --- | --- |
| `0x41db9d` | `push -1` — the gated all-maneuver path |
| `0x4b9ff8` | `push -1` — the gated all-maneuver path |
| `0x43e10e` | `push eax` — one named maneuver, both gates bypassed |

With `-1` it loops `ebx = 0..0x10` (`0x4201d7`, `0x42069a`: `cmp ebx,0x11`), and
each index is one record of a global array:

* **Seventeen records of `0x1c` bytes at `0x71b210`** (`0x45f340`: `push
  0x71b210; push 0x1c; push 0x11` into the array constructor `0x5f6f5a`). The
  array is `.bss` (the file image carries neither `0x71b210` nor the guard byte
  at `0x71b3ec` that ends it), and it is filled from **`maneuvers.zrd`**
  (`0x6258c0`) by the constructors at `0x45f60c`..`0x45f718`, in this fixed
  order:

  `roll, rudder_turn, bank_turn, climb, dive, jinking, scissors,
  rolling_scissors, snap_roll, immelman, loop, split_s, lag_pursuit_roll,
  high_yo_yo, nitro_evade, barrel_roll, spiral_dive`

  at `0x71b210, 0x71b22c, … , 0x71b3d0`. The record is
  `{int +0 = natural_touch (default 0x63), byte +4 = relative,
  byte +5 = nitro, byte +6 = autogyro_allowed, float +8 = bias + 0.2 *
  natural_touch, byte/dword +0xc, vector<0x20> steps at +0x10..+0x1b}`
  (constructor `0x45f3e0`; `nitro` at `0x45f4ed`, `relative` at `0x45f4d7`,
  `autogyro_allowed` at `0x45f503`, the step list is parsed from `steps`).

Per index the chooser applies, in order:

1. `record+0 > actor+0x958` (the actor's skill level, clamped `0..9` by
   `0x47ce80..0x47ce92`) → skip (`0x42023d`).
2. only when `[actor+0x64]+0x21c` is non-zero: `record+6 == 0` → skip
   (`0x420221`).
3. the gate measured here (`0x4202ca`..`0x4202f0`).
4. otherwise it instantiates the maneuver — copies the record's step vector
   (`0x421f50`), appends the 16 bytes of actor state at `+0x150` (or a value
   derived from `+0x180` by `0x53def0`/`0x53f470`) into it (`0x45f7c0`),
   scores it, and inserts a 24-byte candidate `{int slot +0, vector<0x20> +4,
   float weight +0x14}` into the tick's candidate list (`0x4221e0`).

After the loop the chooser runs a **weighted random draw** over the candidates
that survived (`0x4206db`..`0x4207a5`: `rand()` at `0xa20350`, `fild`,
`fmul [0x603598]` = 1/32767.7, `fmul` the summed weights at entry `+0x14`, then
subtract each weight until the running value goes negative). The weight is
`record+8 + 1.0` (`0x4204ca`..`0x4204d7`), adjusted at `0x4204da`..`0x42050d`,
then `×0.1` for the actor's current maneuver `+0x9b4` (`0x6034a8`), `×0.2` for
the previous one `+0x9b8` (`0x6034fc`), `×6.0` for a slot listed in
`actor+0x994..+0x998` (`0x60369c`), floored at `0.1`. The winner becomes
`actor+0x9b4` (the old one moves to `+0x9b8`), its steps are copied to
`actor+0x9a0`, and **if the winner's own `+5` is set the chooser enables the
nitrous injector** (`0x420924`: `push 1; mov ecx, edi; call 0x4b2110`), the
setter `0x4b2110` that refuses while the tangle flag is set
(`tangler.boost_refuse`).

## The gate, exactly

```
0x4202ca  mov  cl, [edx*4 + 0x71b215]   ; maneuver record byte +5 = `nitro`
0x4202d1  test cl, cl
0x4202d3  je   0x4202f0                 ; not a nitro maneuver: build the candidate
0x4202d5  mov  cl, [esi + 0x946]        ; the actor's nitrous injector
0x4202db  test cl, cl
0x4202dd  je   0x420695                 ; no injector: skip the maneuver
0x4202e3  test byte [esi + 0x2dc], 2    ; the tangle status (tangler.status_store)
0x4202ea  jne  0x420695                 ; tangled: skip the maneuver
0x4202f0  …                             ; candidate setup
```

`0x420695` is the **loop tail**, not an action:

```
0x420695  mov  byte [ebp-4], 0          ; SEH state
0x420699  inc  ebx
0x42069a  cmp  ebx, 0x11
0x42069d  mov  [ebp-0x10], ebx
0x4206a0  jl   0x4201e1                 ; next maneuver
```

So the branch skips **the offer of one maneuver for this tick**, and the loop
carries on with the next record. `esi` is the same actor object whose status
word `+0x2dc` the tangler sets (flag 2) and whose timer `+0x2e0` counts down, so
the gate reads the tangle directly.

`+0x946` is named from the image's own strings: the `gimme`/`nitrous` cheat
branch (`0x43dd5f` compares `gimme` then `nitrous`, `0x43dd99` sets
`[actor]+0x946 = 1`) and the `procure`/`nitrous` branch (`0x43de2c`), both
printing `You now have the nitrous injector.` (`0x622d08`, `0x622d64`). The
aircraft-side loader also sets it when a byte of the equipment block is set
(`0x47d4e0`..`0x47d4f0`). The setter `0x4b2110` stores its outcome in
`actor+0x947` (`0x4b2163`), so `+0x946` is "the aircraft has the injector" and
`+0x947` "it is firing".

## What a tangled actor does instead

**Nothing distinct.** A tangled actor is treated exactly like an actor that
carries no nitrous injector: every maneuver whose `maneuvers.zrd` node declares
`nitro` is removed from this tick's candidate list before it is scored, and the
weighted draw renormalises over the maneuvers that remain. The actor therefore
starts (or continues) one of the **non-nitro** maneuvers; it does not receive a
substitute evasive action, and the runtime's existing "the force scalar is zero
while tangled" effect (`tangler.force_zero`) is the only flight consequence the
image expresses for it.

If every candidate is gated out the list is empty and `0x4206db` jumps to
`0x42092d`, skipping the whole selection: no maneuver is chosen, `actor+0x9b4`
(the current maneuver) and `actor+0x9a0` (its steps) keep their values, and
`actor+0x89c` — the timestamp the per-actor update at `0x41db86` compares
against — is not refreshed, so the next update calls the chooser again.

The gate is also coherent with the boost refusal already measured: the last
thing the chooser does with a nitro maneuver is enable the injector through
`0x4b2110(1)`, which a tangle refuses. Without the gate the AI would keep
picking maneuvers it cannot power.

**Bypass, recorded so it is not mistaken for a universal rule:** the
named-maneuver path (`0x43e10b`, the console command whose error text is
`Maneuver number must be between 0 and %d.` at `0x622df8`) reaches the setup
directly (`0x4201fc: cmp edi, -1; jne 0x4202f2`), so a forced maneuver ignores
the skill gate and the nitro/tangle gate alike. A forced nitro maneuver still
fails to enable the injector while tangled, because `0x4b2110` refuses on its
own.

## Not decoded

* The flight content of a maneuver (the `0x20`-byte step records and the
  per-step fields). `maneuvers.zrd` is packed inside the installation's ZBD
  archives and was not read here; no step is named.
* Which of the seventeen nodes carry `nitro`, and each one's `natural_touch`
  and `bias`. That is original data, not read here, so the finding claims the
  gate's rule and not a list of gated maneuvers.
* `record+4` (`relative`) selects between the actor's 16 bytes at `+0x150` and
  a value derived from `+0x180` (`0x420347`..`0x420382`). What those two are
  physically (orientation quaternion, reference matrix, …) is unmeasured.
* The cadence of the chooser. The `0x41db99` caller is guarded by
  `[0x64f66e] == 0`, `[esi+0xba] != 0` and `[0x71c470] >= [esi+0x89c]`; the
  meaning of those three is not decoded, so no selection period is claimed.
* Which actor kinds run the chooser. `+0x958` (a `0..9` skill level) and the
  cheat that sets `+0x946` on `[0x71c298]` are consistent with AI pilots, but
  the call sites were not attributed to an actor kind.

## Runtime: measured, not expressible yet

`cs_app::ai` has no maneuver chooser: the runtime AI is F31 navigation
(`ai::navigation`) and the F32 declared-to-runtime combat lowering
(`ai::combat`), and nothing selects among named maneuvers or enables a boost
from a choice. Applying the gate would mean inventing both the seventeen-record
catalog and the chooser, which is the "guess a layout" the contract forbids. So
this stage changes no AI code; what the gate protects is already expressed by
`OrdnanceSession::boost_allowed` (the tangle refuses a boost, #2186) and
`FlightState::boost_available`.

The limitation is named `t452_reg.limit.tangler_nitro_maneuvers`: *"which
maneuvers a tangled actor is not offered is measured, but the runtime has no
maneuver chooser to apply it to"*. Affected content: the seventeen
`maneuvers.zrd` maneuvers and the chooser `0x4201a0`. It is recorded here rather
than as a runtime record because no runtime record carries maneuvers.
Resolving task: **#2204** (`T452-REG-MANEUVER-CHOOSER`), which measures and
lowers the catalog and the chooser and then expresses this gate.

## Traceability

No evidence report: this stage needed only `CS_ENGINE_IMAGE` (static reading),
not a runtime capability, and it changes no runtime code. Every byte window in
the `tangler_ai.*` rows is re-read from the image by
`accept_t452_tangler_ai_gate_image_rows_match_the_engine_image`
(`#[ignore = "requires CS_ENGINE_IMAGE"]`, fails loudly without it), which also
re-checks the closed caller list of `0x4201a0` and the `maneuvers.zrd` /
`nitro` / console-hint strings; the non-ignored tests check the rows against
each other, including that the names row really encodes seventeen constructor
calls with seventeen consecutive record slots.
