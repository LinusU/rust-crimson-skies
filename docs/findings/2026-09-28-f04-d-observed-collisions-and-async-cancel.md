# F04-D: Observed collisions and asynchronous cancel

Date: 2026-09-28. Task: F04-D "Compare original lookup behavior for every
observed collision" (`specs/F04-context-aware-virtual-filesystem-and-precedence.md`).
Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test plus `retail` (read-only runs against
`$CS_GAME_DIR`; outputs only under `private/`, nothing derived from original
bytes is committed except hashes and relative spellings in the evidence copy).

## Files and the one observable failure (listed before editing)

- `crates/cs_assets/src/vfs/session.rs`: `PendingRead::complete_with`,
  `PendingRead::cancel_handle`, `ReadCancel`, `ReadProgress`,
  `PENDING_READ_CHUNK`; `ContentSession::resolve` now goes through the
  blocking lookup; `ContentSession::collision_report`;
  `SessionBuilder::mount_installation` marks its mounts retail.
- `crates/cs_assets/src/vfs/source.rs`: `ReadError::Cancelled`.
- `crates/cs_assets/src/vfs/resolve.rs`: `Vfs::resolve_blocking_unmeasured`,
  `ResolveError::UnmeasuredOrder`, `check_member_digest` (split out of
  `read_whole_member` so chunked reads share it).
- `crates/cs_assets/src/vfs/mount.rs`: `MountBuilder::retail`,
  `Mount::is_retail`, `Mount::members`.
- `crates/cs_assets/src/vfs/collision.rs` (new): `observe_collisions`,
  `compare_collisions`, `Collision`, `CollisionMember`, `LookupOutcome`,
  `MemberLookup`, `CollisionVerdict`, `CollisionComparison`,
  `CollisionReport`.
- `tools/cs_inspect/src/resolve.rs`: reports the new outcome as
  `"status": "blocked_unmeasured_order"` (exit 3, like every unresolved
  lookup).
- Tests: `crates/cs_assets/tests/accept_f04_d_collisions_and_async_cancel.rs`
  (6 synthetic, 2 retail marked `#[ignore = "requires CS_GAME_DIR"]`) and the
  harness `crates/cs_assets/tests/evidence_report_f04_d.rs`.
- Wiring only: `crates/cs_assets/src/vfs/mod.rs` (module doc, `pub mod
  collision`, re-exports).

