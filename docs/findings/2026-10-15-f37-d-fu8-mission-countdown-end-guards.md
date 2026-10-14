# F37-D-FU8: the mission countdown's end-path guards, measured

* Task: Rally #740 (`F37-D-FU8`), follow-up to #737 (`F37-D-FU6`).
* Date: 2026-10-15. Method: static analysis (radare2, `objdump`-style byte
  reads) of the owner-supplied decrypted executable at
  `/Users/linus/coding/rust-crimson-skies/crimson.decrypted.exe`, image
  sha256
  `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`.
  No original run was used or needed. Claim status: `inferred`, never
  `verified_original`; every address below is a virtual address in that
  image (file offset = VA − 0x400000 for `.text`, `.rdata` and `.data`
  below VA 0x643000).

## 1. What was open

`f37.d.limit.mission_countdown_end_guards` recorded two guards of the
original's countdown-expiry end path that F37-D-FU6's producer did not
model, because their conditions could not be sourced:

1. `0x440ad0()`'s global game-state byte, read between the poll and the
   end call — whose *meaning* was unknown: which game states suppress the
   end path and which allow it.
2. The `remaining < -1.0f` skip — the end path refuses a timer already
   that far past zero, and it was unclear whether and when that can be
   reached.

Both are now measured. The byte is the registered game option named
**`Network`** — zero in a single-player game, non-zero in a network
game — and the skip is an exact `<= -1.0f` stale-timer guard that is
reachable in the original only while some *other* guard has been
suppressing the end path frame after frame.

## 2. `0x440ad0()` — the byte is the `Network` game option

```
0x440ad0  a1 50 f7 64 00   mov eax, dword [0x64f750]
0x440ad5  8b 00            mov eax, dword [eax]
0x440ad7  c3               ret
```

`[0x64f750]` is a pointer to a 0x20-byte **named-option node** returned
(find-or-create by name) from `0x5bde90(src, arg_ch, arg_10h, arg_14h)`
during game init, in `fcn.0043fb50`:

```
0x440234  push 2 / push 0 / push 0 / push 0x6237f4   ; "Network"
0x44023d  call 0x5bde90                              ; find-or-create
0x440247  mov [0x64f750], eax                        ; the handle
0x44024e  push 0 ; call 0x440ab0                     ; default the value to 0
```

`0x5bde90` looks the name up through `0x5be790` and returns the existing
node when found; otherwise it `malloc`s 0x20 bytes, `strdup`s the name at
`[node+0x10]`, stores the type at `[node+8]` (switched on; type 0 sets
`[node+0xc] = 4`), `arg_10h` at `[node+0xc]` and `arg_14h` at
`[node+0x14]`. The value cell is **`[node+0]`**: the sibling helpers make
the layout unambiguous —

* `0x440ab0(v)`: `[[0x64f750]] = v` — the option's **writer**;
* `0x440ad0()`: returns `[[0x64f750]]` — the option's **reader**;
* `0x440ac0(v)`: `[[0x64f754]] = v` — the writer of the option registered
  four instructions later under the name `"NetListen"` (0x6237fc), with
  its own global at `0x64f754`;
* the same init block registers `"JoystickNumButtons"` (0x6237e0) at
  `0x64f744` and one more name at `0x64f740` — the same named-option
  family.

A raw scan of the image for the bytes `50 f7 64 00` finds exactly **three**
occurrences — the init store at `0x440247`, the writer `0x440ab4` and the
reader `0x440ad1` — so **every** code path that changes the option goes
through `0x440ab0`. Its five callers, with the values they write:

| site | function | value | measured context |
| --- | --- | --- | --- |
| `0x44024f` | `fcn.0043fb50` (game init) | 0 | the default, right after registration |
| `0x440428` | `fcn.0043fb50` (init tail) | 0 | after the config/init calls |
| `0x440462` | `fcn.00440460` | 0 | only caller `0x4a7a14`, in window teardown near `UnregisterClassA` (0x4a79e2); then `jmp 0x5bdc89` |
| `0x496d05` | `fcn.00496cf0` | 0 | guarded `if (0x440ad0() != 0)`: teardown `0x4966c0` + `0x5b40f0`, then clear |
| `0x4a8325` | `fcn.004a8270` tail | `arg & 0xff` | the session start/stop path, below |

`fcn.004a8270(arg)` is the network-session toggle. Measured:

* the start path (arg != 0, entered with `push 1` by its callers
  `0x4128d4` and `0x412967`): if the option already reads non-zero it
  only rewrites the same value; else it clears `[obj+0x4fc]`/`[obj+0x500]`,
  establishes the session through `0x5b4200`/`0x5b4ae0`/`0x5b24a0`/
  `0x5b34b0` and, on every branch, writes `1` through `0x440ab0`;
