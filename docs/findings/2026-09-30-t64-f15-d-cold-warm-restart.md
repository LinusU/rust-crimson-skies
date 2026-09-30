# T64: F15-D — cold/warm/restart equality on the original installation

Date: 2026-09-30. Task #64 `F15-D`, stage D of F15
(`specs/F15-asynchronous-asset-loading-and-private-cache.md`). Depends on
#63 `F15-C`. Shared contract `docs/contracts/IDENTITY-CONTENT.md`; evidence
contract `docs/contracts/CLI-EVIDENCE.md`.

Capabilities used: `retail` (read-only access to the installation at
`$CS_GAME_DIR`; every output under `private/`, only spellings, lengths and
hashes committed). No `gpu`, `audio`, `human_play` or `human_review` claim is
made or needed.

## Files

- `crates/cs_assets/tests/accept_f15_d_cold_warm_restart.rs` (new): the five
  retail scenarios named `accept_f15_d_*`, all
  `#[ignore = "requires CS_GAME_DIR"]` and all panicking without the
  variable, plus `f15_d_child_killed_mid_cache_write_is_not_an_acceptance_test`
  — the restart scenario's child process, deliberately **not** under the task
  prefix so the acceptance selection never discovers it.
- `crates/cs_assets/tests/evidence_report_f15_d.rs` (new): the evidence
  harness.
- `crates/cs_assets/tests/common/mod.rs`: the derived form
  (`derive_texture_base_levels`), the per-archive bound
  (`DERIVED_TEXTURES_PER_ARCHIVE`) and the converter identity
  (`derived_texture_converter`). One definition, used by both the acceptance
  suite and the evidence harness — see "Review fixes" below.
- `docs/findings/evidence/F15-D.json`: the validated evidence report.
- `crates/cs_assets/src/cache/mod.rs`: re-exports the store's own
  `ENTRIES_DIR` and `STAGING_DIR`. No logic change; a caller needs the scratch
  directory's *name* to observe an interrupted write without opening (and
  therefore sweeping) the store. The constants already existed as `pub` in
  `cache::store`.
- No production behaviour changed. Nothing needed repair: every failure this
  stage hunted for turned out to be in the first draft of the tests, not in
  the loading path. That is recorded below rather than claimed as a success.

## Review fixes

Recorded here because a reader of the evidence should know what the merge
reviewer changed, and because two of them make the claims of this stage
*stronger* rather than merely tidier.

