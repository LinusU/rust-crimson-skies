# T345: crimptch.rof member collisions over crimson.rof

Date: 2026-09-29. Task #345 `F04-D-member-collisions-rof`, part of #342
(follow-up 2 of `2026-09-28-f04-d-observed-collisions-and-async-cancel.md`).
Spec F04 (`### F04-D`), shared contract `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: build/test plus `retail` (read-only; outputs only under
`private/`; only names, lengths and hashes are committed).

## Files

- `crates/cs_assets/tests/accept_f04_d_rof_member_collisions.rs` (new): 4
  synthetic tests, 1 retail test (`#[ignore = "requires CS_GAME_DIR"]`,
  panics without it), and the evidence harness
  `evidence_report_t345_writes_the_acceptance_report`.
- `docs/findings/evidence/T345.json`: the validated evidence report.
- No production change. `vfs/collision.rs` already compares any mounted
  members, ROF members included, so it needs no member-level field (see
  *Stored vs decoded digests* below for why none was added).

## Designed layout (not a claim about the original)

Both archives are mounted through the production `cs_assets::rof::mount_rof_into`
into one content session, both `MountBuilder::retail()`, both in the
`install` key space the `cs-inspect rof` command uses, both unbound to any
world, mission or locale:

| Container | Mount id | Precedence class |
| --- | --- | --- |
| `GOSDATA/ASSETS/crimson.rof` | `rof-gosdata-assets-crimson.rof` | `shared` |
| `GOSDATA/ASSETS/crimptch.rof` | `rof-gosdata-assets-crimptch.rof` | `patch` |

The `patch` class makes the designed order *prefer* `crimptch.rof`. Because
both are retail and the order is only `designed`, that preference never
decides between different bytes: the lookup is refused with
`ResolveError::UnmeasuredOrder` (F04 non-negotiable behavior 2).

## Measured on the retail installation

Installation fingerprint `b4e780ab…c631978`. `ContentSession::collision_report`
under no world and under each of the 8 world groups (`ZBD/C1`, `C1B`,
`C1C`, `C2`, `C2B`, `C3`, `C4`, `C5`), 9 contexts:

- `crimson.rof` mounts 846 members, `crimptch.rof` 1.
- **Exactly one key is held by both archives:**
  `ASSETS/SCRIPTS/AIRFRAME.SCRIPT` (record id 0 in both, compressed in
  both).

  | | stored len | stored sha256 | decoded len | decoded sha256 |
  | --- | --- | --- | --- | --- |
  | `crimson.rof` | 703 | `e0ba801b…1a128f` | 1813 | `910d9292…110cd0` |
  | `crimptch.rof` | 670 | `05fa0136…8e58d6` | 1641 | `9c22a35a…59994b` |

  The two members differ in stored **and** decoded bytes, so the patch
  really changes the script and is not just a recompression. Verdict
  `conflicting`: all 18 lookups (2 members × 9 contexts) are
  `blocked_unmeasured_order` with `crimptch.rof` selected and `crimson.rof`
  shadowed, and `ContentSession::resolve` refuses the key the same way. So
  a content session cannot read `AIRFRAME.SCRIPT` from either archive until
  the original order is measured. That is the intended result.
- **The 63 other file-name collisions are all `distinct_by_path`.** They are
  basenames `crimson.rof` repeats in different directories (aircraft `.bm`
  textures such as `agyro_fusalage1.bm` ×4, `bri_engine.bm` ×3). Every member
  resolves to itself under every context, and nothing reaches the order.

The full table is `private/evidence/T345/rof-member-collisions.json`,
hashed in `docs/findings/evidence/T345.json`.

## Stored vs decoded digests

A ROF mount records the SHA-256 of the member's **stored** extent (F05-C,
F05-D), and the block compares those digests. Two members with equal
decoded content but different compression would therefore be *blocked*,
not reported as identical. The synthetic test
`…equal_content_stored_differently_stays_blocked` pins this. It is the
conservative direction: nothing is ever served on the strength of a
decode the lookup did not do. It never matters on retail, because the one
shared key differs in decoded bytes too. A decoded-digest comparison would
need a member-level decoded hash in the mount or the report. That was not
added, because no observed case needs it.

## Original lookup behavior: unmeasured

Nothing measured says whether the original engine reads `crimptch.rof`'s
`AIRFRAME.SCRIPT` instead of `crimson.rof`'s. The file name suggests a
patch, but that is a guess, and the order is not decided from it. Measuring
it needs the original running (a file-access trace, or the airframe values
observed in play). That is the owner-gated follow-up 1 of the F04-D findings
(`human_play`). Until then the collision report carries
`"precedence_status": "designed"` and `"original_lookup_behavior":
"unmeasured"`, and the key stays blocked. The evidence claim is only
`implemented`.

**Consequence for script/airframe work:** any consumer of
`ASSETS/SCRIPTS/AIRFRAME.SCRIPT` through a content session gets
`UnmeasuredOrder` until that measurement exists. It must not pick one of
the two archives itself.

## Tests and mutation probes

| Test | Pins |
| --- | --- |
| `accept_f04_d_rof_member_collisions_different_bytes_are_blocked` | same key, different bytes → `UnmeasuredOrder` via `resolve` (also case/separator-folded) and in every report lookup; a base-only member still resolves |
| `accept_f04_d_rof_member_collisions_identical_digest_resolves` | same key, identical bytes → resolves; verdict `shadowed_by_identical_bytes` |
| `accept_f04_d_rof_member_collisions_equal_content_stored_differently_stays_blocked` | plain vs hand-authored zlib stored-block stream: equal decoded bytes, different stored digests → blocked |
| `accept_f04_d_rof_member_collisions_other_directories_are_distinct_by_path` | same name, other directory → `distinct_by_path` |
| `accept_f04_d_rof_member_collisions_retail_patch_over_base_until_measured` | the retail measurement above; shared keys computed independently from both sources' member lists |

Each probe changed one production line, was run, and was then restored.
Afterwards `grep -rn "MUTATION PROBE" crates/` returns nothing:

| Probe | Failing tests |
| --- | --- |
| `resolve_blocking_unmeasured` never blocks (`if true \|\| shadowed.is_empty()`) | `different_bytes_are_blocked`, `equal_content_stored_differently_stays_blocked`, `retail_patch_over_base_until_measured` |
| only `patch`-class mounts can be shadowed | `different_bytes_are_blocked`, `equal_content_stored_differently_stays_blocked` |
| collision report never sees same bytes (`same_bytes: false`) | `identical_digest_resolves` |

## Commands

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f04_d_rof_member_collisions_ --include-ignored` | 0 (5 passed) |
| same, without `CS_GAME_DIR` (retail test only) | 101 (panics: `CS_GAME_DIR must name…`) |
| evidence harness + `python3 tools/validate_evidence.py private/evidence/T345/acceptance.json --artifact-root private/evidence/T345 --require-pass` | 0 |

## Sources

- `docs/findings/2026-09-28-f04-d-observed-collisions-and-async-cancel.md`,
  `…f05-c-mount-rof-into-vfs-and-expose-inspection.md`,
  `…f05-d-resolve-compressed-length-semantics.md` (stored/decoded words).
- `crates/cs_assets/src/vfs/{collision,resolve,mount,session}.rs`,
  `crates/cs_assets/src/rof.rs`.
- `$CS_GAME_DIR` (read-only).