* the stop path (arg == 0, the callers at `0x412b24`, `0x4197d9`,
  `0x4a1795`, `0x4a79ec`): if the option reads zero it only rewrites zero;
  else it runs the teardown `0x5b40f0`, clears `[obj+0x504]` and writes
  `0`.

So through every in-image writer the option's value domain is exactly
`{0, 1}`: **0 = not a network game, 1 = network game**. Corroboration
outside the code: the name string itself (`.data` 0x6237f4, `Network`),
the multiplayer UI strings `Network Notice` / `Network Error`
(0x63db08 / 0x63db38), and the two consumers that had already recorded
this byte — the zeppelin-tick tails' DirectPlay-session predicate
`0x5b4210` (the M01 zeppelin-attitude finding) and the motion-integrator
gate at `0x4bf9f3`. **Cross-finding consequence:** that integrator gate
reads the `Network` option non-zero, i.e. the spawn-attitude rewrite path
#770/#814 left open runs only in a network game; on a single-player
launch the spawn attitude cannot be rewritten by that integrator.

**Unknown, recorded:** `0x5bde90`'s node is also reachable by name from
the registry's own parse path (the console/config writer), which was not
traced here; a config that spelled `Network 1` would land in the same
cell. Nothing in the image writes any value but 0 and 1 through the
option's typed writer, and no shipped file in the installation root
spells it.

## 3. What gates the countdown poll (and a correction)

The task description said the expiry branch reads the byte *before* it
polls the countdown. The measured order inside `CZMission::Update`
(`0x46a490`) is the other way round — the poll runs first, the byte gates
the *end path*:

```
0x46a4e2  call 0x46c640             ; the poll: expiry iff running && [+4] <= 0.0
0x46a4e7  test eax, eax
0x46a4e9  je  0x46a586              ; no expiry → past the whole block
0x46a4ef  call 0x440ad0             ; the Network option
0x46a4f4  test eax, eax
0x46a4f6  jne 0x46a586              ; network game → skip the end path
0x46a501  call 0x46c580             ; read [+4] (stored, compared below)
0x46a50b  call 0x463c00             ; the mission's ended flag [+0xc54]
0x46a512  jne 0x46a586              ; already ended → skip
0x46a517  fcomp dword [0x6034e8]    ; -1.0f
0x46a51f  test ah, 0x41
0x46a522  jne 0x46a586              ; stale → skip
0x46a52d  call 0x463c30             ; end the mission (1, 3.0)
            … messages 0x1772, then 0x89 gated by
               0x440ad0() == 0 && 0x4639b0() == 0 (0x46a54d) …
0x46a581  call 0x46c5c0             ; stop the timer
```

**The gate: the end path runs only while the `Network` option reads
zero** — i.e. only in a single-player game; a network game's poll reports
expiry and the mission runs on. Two sibling sites agree: mission load
stops the timer only when the option reads zero (`0x4646b0`, skip over
`0x46c5c0`), and the session teardown at `0x496cf0` clears the option
when a network session ends. This is exactly the session-constant
`network_game` flag the recreation has carried in
`MissionCountdown::preempts` since F37-D-FU4; its meaning is no longer
unknown, and the value that sources it is measured.

## 4. The `remaining < -1.0f` skip

The constant at `0x6034e8` is `0xbf800000` = **-1.0f**. The compare idiom
(`fcomp`, `fnstsw ax`, `test ah, 0x41`, `jne`) tests the x87 C0 (below)
and C3 (equal) bits, so the exact guard is:

* the end path runs iff `remaining > -1.0f` — and skips iff
  `remaining <= -1.0f` (the earlier findings' "`remaining < -1.0f`"
  shorthand is imprecise: **exactly -1.0 s is already too stale**);
* an unordered compare sets the same bits, so NaN skips as well.

**Whether and when it can be reached, in the original:**

* *Not on a first expiry in single-player.* The decrement is the frame
  dt, clamped at 0.125 s (0.25 s under the 2x speed-up), so the first
  poll that reports expiry always finds `remaining` in `(-1.0, 0.0]` and
  the guard always lets the end through.
* *Yes, under suppression* — whenever the other two guards (network
  game, already-ended mission) have been skipping the end path while the
  timer keeps running (nothing else stops it; `0x463c30` does not):
  the poll keeps reporting every frame and `[+4]` falls without bound.
  The guard is what keeps such a **stale** timer from ending the mission
  later — e.g. after the session teardown at `0x496cf0` drops the
  `Network` option back to 0 with a long-expired mission timer still in
  `[+4]`.