1. **The derived form is now one definition, not two.** The acceptance suite
   serialized each base level as `name length ‖ name ‖ width ‖ height ‖
   texels`; the harness's independent cycle serialized it as `name ‖ width ‖
   height ‖ texels`. Both were genuine decodes, but they were different byte
   forms, so `cycle.json` measured a slightly different asset from the one the
   suite cached while presenting itself as a re-measurement of it. Both now
   call `common::derive_texture_base_levels`, which is also where the
   converter identity and the per-archive bound live, so the two cannot drift
   apart again.
2. **The kill is measured, not asserted.** The parent used to assert
   `!killed.signal.is_empty()`, where `signal` was the hardcoded string
   `"KILL"` it had just handed to the `kill` binary — an assertion that
   could not fail, and that would also have passed had the child died of
   something else entirely (a `kill` that did not land leaves the child to
   `abort()` after its hold window, which is still a non-zero status). It now
   uses `Child::kill` (SIGKILL on unix, no external binary) and asserts
   `status.signal() == Some(9)`, read back from the child's exit status.
3. **The interrupted write is proven partial.** The parent used to wait for
   the child's scratch *directory* to appear, which `begin_write` creates
   before a single payload byte is staged, so the kill could in principle land
   on an empty scratch area while the comments claimed a partial payload. The
   child now stages half the payload and only then writes a readiness marker
   **outside the cache root** (the store must never see a file it did not
   write), the parent waits for that marker, and then counts the payload bytes
   the interrupted write left on disk and requires `0 < staged < published`.
   The published length is read straight off the reference entry's file,
   because opening a store would sweep the very write being measured.

## The measured installation

| | |
| --- | --- |
| `install_sha256` | `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` |
| `content_sha256` | `a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d` |
| world groups | 8 (`ZBD/C1`, `C1B`, `C1C`, `C2`, `C2B`, `C3`, `C4`, `C5`) |

Both hashes are produced by `cs_assets::install` at run time. A different
installation is a different measurement; the assertions that depend on this
one (the eight world groups) name the fingerprint they were taken from.

## What is loaded, and how it is mounted (design, not an original claim)

Each world group is mounted with the **designed** baseline layout: the same
`MountBuilder` shape, namespace, precedence and container label that
`SessionBuilder::mount_installation` builds for each group, applied one group
at a time through `SessionBuilder::mount_directory`. Every byte is therefore a
resolved `SourceSpan` of the real installation. Which sources the original
engine really binds to a world is still **unmeasured** (F04-D); that limitation
is inherited, not introduced here.

The closure of a load is that world's own **texture archives** — `texture.zbd`
and its `rtexture*.zbd` tiers, six per group in this installation:

| World | `rtexture*` tiers | primary |
| --- | --- | --- |
| `ZBD/C1` | 15, 2, 4, 6, 8 | `texture.zbd` |
| `ZBD/C1B` | 11, 2, 4, 6, 8 | `texture.zbd` |
| `ZBD/C1C` | 10, 2, 4, 6, 8 | `texture.zbd` |
| `ZBD/C2` | 14, 2, 4, 6, 8 | `texture.zbd` |
| `ZBD/C2B` | 2, 4, 6, 8, 9 | `texture.zbd` |
| `ZBD/C3` | 12, 2, 4, 6, 8 | `texture.zbd` |
| `ZBD/C4` | 14, 2, 4, 6, 8 | `texture.zbd` |
| `ZBD/C5` | 14, 2, 4, 6, 8 | `texture.zbd` |

The list is read from what the mount actually holds, not hardcoded, so another
installation is measured rather than assumed.

### The derived asset is a real decode, not a copy

`read_zbd_textures` (the production ZBD texture reader) parses the archive and
the production base-level decode decodes each texture. The derived form is the
first **8** textures of the archive, each serialized as its name length, name,
extent and decoded texels, in archive order — one definition,
`common::derive_texture_base_levels`, which the evidence harness's cycle also
calls, so `cycle.json` measures these same bytes.

Two things follow, and both matter:

* Every cache entry is an **original image decoded by the engine**, not the
  source bytes under another name. A pass-through converter would make AC04
  nearly vacuous.
* 8 textures per archive is a **project-chosen bound**, not an original one. It
  keeps one load's IO inside a frame budget while still exercising six distinct
  real spans per world. The full archive decodes without error on this
  installation (881 textures in `ZBD/C1/texture.zbd`).

The decoder identity is `zbd-texture-base-level` / `1` / IR `1`. It is a
project-defined converter; **nothing here claims the original engine
precomputes these assets, or with which options**.

## AC04, measured

`accept_f15_d_retail_cold_and_warm_loads_deliver_equal_content_and_gameplay_state`
runs, for **every** world group, one cold load against an empty private store
and one warm load against the store a fresh process would re-open, and requires:

* the cold pass is entirely `Rebuilt` and the warm pass entirely `CacheHit` —
  so equality cannot be produced by silently rebuilding every time;
* identical per-item payload digests;
* an identical `ReadyBundle::closure_hash`;
* an identical **gameplay state**: the same sorted `(content id, asset key)`
  pairs read back out of the Bevy `World` from the entities the controlled
  `ExpectedLoad` handoff attached. The load identity is deliberately excluded
  from the comparison — cold, warm and restarted loads are different
  transactions and must still deliver the same state.

`private/evidence/F15-D/cycle.json` carries an **independent re-measurement** of
the same cycle, run by the evidence harness through the production store (not
through `LoadingSession`), so the report does not rest on a test log alone. For
`ZBD/C1` it records: cold `cache_hits: 0`, warm `cache_hits: 6`, restart with
`swept_interrupted_writes: 1`, and `cold_equals_warm` /
`restart_equals_cold` both true with the same closure hash in all three.

## The failure cases

AC04 only means something if the ways it could be faked are excluded. Four
further scenarios each break the cache the way a plausible shortcut would and
still require the reference content:

1. **`…restart_after_a_killed_cache_write…`** — a **real child process** is
   started on the same test binary, derives real bytes from the real
   installation, opens a real store write, stages half of them, and is then
   killed with `SIGKILL` while the write is in flight. The parent waits for the
   child's out-of-band readiness marker, so the kill lands after real bytes
   reached the disk; it then asserts the child really died of signal 9 and
   measures the surviving scratch: strictly more than nothing, strictly fewer
   bytes than the reference entry published. The next open must sweep exactly
   one interrupted write and deliver the reference content. This is the
   stage's `restart`, on real content.
2. **`…corrupt_cache_entry_is_rebuilt…`** — one published payload has a byte
   flipped behind the store's back. The entry stays committed and its declared
   length still matches, so only `verify_entry`'s digest comparison can catch
   it. Exactly one item must be refused with a named `digest_mismatch` and
   rebuilt; the other five stay warm; the delivered content is unchanged; and
   the next pass is warm again, so the corruption is recoverable, not sticky.
3. **`…cache_never_serves_an_entry_under_another_identity`** — well-formed
   committed entries are planted under a **foreign installation hash**, a
   **foreign source span** (same member, one byte further in) and a **foreign
   converter version**, each holding a payload that is plainly not the real
   derived image. The real load runs over a store that holds them and must
   serve none of them. This is what makes the cache-key identity load-bearing
   rather than decorative: drop any one component from `CacheKey::compute_digest`
   and a planted entry is served and the delivered digest changes.
4. **`…budget_eviction_never_changes_the_delivered_content`** — a budget of one
   entry's worth of bytes against a cold store, so the closure cannot fit, some
   items go uncached and entries are evicted. The delivered content and
   gameplay state are still the reference's: a cache is an optimization, never
   the authoritative data source.

## Sensitivity

The implementation was mutated four times and the suite failed each time
(then restored; the working tree is clean). The first three were the
implementer's; the fourth was added by the merge reviewer, who re-ran the
other three independently and got the same failures.

| Mutation | Caught by |
| --- | --- |
| `CacheStore::commit` never performs the publishing `fs::rename` | `…cold_and_warm…`: 6 swept staging directories where 0 were expected |
| `CacheStore::sweep_staging` made a no-op | `…restart_after_a_killed_cache_write…`: 0 swept where 1 was expected |
| `verify_entry` skips the payload digest comparison | `…corrupt_cache_entry…`: 0 refusals where exactly 1 was expected |
| `CacheKey::compute_digest` drops the decoder version | `…cache_never_serves_an_entry_under_another_identity…`: the planted key became identical to the real one |

## What this stage does **not** establish

Recorded here so a `checked` merge cannot be read as a fidelity claim, and
mirrored in the evidence report's `review.method`:

* **The closure is not a mission closure.** It is each world group's own
  texture archives. A mission's full dependency closure, the script adapter's
  conservative dynamic candidate sets, and every non-texture content kind
  (models, collisions, audio, script, animation, UI) are outside this stage.
  Resolving task: the closure work tracked under the mission path (`M01`).
* **The derived form is project-defined.** Nothing here observes which assets
  the original engine precomputes, with which options, or at which point in
  its load. AC04 is an *internal consistency* property — cache warmth must not
  change what a load delivers — and holds regardless of that answer.
* **No original load semantics are asserted.** That the original engine had a
  cache at all, what it keyed on, or whether it tolerated a kill mid-write are
  unmeasured. This stage measures *our* cache.
* `retail` here means read access to the original files. The original
  executable was not run and no original behaviour is claimed.

`unknowns` in the evidence report is empty on purpose: nothing inside F15-D's
own scope is left unresolved. The items above are limits of scope, and they
gate the claim level (`implemented`, never `verified_original`).

## Cost

One run of the suite is ~85 s wall clock, dominated by installation discovery
(~34 s) and per-world mounting (~1.5 s each), both shared across the binary
through a `OnceLock`. The restart scenario's child pays its own discovery,
which is why the parent waits with a generous window rather than a fixed sleep.
