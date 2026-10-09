# M02-B-FU3: what the original does with a cross-objective address past the record's block count

Date: 2026-10-09. Task: `M02-B-FU3` (Rally #802), follow-up of `M02-B`
(#262, `missions/M02.md`). Shared contract:
`docs/contracts/SCRIPT-MISSION.md` ("IR requirements": *"Validate names/ids/
ranges"*, *"Control flow is explicit and bounded… Do not kill the whole process
or continue with arbitrary skipped instructions"*). Capabilities used: `retail`
(`$CS_GAME_DIR` read-only) and the owner's decrypted engine image
(`$CS_ENGINE_IMAGE`, read-only, never committed). Parent measurement:
`docs/findings/2026-10-08-m02-b-compatibility-gaps.md`.

Nothing here is `verified_original` (AGENTS.md rule 8): **every runtime
behaviour below is static code evidence** read out of the executable. No
original program was run, no mission was played, and the one observable the
code cannot settle is recorded as an unknown rather than guessed.

## The question, and the answer in one line

M02's control record declares 50 numbered blocks and its block `OBJECTIVE13`
spells `WAKE_OBJECTIVE_WHEN_I_COMPLETE [14, 50]`. What does the original do
with an address past the block count — ignore it, clamp it, log it?

**None of the three.** The wake walk multiplies the address into a record
pointer and touches that record: there is no comparison against the objective
count anywhere in the function, so an address past the count is *executed*
against whatever memory follows the array — no clamp, no ignore, no log, and
an outcome that depends on the allocator's neighbours. Separately, and decisively
for M02 itself: **the parse decrements every objective address before storing
it**, so M02's authored `50` reaches the walk as record index **49** — inside
the 50-record array. M02 therefore never hands its wake walk an out-of-range
address at all.

## Provenance

| Item | Value |
| --- | --- |
| File | `$CS_ENGINE_IMAGE` (`crimson.decrypted.exe`, outside `$CS_GAME_DIR`) |
| Format | PE32 executable, image base `0x400000`, `.text` VA `0x401000` ↔ file `0x1000` |
| SHA-256 | `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` (same image `2026-10-06-m01-lc-directive-b-…md` measured) |
| Address convention | virtual address (VA) |
| Tools | `llvm-objdump -d --triple=i386`, `rabin2 -S`, byte scans over `.text` |
| Installation | `$CS_GAME_DIR`, `install_sha256` re-derived by the acceptance run (evidence `M02-B-FU3`) |

## The functions the measurements come from

| Address | Role |
| --- | --- |
| `0x468c0f` / `0x468c26` | the parse's key dispatch: `WAKE_OBJECTIVE` / `WAKE_OBJECTIVE_WHEN_I_COMPLETE` (strings at `0x626840` / `0x626850`) |
| `0x468c40` | the store loop that fills the `+0x1c` wake array |
| `0x468c60` | the `−1` terminator written after the last stored address |
| `0x468cf0` | `NAP_OBJECTIVE_WHEN_I_COMPLETE`'s target (`+0xd0`), its seconds at `+0xd4` (`0x468d18`) |
| `0x4679fc` | `TICK_DEPENDS_ON_OBJ`'s dependency (`+0x10`) |
| `0x467a21` | `DEDG`'s two integers (`+0x580`/`+0x584`) — the **control**: stored without any decrement |
| `0x467956`..`0x467995` | one record allocated and zeroed per numbered block (`realloc`, stride `0x5e4`, `rep stosl` over `0x179` dwords = 377) |
| `0x469043` | `+0xc48 := the block loop's exit counter` — the objective count |
| `0x469af0` | the wake walk: `−1`-terminated index array → records |
| `0x46a6dd`, `0x46a750`, `0x46a7b9`, `0x46aa47` | the walk's four call sites: three inside `CZMission::Update`'s pass-1 lifecycle paths and one at step 7 of the completion pipeline, the `+0x1c` wake list (the call-site map `2026-10-06-m01-lc-directive-b-…md` measured) |
| `0x469800`, `0x469860` | two other mission methods that take an objective index and *do* test it |
| `0x43eb0b`, `0x43eb58` | the `objective` / `objective_status` console dispatch that calls them |

## Measurement 1 — the parse decrements the address

The wake-array store loop, first iteration (`esi` starts at `1`, so the first
load is `payload + 0xc`, the first child's payload):

```
468c40: lea    0x1c(%ebp),%edx          ; destination: the record's +0x1c array
468c43: mov    0x4(%ecx,%esi,8),%ecx    ; child payload (stride 8, value at +4)
468c47: dec    %ecx                     ; <-- the decrement
468c48: mov    %ecx,(%edx)              ; store the *decremented* address
468c4a: mov    0x4(%eax),%ecx
468c50: incl   %esi
468c51: add    $0x4,%edx
468c54: cmpl   0x4(%ecx),%esi           ; one field per stored child
468c56: jl     0x468c43
468c58: orl    $-0x1,%edi
468c60: mov    %edi,0x18(%ebp,%esi,4)   ; the −1 terminator, one slot past the last
```

The same `mov child; dec; store` shape appears at `0x468cf0` (NAP's target) and
at `0x4679fc` (`TICK_DEPENDS_ON_OBJ`). Two controls show the decrement is
specific to objective addresses rather than an encoding of every integer:

* `DEDG`'s children are stored **without** it — `mov 0xc(%edx),%ecx` followed
  directly by `mov %ecx,0x580(%ebp)` (`0x467a21`), second child at `+0x14`;
* NAP's second child (the seconds) is stored **without** it too — the tag
  dispatch at `0x468d07` (`tag == 2` → raw store at `0x468d18`, `tag == 1` →
  `fildl`, `tag == 3` → the `Expecting real value, found string %s` log at
  `0x468d47`).

So a spelled address `a` becomes record index `a − 1`: the record's own
numbering is **one-based**. Three independent measurements agree:

* **F39-E2's corpus closure** (`measure_block_precedence`,
  `crates/cs_app/src/objectives.rs`) found all 1706 retail targets inside the
  set of declared `OBJECTIVE<n>` **numbers** and `dangling_sites == 0` over 1338
  blocks — corroboration, not proof: read zero-based, that corpus never spells
  a `0` (so no record would ever name its first block) and `zbd/c3/m05`'s
  `WAKE … [9, 10, 11, 44, 30, 68]` would sit one past its own 68 blocks, while
  `c1/m02`'s `50` sits exactly on its 50. Read one-based, every target is
  simply a block number, which is exactly what the check measured. The parse's
  `dec` above is what decides it; this only has to be consistent with it.
* **M03-B's independent measurement, already on `main`**: every one of M03's
  84 wake/kill/nap/gate addresses is in `1..=55` for its 55 blocks and *"the
  spelled value is the block number"* (`docs/findings/2026-10-08-m03-b-control-program-gaps.md`,
  pinned by `accept_m03_b_the_terminal_blocks_are_gated_and_every_address_is_in_range`
  as `(1..=i64::from(BLOCKS)).contains(&address)`). That is the same rule this
  task implements, measured on another mission by another stage.
* **M02's own record**, re-read here through production code: every
  cross-objective address it spells lies in `[1, 50]`, none is `0`, and the
  address the findings name (`50`) resolves to record 49 — the block the record
  spells `OBJECTIVE50`.

The `objective_status` console dispatch corroborates the convention from a
second surface: at `0x43eb51` it executes `decl %eax` before passing the
parsed number to the bounds-checked accessor `0x469860`.

**How this relates to M02-B's pin.**
`accept_m02_b_the_objective_graph_the_sheet_priorities_need_is_measured_not_invented`
pins the *document-level* fact that the authored integer `50` exceeds the 50
blocks when the document's integers are read as zero-based record positions,
and that pin is what this task was filed against. Measured here, the original's
own parse makes the authored integer a **one-based objective number**, so the
same datum is the record's *last* address at runtime and the only address in
M02 that sits on the count boundary. Both statements are about the same bytes;
they differ in which layer they describe, and the runtime layer is the one this
task's rule implements. The M02-B test is untouched: it reads the document and
its assertion still holds.

## Measurement 2 — the record: one record per numbered block, count = block count

```
467956: mov    0xc4c(%edi),%ecx         ; the current array
46795c: … 1508 * (block index + 1) …    ; 0x5e4 = 377 dwords
46796c: call   *0xa20338                ; realloc(array, 1508 * n)
467972: mov    %eax,0xc4c(%edi)
467978: … ebp = new_array + 1508 * block_index …
46798e: mov    $0x179,%ecx
467995: rep stosl                        ; the new record is zeroed
```

and, when the block walk runs out of fields:

```
46903b: mov    0x3c(%esp),%eax
46903f: mov    0x40(%esp),%edx
469043: mov    %edx,0xc48(%eax)         ; count := the block loop's exit counter
```

So `+0xc48` is the **number of numbered blocks** (50 for M02) and the record
array holds indices `0..count`.

## Measurement 3 — the wake walk checks nothing

`0x469af0(array, address_list)`, loop head and tail:

```
469b0d: mov    0x24(%esp),%ecx          ; current slot of the −1-terminated list
469b11: mov    (%ecx),%eax              ; the stored address
469b13: cmpl   $-0x1,%eax               ; terminator?
469b16: je     0x469d9c                 ;   -> return −1
469b1c: lea    (%eax,%eax,2),%edx       ; 3a
469b1f: shl    $0x4,%edx                ; 48a
469b22: sub    %eax,%edx                ; 47a
469b24: lea    (%eax,%edx,8),%ecx       ; 377a
469b27: mov    0x20(%esp),%edx          ; the record array base
469b2b: lea    (%edx,%ecx,4),%esi       ; base + 1508a = base + 0x5e4·a
469b2e: cmpl   %ebx,0x8(%esi)           ; record->+0x8 (alive), ebx == 0
…
469d73: mov    0x10(%esp),%eax          ; entry counter
469d7b: incl   %eax
469d7c: add    $0x4,%edx                ; next slot
469d7f: cmpl   $0xf,%eax                ; cap: 15 entries (skipped ones count)
469d8a: jl     0x469b0d
```

A scan of the whole function (`0x469af0`..`0x469db4`, 232 lines of
disassembly) finds **no reference to `0xc48`** and no other comparison of the
address against a count: the only compares are the `−1` terminator, the record's
own fields and the 15-entry cap. Measured consequences:

* an address `a >= count` reads and writes the record at `base + 0x5e4·a`,
  i.e. memory that is *not* an objective record — never clamped, never ignored,
  never logged;
* an address `< −1` (a spelled `0` decodes to `−1`, which *is* the terminator,
  and any smaller value decodes below it) walks a negative index, i.e. memory
  before the array.

What the original *observes* in those cases — a crash, a silently wrong state
or nothing visible — depends on what the allocator placed beside the array and
is **not decidable from the code**. Recorded as an unknown below.

## Measurement 4 — two other objective accessors do check, and accept one past

```
469800: mov    0x4(%esp),%eax           ; the address argument
469804: test   %eax,%eax
469808: jl     0x469851                 ; address < 0     -> return
46980a: cmpl   0xc48(%ecx),%eax
469810: jg     0x469851                 ; address > count -> return
469812: mov    0xc4c(%ecx),%ecx
469818: … base + 1508·address …
```

`0x469860` has the same shape; the address it receives is a `decl %eax`'d
number from the `objective_status` dispatch at `0x43eb51`, so that command
passes a one-based number where the accessor expects a zero-based index. The guard rejects only
`address < 0` and `address > count`, so **`address == count` is accepted** — one
record past the array — while `address > count` returns without doing anything.
These two methods are therefore *ignored-and-not-logged past the count* only
beyond it, and *executed one past it* exactly at it. The directive path
(`0x469af0`) has no such guard at all.

Who calls `0x469800`/`0x469860` besides the console dispatch, and whether the
`objective` command's own argument is one-based (it pushes the parsed number
straight through at `0x43eb06`, unlike `objective_status`) are **not
established here**; the two console commands disagree and this task measures
the directive parse, not the console surface.

## The named engine rule

> **`OUT_OF_RANGE_OBJECTIVE_ADDRESS`** — a cross-objective address resolves
> only inside `[1, objectives]` (one-based, matching the parse's `dec`) and maps
> to record index `address − 1`. An address outside that range is **refused by
> name**: never clamped to a neighbouring record, never ignored, never silently
> accepted.

Implemented as production code in the objective-lifecycle subsystem:

* `crates/cs_sim/src/objectives/address.rs` —
  [`resolve_objective_address(address, objectives)`] returns
  `Ok(SymbolId(address − 1))` in range and `Err(AddressRefusal { address,
  objectives })` outside it; `AddressRefusal`'s `Display` spells the rule name,
  the address, the count and the words *"refused, never clamped"*;
  `address_of` is its inverse. The rule name is
  [`OUT_OF_RANGE_OBJECTIVE_ADDRESS`].
* `crates/cs_sim/src/objectives/runtime.rs` — the refusal is part of the
  subsystem's error vocabulary: `RuntimeError::Address(AddressRefusal)` with a
  `Display` arm and `From<AddressRefusal>`, so the future executor reports it
  through the same `ObjectiveEventKind::RequestRefused` path every other
  named refusal uses instead of dropping it.
* `crates/cs_sim/src/objectives/mod.rs` — wiring (module and doc).

**Why refusal rather than the original's unchecked walk.** The original's
behaviour past the count is a raw access to memory that is not an objective
record; a safe engine cannot reproduce it, and choosing *what* it would have
done would be a guess (AGENTS.md rule 4). Clamping would move an objective the
record never named; accepting silently would hide a data error behind a
directive that appears to work; skipping without a report would make "the
program asked" and "the world did nothing" indistinguishable — the failure the
runtime's own *"a named reference is never dropped"* discipline exists to
prevent.

**What the rule is not.** It is a resolver plus a named refusal, not an
executor: this build still has **no consumer for the wake/sleep/kill/nap
directive family** (`MissionCountdown` consumes the three timer operations;
`apply_new` skips every other emission). The rule is what that executor calls
when it lands, and wiring it is filed as `M02-B-FU4` (#807) rather than
invented here.

## Files

- `crates/cs_sim/src/objectives/address.rs` — the rule (new).
- `crates/cs_sim/src/objectives/runtime.rs` — the refusal in `RuntimeError`.
- `crates/cs_sim/src/objectives/mod.rs` — wiring only (module, doc).
- `crates/cs_sim/tests/accept_m02_b_fu3_objective_address.rs` — the two
  synthetic arms (CI).
- `crates/cs_app/tests/campaign/m02_b_fu3.rs` — the retail arm over M02's own
  record (new member; the production binding and decode are re-read from disk).
- `crates/cs_app/tests/campaign/evidence.rs` — the evidence harness
  `evidence_report_m02_b_fu3_*` plus its two test lists (the pattern M02-T3
  used in the same file).
- `crates/cs_app/tests/campaign/main.rs` — wiring only (`mod m02_b_fu3;` and
  the doc paragraph).
- `docs/findings/evidence/M02-B-FU3.json` — the committed copy of the
  acceptance report (the artifacts it hashes stay in
  `private/evidence/M02-B-FU3/`).

No `Cargo.toml` or `Cargo.lock` change: no dependency and no crate edge was
added (`cs_sim` already exports its objectives to `cs_app`).

## Test inventory (`accept_m02_b_fu3_`, 3 tests)

| Test | Capability | What it pins |
| --- | --- | --- |
| `accept_m02_b_fu3_an_address_inside_the_record_resolves_to_its_one_based_record_index` (`cs_sim`) | synthetic | address 1 → record 0 and address 50 of 50 → record 49 (both boundaries, so an off-by-one in either direction fails); address 7 of 10; a one-block record; the round trip through `address_of` |
| `accept_m02_b_fu3_an_address_past_the_block_count_is_refused_never_clamped` (`cs_sim`) | synthetic | 51 of 50 (and 500 of 50, 0, −1, `i64::MIN`, 1 of 0) all refuse with `AddressRefusal { address, objectives }` carrying the address *as spelled* — which is what a clamp would destroy; 51 never resolves to record 49; `RuntimeError::from(refusal)` names the rule, says "refused"/"never clamped" and carries both numbers |
| `accept_m02_b_fu3_m02s_wake_addresses_resolve_inside_its_block_count` (`campaign`, retail) | retail | re-reads M02's control member from the installation through production discovery, walks every cross-objective address the record spells (the eight address-taking keys), asserts **all** of them resolve inside the block count, that the census agrees on the count, that `OBJECTIVE13`'s wake site spells `50`, that `50` resolves to record 49 and that record 49 is the block spelled `OBJECTIVE50`, and that one past M02's count refuses |

Every test calls production code (`resolve_objective_address`,
`AddressRefusal`, `RuntimeError`, `SourceContext::control_program`,
`discover_container`, `decode_zrd`, `survey_mission_control_programs`). The
retail test re-derives its numbers from `$CS_GAME_DIR` through two independent
walks (the binding's record and its own document walk); no expected value is
read from the assertion it checks. The retail test is
`#[ignore = "requires CS_GAME_DIR"]` (CI skips it); the two synthetic tests run
in CI.

## Mutation probes

Three mutations were applied one at a time to
`crates/cs_sim/src/objectives/address.rs`, each reverted before the next; the
tree carried none of them afterwards (`diff` against the saved copy showed the
file byte-identical). Every mutation was observed on the synthetic arm
(`cargo test --locked -p cs_sim --test accept_m02_b_fu3_objective_address`);
the clamp and the upper-bound mutations were observed on the retail arm too
(`cargo test --locked --test campaign accept_m02_b_fu3_ -- --include-ignored`):

| Mutation | Observed result |
| --- | --- |
| clamp instead of refuse (`address.clamp(1, objectives)`, the shortcut the rule forbids) | **2 fail**: the synthetic refusal arm (`expect_err` gets an `Ok`), and the retail test at its one-past-M02's-count `expect_err` — a clamp is caught on retail data, not only on authored values |
| zero-based mapping (`SymbolId(address)`) | **1 fails**: the synthetic in-range arm, at both boundaries (`50 of 50` no longer names record 49) |
| upper bound off by one (`address >= objectives`, refusing the record's last address) | **2 fail**: the synthetic in-range arm, and the retail test at the per-address resolve — M02's own `50` no longer resolves |

So the three properties the rule claims — refuse out-of-range, resolve
one-based, keep the count boundary in range — are each pinned by a test that
fails when the implementation stops doing that.

## Checks

Run on this branch, rebased onto `origin/main` at `4003771a`:

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | **1** — exactly one test fails and it is **not this task's**: `cs_xtask`'s `accept_t430_b_ci_workflow_pins_line_tables_for_the_rust_job` requires `.github/workflows/ci.yml` to pin `CARGO_PROFILE_DEV_DEBUG: line-tables-only`, while main's `4003771a` ("Build CI test binaries without debug info") changed it to `0` + `STRIP: debuginfo`. Every other binary is green (451 `test result: ok`), and the contradiction is on main for every branch — the owner's own push run `37864407492` failed the same way and nothing has fixed it since. Filed as `CI-T430-DEBUG-PIN-CONTRADICTION` (#811); `.github/` is protected and this task does not touch it |
| `cargo test --workspace --locked -- accept_m02_b_fu3_ --include-ignored` | 0 (3 tests: the two `cs_sim` synthetic arms and the `campaign` retail arm; recorded in the evidence report) |
| `python3 tools/validate_evidence.py private/evidence/M02-B-FU3/acceptance.json --artifact-root private/evidence/M02-B-FU3 --require-pass` | 0 (`structurally_valid: true`) |

The three mutation probes above were run between the workspace checks, each
reverted before the next; the file was byte-identical to its saved copy after
the last one.

## Review re-run (reviewer bunny-2/bunny-2, 2026-10-09)

The reviewing agent (same agent name and model as the implementer, a different
session with fresh context — recorded honestly, not independent-model review)
rebased the branch onto `origin/main` at `20c4efa3` (main's `decd471f` revert
plus `20c4efa3` restored `CARGO_PROFILE_DEV_DEBUG: line-tables-only` in
`.github/workflows/ci.yml`, so the `CI-T430-DEBUG-PIN-CONTRADICTION` (#811)
contradiction no longer exists on main) and re-ran every check on the rebased
tree:

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | **0** — fully green now that main is fixed, including the T430 binary |
| `cargo test --workspace --locked -- accept_m02_b_fu3_ --include-ignored` | 0 (3 tests), each also passing alone with `--exact` |
| `python3 tools/validate_evidence.py private/evidence/M02-B-FU3/acceptance.json --artifact-root private/evidence/M02-B-FU3 --require-pass` | 0 |

The reviewer regenerated the acceptance report on the rebased commit with
`CS_EVIDENCE_REVIEWER` naming the reviewer; it validates, and its second
production observation `m02-b-fu3-addresses.json` is **byte-identical**
(sha256 `50d13d718b3e9878050cd1b76c5ab94067c94252d4e4381e4845cf2fcb34adf9`)
to the implementer's run's — the same 63 addresses, record indices and rule
verdicts re-derived independently from the installation. One mutation probe was
re-applied by the reviewer (clamp instead of refuse): the synthetic refusal arm
failed, and the file was reverted byte-identical.

## Review re-run after the landing conflict (reviewer bunny-2/bunny-2, 2026-10-09)

Rally's lander could not fast-forward the approved commit `ec442ed4`: main had
moved (with M02-B-FU1's lowering work among others, up to `f48fc720`) and the
automatic rebase hit exactly one conflict, so a reviewer had to rebase by hand.
The conflict was where the overlap check said it would be — both M02-B-FU1
(#800) and this task append their evidence harness to the end of
`crates/cs_app/tests/campaign/evidence.rs` — and nowhere else: of every file
either side changed since the old base `20c4efa3`, only that file is on both
lists, and no `Cargo.toml`/`Cargo.lock` moved on main. The resolution keeps
both tasks' additions verbatim and coexisting (main's `M02-B-FU1` constants,
harness and suite parser; this task's `M02-B-FU3` constants, harness, suite
parser and `address_record`); the branch replayed cleanly on top.

Because that rebase was not conflict-free, the full four checks were re-run on
the rebased tree (the lighter re-push set does not apply):

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | **0** — 472 green `test result` summaries, zero failures |
| `cargo test --workspace --locked -- accept_m02_b_fu3_ --include-ignored` | 0 (3 tests; each also green alone with `--exact`) |
| `python3 tools/validate_evidence.py private/evidence/M02-B-FU3/acceptance.json --artifact-root private/evidence/M02-B-FU3 --require-pass` | 0 (`structurally_valid: true`) |
| `python3 tools/tests/test_evidence_review_identity.py` | 0 (27 OK with the regenerated report committed) |

The acceptance report was regenerated on the rebased tree by the same harness
(its `candidate_tree` equals `git rev-parse 'HEAD^{tree}'` of the commit it ran
on — the commit that carries this findings section, so the only later delta is
this report's own copy, exactly as the harness's method text says; its
`review.identity` names this second review session and records the same-agent,
different-session limitation as above), and the second production observation
`m02-b-fu3-addresses.json` came back **byte-identical** again (sha256
`50d13d718b3e9878050cd1b76c5ab94067c94252d4e4381e4845cf2fcb34adf9`) — the
same 63 addresses, record indices and rule verdicts re-derived from the
installation through the production rule on the rebased tree.

## Residual unknowns (not guessed)

1. **What the original observes past the count.** The walk executes
   `base + 0x5e4·a`; the outcome depends on the memory after (or before) the
   array and cannot be read out of the code. No original program was run, so
   "ignored, clamped, logged, or something else" is answered as **none of those
   three — unchecked execution** at the mechanism level, and left **unknown**
   at the observable level.
2. **The console surface's numbering.** `objective_status` decrements its
   argument before a bounds-checked accessor; `objective` does not, and both
   accessors accept `address == count`. Whether the console commands are
   one-based, zero-based or one of them an original off-by-one is unmeasured
   here (findings §Measurement 4).
3. **The document-level pin's reading.** M02-B's zero-based reading of the
   authored integer and this task's measured one-based parse describe the same
   bytes at different layers; whether the record's *author* intended numbers or
   positions is an intent question the code does not answer (the code's answer,
   `dec` before storage, is measured).
4. **No directive family consumer exists.** The wake/sleep/kill/nap operations
   are measured, lowered and carried as emissions, but no executor applies
   them, so the rule has no live call site yet; wiring it is `M02-B-FU4`
   (#807), and nothing here pretends the rule has been exercised by a running
   mission.
5. **`verified_original` is not claimed.** Nothing here ran the original, and
   no reference capture exists for M02 (REF-OWNER-FIRST-CAPTURE stays blocked on
   the owner).

## Sources

`$CS_ENGINE_IMAGE` read-only through `llvm-objdump`/`rabin2` (addresses above);
`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`SourceContext::read`, `SourceContext::control_program`,
`cs_formats::script_raw::discover_container`, `cs_content::stunts::decode_zrd`,
`cs_app::mission_control::survey_mission_control_programs`;
`missions/M02.md`; `docs/contracts/SCRIPT-MISSION.md`;
`docs/contracts/CLI-EVIDENCE.md`;
`docs/findings/2026-10-08-m02-b-compatibility-gaps.md`;
`docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`;
`docs/findings/2026-10-06-m01-lc-directive-a-objective-directive-parser.md`;
`docs/findings/2026-10-03-f39-e2-block-completion-effect-precedence.md`;
`docs/findings/2026-10-08-m03-b-control-program-gaps.md`;
`crates/cs_app/src/objectives.rs` (`measure_block_precedence`);
`crates/cs_app/src/control_lowering.rs`;
`crates/cs_app/tests/campaign/m02_b.rs`;
`crates/cs_app/tests/campaign/m03_b.rs`.