**In this runtime, before this task:** the producer reported expiry on
the first tick `remaining <= 0.0` and the consumer ended the mission on
that same report, so the skip was unreachable — but it was *not*
unreachable in general: a spelled `TIMER_ADJUST` whose `SET` (or `ADJUST`
add) takes `[+4]` past -1.0 in one step makes the next tick report an
expiry the original's stale guard would refuse, and the recreation ended
the mission there — a reachable divergence on mission data that spells
such a directive. `TickRate` accepts any positive tick count, so the
producer's own decrements (dt <= 1.0 s) always land the first report in
the window; the directive path was the gap.

## 5. What changed

* `crates/cs_script/src/runtime.rs`:
  * `MissionCountdown` gains `remaining_seconds: f64` — `[+4]` as the
    end path reads it after the poll — and `preempts()` gains the
    measured stale-timer guard (`> -1.0`, below-or-equal skipping,
    unordered skipping); `network_game`'s doc now names the option and
    its sites; the struct drops `Eq` (an f64 field), keeping `PartialEq`.
  * `f37.d.limit.mission_countdown_end_guards` is **closed** — both
    guards it recorded are measured and modelled — and the two facts that
    listed it (`countdown_preempts`, `per_tick_order`) no longer do. The
    `countdown_preempts` fact's statement and addresses now carry both
    gates, and the rule's evidence names this findings entry.
  * What stays open is unchanged and still recorded:
    `f37.d.limit.mission_countdown_tick_dt` (the wall-clock dt
    divergence) and `f37.d.limit.mission_countdown_spec_sourcing` (no
    source yet for `MISSION_TIMER`'s record field or for a networked
    session's mode — the `Network` option's sourcing belongs with the
    F56 mode line and VS-M01-RUNTIME #359).
* `crates/cs_sim/src/mission.rs`: the producer carries `[+4]` in the
  input it feeds `MissionState::step_with_countdown` (post-NOLOSS-clamp,
  as the end path would read it); the poll's observation, the clamp and
  the stop-on-preempt behaviour are unchanged.
* `crates/cs_sim/tests/accept_f37_d_fu6_mission_countdown_producer.rs`:
  the input-literal expectations carry the remainder; the closure test's
  residual set shrinks to the two limitations this task did not close.
* `crates/cs_script/tests/accept_f37_d_fu8_countdown_end_guards.rs`
  (new) and `crates/cs_sim/tests/accept_f37_d_fu8_countdown_stale_guard.rs`
  (new): the `accept_f37_d_fu8_*` pins — the guard's exact boundary, the
  stale-reachability case (a negative `TIMER_ADJUST` SET leaves the
  mission running, where the unguarded runtime failed it), the unchanged
  in-window path at 64 Hz and at 1 tick/s, the carried remainder, and
  the machine-readable closure (the entry may not come back; the fact
  carries `0x440ad0`, `0x46a4ef`, `0x46a517`, `0x6034e8` and the
  `Network` name).

## 6. Residues, affected content and resolving work

| residue | affected content | resolves in |
| --- | --- | --- |
| who sources the `Network` option into a real session here (the option is measured; nothing in this tree writes it — the input flag is caller-declared) | any networked mission session's countdown expiry, and the motion-integrator gate at `0x4bf9f3` | the F56 mode line and VS-M01-RUNTIME (#359); `f37.d.limit.mission_countdown_spec_sourcing` records it |
| the registry's name-based write path to the option (console/config parse, not traced) | whether a mid-mission config change could flip the gate in the original | F38's native-binding line, whenever the option is bound rather than declared |
| `0x4639b0()`'s `[mission+0x700] == 3` phase predicate (the second expiry message's gate) | which mission phase suppresses message id 137 after a timeout | unchanged pre-existing unknown (M01-LC directive-D findings) |
| the text behind localized ids 6002 / 137 | the timeout messages' wording | unchanged pre-existing unknown (M01-LC directive-D findings, F39-E1) |
| no original run | every claim here is static code evidence: `inferred`, never `verified_original` | owner-run evidence only |

Nothing here is `verified_original`, and no test or evidence record was
weakened to close the limitation: the entry is gone only because the
behaviour it recorded is implemented, pinned by the `accept_f37_d_fu8_*`
tests.

## 7. Checks

Recorded in the handover summary for Rally #740: `cargo fmt --check`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`cargo test --workspace`, and the task-prefix selection
`cargo test --workspace -- accept_f37_d_ --include-ignored`, all green on
the pushed head.