**One observable failure:** before this stage a `PendingRead` read its whole
member in one call, so nothing could stop it: a world switch while a 9 MB
retail `texture.zbd` was being read had to wait for the read to finish (the
bytes were then refused by the new session, but the read itself could not
be cancelled). And spec F04 non-negotiable behavior 2 ("block conflicting
retail resolutions" until the original order is measured) was not
implemented anywhere: two retail mounts holding different bytes for one key
resolved silently by the `designed` order.

## What was measured on the retail installation

Installation fingerprint `b4e780ab…c631978` (content `a0223506…262c12d`),
mounted with the F04-C designed layout (`install` = whole tree, shared;
`world-<n>` = each discovered world group, mission/world, bound to it). Every
member of every mount was grouped by logical file name, then looked up by its
own key under no world and under each of the 8 world groups (`ZBD/C1`, `C1B`,
`C1C`, `C2`, `C2B`, `C3`, `C4`, `C5`) through the same lookup content sessions
use.

- **15 file names collide**, all `.zbd`: `zrdr.zbd` (62 files, 62 distinct
  digests: `ZBD/`, each world group and each mission/multiplayer directory),
  `mis_anim.zbd` (53 files, 53 digests), `texture.zbd`, `gamez.zbd`,
  `cam_anim.zbd`, `rtexture2/4/6/8.zbd` (8 each, one per world group, all
  different), `rtexture14.zbd` (3) and `rtexture9/10/11/12/15.zbd` (one file
  each; they "collide" only because the `install` and `world-<n>` mounts
  both expose the same file under different keys).
- **Every group is `distinct_by_path`.** Every lookup under a context that
  admits the member's mount served the member itself (`own`); every other
  lookup was `not_eligible`. No lookup was ambiguous, blocked, not found or
  served by another member. So, in the designed layout, no retail file is
  reached through the precedence order at all: the directory level
  disambiguates every observed collision, and the unmeasured order decides
  nothing on this installation.
- **Every world's `texture.zbd` is its own.** Under each world context only
  that world's mount serves `world:texture.zbd`; the eight archives have
  eight different digests (sizes 5.6–9.2 MB), so a first-wins basename map
  would have served the wrong world's textures to seven of eight worlds.
- **AC04 on retail.** A `world:texture.zbd` read issued in `ZBD/C1` was
  completed on a second thread; after its first 1 MiB chunk the session was
  closed, a `ZBD/C1B` session opened and the read cancelled. It stopped at
  the next chunk boundary with `Cancelled { read: 1048576 }` and delivered
  nothing; the new session resolved its own (different-digest)
  `texture.zbd` and read it whole, digest-checked. The pending read held
  only its own `Arc<Mount>` description, never a file handle or a borrow of
  the closed session, so there is nothing a later read could use after free.

The full per-member table (mount, container, spelling, size, digest, scope,
outcome counts) is `private/evidence/F04-D/collisions.json`, hashed in
`docs/findings/evidence/F04-D.json`.

## Original lookup behavior: unmeasured (recorded, not guessed)

The task title asks for a comparison with the **original** engine's lookups.
That comparison could not be made with the capabilities available:

- `crimson.exe` (345 KB) contains no asset path strings at all (a plain
  `strings` search finds only `%s\%s`-style formats of its own); the
  installation ships `crimson.icd`, `clokspl.exe` and `drvmgt.dll` beside
  it, and `crimson.icd` likewise shows no `.zbd`/`.rof` spellings (only the
  word `textureheap`). The game code that builds lookup paths is therefore
  not readable as plain data. No attempt was made to unpack or decrypt the
  protected executable, and none should be.
- Observing the original at run time (a file-access trace of the game
  loading each world) needs the game running on Windows with a person
  playing into each world — `human_play`, which an agent never has.
- The only reference tool (S03, `everything2blend.py`) is a contrast, not a
  measurement: it extracts every `texture.zbd` it finds and keeps the
  **first** texture of each name across all world groups — exactly the
  first-wins flattening spec F04 behavior 3 forbids. Its per-world
  `ZBD/<c>/gamez.zbd` use agrees with binding world archives to their world
  group, which is what the designed layout does (`observed_tool` at best).

So `PRECEDENCE_ORDER_STATUS` stays `designed`, the collision report carries
`"precedence_status": "designed"` and `"original_lookup_behavior":
"unmeasured"`, and — per behavior 2 — any retail answer the designed order
alone would decide between different bytes is now **blocked**
(`ResolveError::UnmeasuredOrder`, with every origin). On this installation
that block never fires, because no retail collision reaches the order.
Filed as follow-ups (owner capability needed; not faked here):

1. Measure the original's lookup/search order with a file-access trace of
   the retail game entering each world group and mission (`human_play`).
2. Compare member-level collisions inside archives (texture names shared by
   the eight `texture.zbd` archives, and the `crimptch.rof` patch overlay
   over `crimson.rof`) once the ZBD texture and ROF readers can mount
   archive members. These are not observable at the file level this stage
   works at.

## Design decisions

- **Collisions are grouped by file name, not full path.** The group is the
  set a basename map would flatten; comparing each member's own lookup
  across all contexts shows whether any context serves another member's
  bytes. `install` and `world` mounts are both included, so the same file
  appearing under two keys is visible as such (identical digest, two keys,
  both `own`).
- **The block is restricted to retail mounts.** `MountBuilder::retail`
  marks sources that are original installation data (`mount_installation`
  sets it on every mount it creates). The block applies when a retail
  winner shadows another retail mount whose digest differs or is unknown.
  Authored overlays and opted-in mods keep the designed order: their order
  is the caller's or user's choice, not a claim about the original game.
  This keeps the F04-A/F04-C tests of the designed order unchanged.
- **Cancellation is cooperative at chunk boundaries.** Each chunk is an
  independent `read_member_range` call (open read-only, symlink/inode and
  length checks, read, close), so a cancelled read has no open handle, and
  the whole-member digest is still checked once at the end. The cancel flag
  is sticky and `Release`/`Acquire` ordered; `complete_with` reports
  progress after every chunk (this is also the hook the tests park the
  reader on, deterministically "during" the switch).
- **Closing a session does not cancel its reads.** F04-C's contract (an
  in-flight read may finish after close and is then refused by the
  successor) is kept; the world switch cancels through the handle.

## Mutation probes (run locally, then reverted)

Each is a one-line production edit; `grep -rn "MUTATION PROBE" crates/` is
empty afterwards.

| Probe | Failing tests |
| --- | --- |
| Cancel check in `complete_with` disabled | `cancel_async_read_during_world_switch`, `read_cancelled_before_it_starts_reads_nothing` |
| `resolve_blocking_unmeasured` never blocks | `designed_order_alone_never_decides_between_retail_bytes`, `collision_report_flags_retail_shadowing` |
| Collision lookup ignores mount scope | `collision_report_resolves_every_member_by_its_own_path` |
| Session generation check always passes | `uncancelled_read_of_replaced_world_is_never_reused` |
| World scope ignored in `MountScope::admit` | 4 of 6 synthetic tests |

## Commands run

All from the repository root on `rally/20-compare-original-lookup-behavior-for-eve`,
Rust 1.98.1, `CS_GAME_DIR` set.

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f04_d_ --include-ignored` (tee `private/evidence/F04-D/cargo-test.log`) | 0 — 8 passed (6 synthetic, 2 retail, 161 s) |
| `cargo test --locked -p cs_assets --test evidence_report_f04_d -- --ignored` (env per its module doc) | 0 |
| `python3 tools/validate_evidence.py private/evidence/F04-D/acceptance.json --artifact-root private/evidence/F04-D --require-pass` | 0 |

The evidence claim is `implemented`: a code/test pass, not
`verified_original`. The original lookup order remains unmeasured (above).
