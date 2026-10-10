# #1155: a team-less zeppelin record carries the allegiance its loader resolved

Date: 2026-10-09. Task `M01-LC-ZEPPELIN-ALLEGIANCE` (#1155), filed by #359
(`VS-M01-RUNTIME`). Feature sheet: `specs/F34-*.md` (the world-actor schema
and its runtime stages). Capabilities used: **`retail`** (read-only
`$CS_GAME_DIR`) and **static analysis** of the owner-supplied decrypted image
(sha-256 `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`,
image base `0x400000`, file offset = VA − `0x400000`), disassembled with the
platform `objdump -d -M intel` and re-read at every address below. **No
original run happened: nothing here is `verified_original`.** Test prefix:
`accept_m01_lc_zeppelin_allegiance_`.

## The answer in three paragraphs

**Outcome 1 was measured: the original assigns an allegiance from
somewhere.** A `zeppelins.zrd` record's loader does not treat a missing
`team` as "no faction". While the record loads, it calls its scene-graph
resolver (`0x4bd96a`: `call 0x4bf080` → `0x4bef90`) on the node the record's
`node` key resolved (`this+0x1c`) and stores the returned team int at
`zep+0xe0` (`0x4bd977`). The resolver walks the node's subtree: a staged
`+0x8d` record for the node (table `0x71d338`), then the turret binding
table `0x71d910` (lookup `0x4a9890`), then the children depth-first in
stored order, first nonzero result wins, else `0` (`0x4bef90`'s own body,
terminal zero written by `0x4a3f80`). A record that *does* state `team`
overrides that value outright when the spelling is recognized
(`0x4bd987`..`0x4bda5e`); an unrecognized spelling keeps the resolved value
(`0x4bda1d`).

**The vocabulary is the loader's own.** The `team` key (`0x62b6bc`) is
compared against three strings: `enemy` (`0x62b6c4`) → `0x45c260(0, &t)`
whose `lea ecx, [eax+2]` writes **2**; `ally` (`0x62b6cc`) → `0x4830c0(&t)`
which writes **1**; `neutral` (`0x62b6d4`) → `0x4a3f80(&t)` which writes
**0**. The same three ints name the same three words on the resolver side:
the faction bound for a record is the vocabulary's own spelling,
`neutral`/`ally`/`enemy` for `0`/`1`/`2`. An int inside the engine's write
but outside `0..=2` has no measured spelling and is never guessed into a
faction; it stays refused under `f34-world.zeppelin-allegiance-open`.

**M01's three team-less records resolve through the turret table.** The
mission start loads the shared `ai.zrd` turret table at `0x465596`
(`call 0x4ac170`) *before* it loads `zeppelins.zrd` (`0x4655c0`), and the
`+0x8d` stager `0x4a2e00` runs between them (`0x4655a7`). The table's
per-record parse reads `TEAM` (`0x629cf8`) at `0x4aa5b7` — stored verbatim
via `0x453740` — and, absent the key, `0x4aa5ee` calls `0x45c260(0, …)` so
the default reads as **2**. Over the retail data (`zbd/zrdr.zbd`'s
`ai.zrd`, 42 `TURRET` records, against `zbd/c1c/gamez.zbd`'s nodes):

| record | resolution | source |
| --- | --- | --- |
| `piratezep` | team **1** → `ally` | `TURRET` record 38 (`TEAM = [1]`), `NODES` entry `ctur*` binding `ctur1` |
| `workersvoyagezep` | team **2** → `enemy` | `TURRET` record 31 (no `TEAM` → the default 2), `NODES` entry `utur*` binding `utur1` |
| `blackswanzep` | team **2** → `enemy` | `TURRET` record 28 (no `TEAM` → the default 2), `NODES` entry `ctur*` binding `ctur2` |

The group archive's `targets.zrd` (`zbd/c1c/zrdr.zbd`, 1 020 bytes) cannot
gate any of them: its five `nodes` elements (`["piratezep","rock_zeppelin"]`,
`["workersvoyagezep"]`, `["wv_tailhook","peoplehook"]`,
`["blackswanzep"]`, `["pzhookpoint"]`) match no allegiance-marked node
(bit 31 of `unk040`) inside the three subtrees, so `0x4a2e00` stages no
`+0x8d` record the resolver could return before the turret table.
**Corrected by #1177 below: the `targets.zrd` match never gated staging —
the mark bit alone does — and all three subtrees *do* hold marked nodes.
The values above stand; only `workersvoyagezep`'s source changes, to the
staged `panels` record whose lane reads the same 2.**

## Address table (re-read this session)

| address | instruction | meaning |
| --- | --- | --- |
| `0x4bd963`..`0x4bd96a` | `lea ecx,[esp+0x30]; push ecx; mov ecx,ebx; call 0x4bf080` | the record loader resolves the allegiance for `this+0x1c`'s subtree |
| `0x4bd96f`/`0x4bd977` | `mov edx,[eax]; mov [ebx+0xe0],edx` | the result lands at `zep+0xe0` |
| `0x4bd97d` | key `team` (`0x62b6bc`) lookup; absent → `0x4bda64` (`position`) | the key is optional |
| `0x4bd99c` | first child tag 3 (text) → spelling branch; tag 1 (int) → `0x453740` verbatim store (`0x4bda3d`..`0x4bda5e`) | `team` may also be an int |
| `0x4bd9b1`..`0x4bd9d2` | `strcmp("enemy")` → `0x45c260(0,&t)` (`lea ecx,[eax+2]`) | `enemy` → 2 |
| `0x4bd9dd`..`0x4bda08` | `strcmp("ally")` → `0x4830c0(&t)` (`mov dword[eax],1`) | `ally` → 1 |
| `0x4bda10`..`0x4bda3b` | `strcmp("neutral")` → `0x4a3f80(&t)` (`mov dword[eax],0`) | `neutral` → 0 |
| `0x4bda1d` | `jne 0x4bda64` | an unrecognized spelling keeps the resolved value |
| `0x4bef90` | `0x4a2850(0x71d338,node)` with byte `[rec+0x8d]` → `[rec+8]`; else `0x4a9890(0x71d910,node)` → `[rec+8]`; else children (`[node+0x56]` count, `[node+0x5c]` array) recursive, first nonzero wins; else `0x4a3f80` → 0 | the resolver |
| `0x4bf080` | wrapper: node at `this+0x1c`, out on the stack, `ret 4` | called from `0x4bd96a` |
| `0x465596` / `0x4655a7` / `0x4655c0` | `call 0x4ac170` / `call 0x4a2e00` / `call 0x4bd110` | start order: turrets, `+0x8d` stager, zeppelins |
| `0x4aa5b7`..`0x4aa60b` | `TEAM` (`0x629cf8`) → `0x453740` verbatim; absent → `0x45c260(0,&t)` = 2 | the turret record's `TEAM` parse and its default |
| `0x629dac`/`0x629dc8`/`0x629dd0` | `TURRET` / `NODES` / `Turret entry #%d specifies no NODES` (`D:\zipper\Crimson\turret.cpp`) | the member's vocabulary, anchored by the assertion string |

## Binding

`crates/cs_app/src/mission_world_actors.rs`:

* `ALLEGIANCE_RESOLVED_CLAIM` (`f34-world.zeppelin-allegiance`) — a
  `Resolved::Known` faction whose value is the vocabulary spelling
  (`ContentKind::Faction`: `ally`/`enemy`/`neutral`), at `ObservedTool`,
  provenanced from the carrier record's own span (a `team`-spelling
  override) or the scope's `ai.zrd` span (a resolver outcome);
* `ALLEGIANCE_OPEN_CLAIM` (`f34-world.zeppelin-allegiance-open`) — the
  refusal when the measured path cannot settle: an unreadable carrier, a
  staged `+0x8d` lane outside the vocabulary (#1177), an unmodelled `NODES`
  element, a node name that joins no single subtree, or an int outside the
  measured vocabulary. The reason is verbatim; no faction is invented.
* The retired verdicts: `f34-world.zeppelin-faction-absent` and
  `f34-world.zeppelin-faction-unmeasured` are gone. #793 asked whether the
  record itself carries a faction; #1155 measured that the loader assigns
  one while the record loads. A record whose allegiance cannot settle still
  refuses by name (now under the open claim), so #793's "measured-absent is
  not a blanket pass" survives: measured absence of a `team` key is no
  longer treated as absence of an allegiance, because the original does not
  treat it that way.
* No change was needed in `cs_content::world_actors` or
  `cs_sim::world_actors`: outcome 1 binds a real faction id, so
  `DeclaredWorldActor::faction` and `WorldActorSpec::faction` keep their
  types.

## What changed

* `crates/cs_app/src/mission_world_actors.rs`: the allegiance model
  (`AllegianceSource`, `AllegianceBinding`, `AllegianceLookup`,
  `read_scope_member`, `build_allegiance_lookup`, `apply_turret_records`,
  `allegiance_for`); `bind_mission_world_actors` reads the scope's
  `ai.zrd` through the mission/group/shared archives and passes the
  `ai.zrd` span for provenance (#1177 no longer reads `targets.zrd` here:
  its `nodes` match never gated a staged record); `declare_actor` binds the
  faction from the measured allegiance.
* `crates/cs_app/tests/accept_m01_lc_zeppelin_allegiance.rs` (new): the
  retail acceptance — `open_fields` no longer contains `faction`,
  `lower_world_actors` + `WorldActorSession::launch` succeed
  (`is_satisfied()`), the three factions are `ally`/`enemy`/`enemy` under
  the allegiance claim with the `ai.zrd` span, and the sources are the
  measured turret bindings (`ctur1`, `utur1`, `ctur2`). A unit test in
  `mission_world_actors.rs` keeps the open refusal covered without retail.
* `crates/cs_app/tests/accept_m01_lc_zeppelin_faction.rs` (#793's test):
  still proves no record states `team` and `net.zrd` holds no text, and now
  asserts the binding binds a measured faction instead of the retired
  absent claim.
* `crates/cs_app/tests/campaign/vs_m01_runtime.rs` (#359's gate): the
  `world_actors` surface is now `Satisfied` — the closure's gap set is
  exactly `{world_geometry}`, `plan.launchable()` stays false while
  `world_geometry` is `Unknown`, and the retail refusal text no longer
  names `world_actors`.

## #1177: the gate byte and the team lane (2026-10-10)

Task `M01-LC-ZEPPELIN-ALLEGIANCE-GATE` (#1177) decoded `0x4a2e00`, its
caller `0x4a2be0` and the matcher `0x4a2400` in the same image (sha-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, base
`0x400000`), resolving every `[esp+disp]` by tracking the frame: one
function's MSVC try-level writes all land on a single slot
(`0x4a2e67`/`0x4a2e97`/`0x4a2edd`/`0x4a2fa6`/`0x4a2fc4`/`0x4a307f` → the
`push -1` slot) and the epilogue's `mov ecx,[esp+0x48]` at `0x4a3166` reads
the saved `fs:[0]` chain head, so the loads below are read from slots the
function writes. **Outcome 1 was taken: the staged team is bound with its
measured source. Nothing here is `verified_original`.**

### What the earlier session got wrong

The shift at `0x4a2e45` (`shr edx, cl`) does **not** come from an unset
caller-stack slot: `mov dword ptr [esp+0x18], eax` at `0x4a2e2a` writes the
same slot `mov ecx, dword ptr [esp+0x1c]` reads at `0x4a2e41` (the two
`esp` values differ only by the `push edi` between them), carrying
`2*[0x71c0a0]-2`. The refusal reason that named an uninitialized slot is
retired, and so is #1155's claim that a `targets.zrd` match gates staging.

### The gate byte `[rec+0x8d]`

| address | instruction | meaning |
| --- | --- | --- |
| `0x4a2be0` | the collector, called at `0x464838` inside `LoadMissionData` (`0x464680`) | runs before that function's own `zeppelins.zrd` load at `0x46489c` |
| `0x4a2cf1` | `test dword ptr [ebx+0x28], 0x80000000` | the mark: bit 31 of a node's stored `unk040` (the word is at record offset `0x28`) |
| `0x4a2d1e` | `call 0x4a2400` | the `targets.zrd` `nodes` match — it picks the stored name only |
| `0x4a2d50` / `0x4a2d57` / `0x4a2d5d` | `jne` / `je` / `je 0x4a2d63` | match *and* no-match both reach the insert |
| `0x4a2d63`..`0x4a2d8c` | `{node, name}` → `mov ecx,0x71d358` … `call 0x4a4ef0` | one list entry per marked node, unconditionally |
| `0x4a2e24` / `0x4a2e2e` | `mov esi,[0x71d35c]` / `mov eax,[0x71d360]` | `0x4a2e00`'s first loop walks that list, stride 8 |
| `0x4a2ec9` | `mov byte ptr [ecx+0x8d], 1` | **the gate byte, written for every entry**, then inserted into `0x71d338` at `0x4a2ee2` |

So the gate is the mark bit alone: a `targets.zrd` element never decides
whether a `+0x8d` record exists. `0x4a2400` (called only from `0x4a2d1e`
and `0x4a2f45`) is a bottom-up name matcher: it starts at the element
array's last entry, compares it against the node's name at offset 0 under
`0x4abea0`'s one-digit `*` rule, then moves to `[[node+0x58]]` and the
previous entry, returning 1 only when entry 0 matched.

### The team lane `[rec+8]`

| address | instruction | meaning |
| --- | --- | --- |
| `0x4a2e18` | `mov eax, dword ptr [0x71c0a0]` | the lane's selector |
| `0x4a2e2a` → `0x4a2e41` | `mov [esp+0x18],eax` read back as `mov ecx,[esp+0x1c]` | the shift slot, written not unset |
| `0x4a2e1f` (and `0x4a3306` in the second reader) | `lea eax,[eax+eax-2]` | shift = `2*[0x71c0a0] - 2` |
| `0x4a2e3e`..`0x4a2e4b` | `mov edx,[edi+0x28]`; `shr edx,cl`; `and edx,3` | the two-bit lane of the node's `unk040` |
| `0x4a2e50` | `call 0x453740` | `*out = value` verbatim (`mov dword ptr [eax],ecx`) |
| `0x4a259f` | `mov dword ptr [ebp+8], eax` | **`[rec+8] = the lane`**, in the record constructor `0x4a2570` |
| `0x4befae` → `0x4befb8` | `mov cl,[eax+0x8d]` … `mov ecx,[eax+8]` | the resolver reports the lane when the gate byte is set |

`0x71c0a0` is **never written by this image**: the whole file holds five
references to it (`0x41952b`, `0x4a1465`, `0x4a1f5a`, `0x4a2e18`,
`0x4a32f1`) and every one is a load, while this build writes globals by
absolute displacement (`mov dword ptr [0x71c470],0` at `0x464803`;
`mov dword ptr [0x71c098],esi` at `0x41a88d` — the sibling global
`0x71c098` *is* written that way). The address also sits past `.data`'s
raw bytes (section RVA `0x219000`, raw size `0x2a000` → zero-filled above
`0x643000`), so this build reads `0`. `2*0-2` is `-2`, and x86 masks a
32-bit `shr` count to five bits: **the staged team is `unk040`'s bits
30..31** — `STAGED_TEAM_LANE_SHIFT = 30` in
`crates/cs_app/src/mission_world_actors.rs`.

### Why M01 keeps its three factions

Measured over `zbd/c1c/gamez.zbd`: the container carries 137 marked nodes
of 5 644, and every one of the three zeppelin subtrees holds some
(`panels`, `healthy`, `gunback`). #1155's "the gate matches nothing
marked here" was right about the `targets.zrd` *match* — which does not
gate — and wrong about the mark. The walk order is what preserves the
values:

| zeppelin | first marked node at | turret binding at | decides |
| --- | --- | --- | --- |
| `piratezep` | 146 | 5 (`ctur1`) | the turret → `ally` |
| `workersvoyagezep` | 42 (`panels`) | 77 (`utur1`) | the staged lane → 2, `enemy` |
| `blackswanzep` | 86 | 24 (`ctur2`) | the turret → `enemy` |

Every marked node of M01's container stores `unk040` in `0x8…`/`0x9…`, so
its bits 30..31 read 2: the staged lane can never contradict the values
#1155 measured, and lane 3 (bit 30 set) never occurs here. The retail test
asserts the container-wide lane and each row of that table.

`workersvoyagezep`'s *source* therefore changes: `0x4bef90` reports the
staged `+0x8d` record of the marked `panels` node, not the `utur1` turret
binding, so #1155's retail test now states that and that faction's
provenance is the world container (the member that stores the `unk040`
word) rather than `ai.zrd`. The value is unchanged.

### Outcome

**Bind.** `AllegianceSource::MarkedNode` carries the measured source, the
faction binds under `f34-world.zeppelin-allegiance` at `ObservedTool`, and
a lane of 3 — inside the engine's write, outside the measured vocabulary —
keeps the open refusal by name rather than inventing a faction.

### Residue

If any module outside this supplied image writes `0x71c0a0` at run time,
the lane moves and every staged value moves with it; no such writer is
measurable here, and the derivation above is the whole evidence for the
lane this build reads.

## Still unknown / residues

| residue | affected content | resolves in |
| --- | --- | --- |
| whether a module outside the supplied image writes `0x71c0a0` at run time, which would move the `+0x8d` lane off `unk040`'s bits 30..31 | every mission with a marked node in a zeppelin's subtree — its staged team is read from the lane #1177 measured (`STAGED_TEAM_LANE_SHIFT = 30`), and a different runtime lane would move it | only an original run or a disassembly of a module that is not in `$CS_ENGINE_IMAGE` can settle it; the image itself has no writer |
| the int-valued `team` path (tag 1 at `0x4bda3d`) | a record stating `team` as an int; `cs_formats`'s zeppelins decoder models `team` as one-text (the measured corpus, #574, states only `ally`/`enemy`), so such a record would refuse to decode loudly before this path matters | only if a corpus survey finds an int `team` |
| what `ally`/`enemy` mean to the gameplay side selection (which side the player's faction is on) | how the runtime consumes `faction/ally` vs `faction/enemy` ids | the F34 runtime/faction-consumption stages; this task binds the allegiance, it does not model combat side logic |
| `mission_launch.rs`'s not-satisfied detail text still lists pose/attitude claims, not the allegiance claim | the diagnostic of a *different* mission whose allegiance stays open (M01's surface is satisfied) | a wiring edit by whichever task next touches that detail; outside #1155's owner paths |

## Tests

* `cargo test --workspace --locked -- accept_m01_lc_zeppelin_allegiance_ --include-ignored`
  → discovers 2 tests (1 retail, 1 unit), both pass.
* `crates/cs_app/src/mission_world_actors.rs` unit tests updated: a settled
  allegiance binds `enemy` (spelling override), the resolver's zero binds
  `neutral`, an unsettled allegiance keeps the open field and the lowering
  refuses by name, and the measured program launches with the measured
  faction.
* #793's and #359's tests updated as described above; nothing was skipped,
  deleted or weakened — the expectations moved to the newly measured
  behavior, and the refusals they guarded still refuse by name.
* `cargo test --workspace --locked -- accept_m01_lc_zeppelin_gate_
  --include-ignored` (#1177) → discovers 3 tests (1 retail, 2 unit), all
  pass. One unit test drives `AllegianceLookup::resolve` over an authored
  world — a marked node's staged lane decides the answer, an unmarked one
  still ends at the resolver's terminal zero, and a lane of 3 refuses with
  `0x4a2cf1`/`0x4a2ec9`/`0x4a2e3e`/`0x4a259f` in the reason — so it fails
  when the gate handling is removed; the second pins `0x4a2400`'s bottom-up
  chain, which the gate no longer consults. The retail test keeps M01's
  `ally`/`enemy`/`enemy` with `open_fields` free of `faction`, and asserts
  the container-wide lane and the walk-order table above.
* #1155's retail test now states `workersvoyagezep`'s measured source as
  the staged `panels` record, provenanced from the world container; its
  value, claim, `ObservedTool` class and the two turret-sourced rows are
  unchanged. Nothing was skipped, deleted or weakened — the expectation
  moved to the newly measured source.
