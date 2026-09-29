# T346: texture-name collisions across the per-world texture archives

Date: 2026-09-29. Task #346 `F04-D-member-collisions-texture`, part of #342
(follow-up 2 of `2026-09-28-f04-d-observed-collisions-and-async-cancel.md`).
Spec F04 (`### F04-D`), shared contract `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: build/test plus `retail` (read-only; outputs only under
`private/`; only names, lengths and hashes are committed).

## Files

- `crates/cs_assets/tests/accept_f04_d_texture_member_collisions.rs` (new): 3
  synthetic tests, 1 retail test (`#[ignore = "requires CS_GAME_DIR"]`, panics
  without it), and the evidence harness
  `evidence_report_t346_writes_the_acceptance_report`.
- `docs/findings/evidence/T346.json`: the validated evidence report.
- No production change. `vfs/collision.rs` already compares any mounted
  members; the ZBD texture members this task needs are the ones
  `cs_formats::texture::read_zbd_textures` already exposes (`name`, `stored`,
  `start_offset`), so it needs no member-level field.

## What mounts (design, not a claim about the original)

There is still no production *texture archive mounter* (`cs_content::textures`
opens a single archive through a session; it does not build a member index), so
the test mounts the index the production reader exposes: for every retail
`ZBD/<c>/texture.zbd` and `ZBD/<c>/rtexture*.zbd`, each texture becomes a
`MountBuilder::add_member` entry whose length is `stored().len()`, whose offset
is the stored level's offset inside the container, and whose digest is the
SHA-256 of the stored level. The mount is `.retail()` and bound to its world
group. That glue lives only in the test; it is documented as such because the
owner paths of this task are `crates/cs_assets/tests/`, `…/src/vfs/collision.rs`
(only if needed) and `docs/findings/`.

| Container | Mount id label | Precedence class | Scope |
| --- | --- | --- | --- |
| `ZBD/<c>/texture.zbd` | `zbd-<c>-texture.zbd` | `mission_world` | world group `<c>` |
| `ZBD/<c>/rtexture*.zbd` (5) | `zbd-<c>-rtexture*.zbd` | `shared` | world group `<c>` |

Ranking `texture.zbd` above its `rtexture*.zbd` is a **designed** baseline
(F02-C observed `texture.zbd` as the expected primary archive of a group; F08-C
recorded that which archive a mission really uses is unknown). Because all 48
archives are retail and the order is only designed, that preference never
decides between different bytes: a lookup it would decide is refused with
`ResolveError::UnmeasuredOrder` (F04 non-negotiable behavior 2). `rimage.zbd`
at the `ZBD/` root is not a member of any world group and is out of scope
here.

## Measured on the retail installation

Installation fingerprint `b4e780ab…c631978` (content `a0223506…262c12d`).
`ContentSession::collision_report` under no world and under each of the 8 world
groups (`ZBD/C1`, `C1B`, `C1C`, `C2`, `C2B`, `C3`, `C4`, `C5`), 9 contexts.
48 archives: 8 world groups × 6 (`texture.zbd` + `rtexture2`, `rtexture4`,
`rtexture6`, `rtexture8`, `rtexture15`).

- **Every texture name is a member of the whole world, not of one archive.**
  Each of a world's six archives holds the same name set, so the worlds have
  881 / 667 / 593 / 820 / 601 / 732 / 935 / 896 textures. The union is
  **1676 distinct names**; 551 of them are in all 48 archives; no name is in
  a single archive. How many archives hold each name (always a multiple of 6):

  | archives | 6 | 12 | 18 | 24 | 30 | 36 | 42 | 48 |
  | --- | --- | --- | --- | --- | --- | --- | --- | --- |
  | names | 839 | 148 | 57 | 25 | 28 | 25 | 3 | 551 |

  Total name-holdings 36750 (= 6 × 6125). So a first-wins basename map — what
  the community tool S03 does — would flatten 1676 names drawn from up to 8
  worlds into one archive.
- **All 1676 collisions are `conflicting`.** A retail texture name has, within
  a world, different stored bytes in `texture.zbd` than in its `rtexture*.zbd`
  tiers (the tiers can agree with each other; e.g. `rtexture2`/`rtexture4` and
  `rtexture6`/`rtexture8` often do). Under a world context every eligible
  lookup — the primary's included, since the tiers shadow it — is
  `blocked_unmeasured_order` with that world's `texture.zbd` selected and its
  five tiers shadowed. Every other-world lookup is `not_eligible`, and **no
  lookup is `own`**: the designed order never serves a texture. Of the 330750
  lookups (36750 member-holdings × 9 contexts) exactly 36750 are blocked and
  294000 are not eligible — none is `own`, `other_*`, `ambiguous` or
  `not_found`. A name this world does not hold is `NotFound` through the
  session's own `resolve` — another world's bytes are never returned.
- **Cross-world isolation.** Each world's six archives are visible only under
  that world's context, so a texture name held by several worlds is a
  different member in each and never crosses a world boundary. That is the
  F04 non-negotiable behavior 3 result for the texture archives.
- The collision report carries `"precedence_status": "designed"` and
  `"original_lookup_behavior": "unmeasured"`.

The full per-name table (worlds, archive count, distinct digests, per-context
verdict, designed selected archive) and the mounted archive list are
`private/evidence/T346/texture-member-collisions.json`, hashed in
`docs/findings/evidence/T346.json`.

## Original lookup behavior: unmeasured

Which archive a world really uses — `texture.zbd` or one of the
`rtexture*.zbd` tiers, and in which order a name is looked up when several
hold it — is not measured here. It is the F08-C recorded unknown; follow-up 1
of the F04-D findings (a file-access trace of the original entering each
world) needs `human_play`, which an agent never has. #352 owns establishing
the real selection order. Nothing in this task guesses it: the designed
preference is only a baseline, and every retail lookup it would decide is
refused. The evidence claim is only `implemented`.

**Consequence for texture work:** until the order is measured, a world's
texture names cannot be read through a content session by name alone — the
session returns `UnmeasuredOrder`, and `cs_content::textures` must be given a
concrete archive key (as it already is). That is the intended block.

## Tests

| Test | Pins |
| --- | --- |
| `accept_f04_d_texture_member_collisions_each_world_serves_its_own` | two world-bound retail mounts, one same-name member each, different bytes → each world resolves its own container/span, the other world's name is `NotFound`, no-world resolves neither; report verdict `distinct_by_path`, one comparison, per-context `own`/`not_eligible` |
| `accept_f04_d_texture_member_collisions_shared_scope_overlap_is_blocked` | shared-scope retail overlap, different bytes → `ResolveError::UnmeasuredOrder` (overlay selected, base shadowed) through `resolve` and every report lookup; verdict `conflicting` |
| `accept_f04_d_texture_member_collisions_equal_priority_overlap_is_ambiguous` | two equal-priority retail mounts, one name, different bytes → `ResolveError::Ambiguous` with both origins, never a first-wins pick |
| `accept_f04_d_texture_member_collisions_retail_each_world_keeps_its_textures` | the retail measurement above: 48 archives, the exact per-world texture counts, whole-world membership, the 1676/551/36750 and histogram numbers, five `rtexture` tiers per world, and every collision's verdict and per-context lookup outcome |

## Commands

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f04_d_texture_member_collisions_ --include-ignored` | 0 (4 passed) |
| same, without `CS_GAME_DIR` (retail test only) | 101 (panics: `CS_GAME_DIR must name…`) |
| evidence harness + `python3 tools/validate_evidence.py private/evidence/T346/acceptance.json --artifact-root private/evidence/T346 --require-pass` | 0 |

## Review

Implementer: `glm-1/deepseek-1`. Independent review is pending (see the
`submit_for_review` handover). The retail test re-derives every number this
file records from the production reader, so a reader or mount change that
mounts less than the archives hold fails there instead of quietly
invalidating the findings; each failure message names the installation
fingerprint, so a different retail build reads as a different measurement.

Mutation probe (applied, observed, reverted; `crates/cs_assets/src/` is
byte-identical to the branch head afterwards): making
`Vfs::resolve_blocking_unmeasured` never block (`if true || …`) fails
`…shared_scope_overlap_is_blocked` and `…retail_each_world_keeps_its_textures`,
so the tests really exercise the production block rather than the report.

## Sources

- `docs/findings/2026-09-28-f04-d-observed-collisions-and-async-cancel.md`
  (follow-up 2), `…2026-09-28-f08-c-texture-catalog-and-upload-boundary.md`
  (per-world texture resolution; the archive-selection unknown),
  `…2026-09-29-t345-crimptch-rof-member-collisions.md` (sibling member-level
  comparison).
- `crates/cs_assets/src/vfs/{collision,resolve,mount,session}.rs`,
  `crates/cs_formats/src/texture/zbd.rs`, `crates/cs_assets/src/install.rs`.
- `$CS_GAME_DIR` (read-only).
