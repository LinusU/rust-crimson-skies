# F04-D: member-level collisions of every mount kind in one session

Date: 2026-10-02. Task #342 `F04-D-member-collisions`, the integration step of
follow-up 2 in `2026-09-28-f04-d-observed-collisions-and-async-cancel.md`.
Spec F04 (`### F04-D`), shared contract `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: build/test plus `retail` (read-only; outputs only under
`private/`; only names, lengths, counts and hashes are committed).
Siblings, measured in separate sessions: `2026-09-29-t345-crimptch-rof-member-collisions.md`
and `2026-09-29-t346-texture-archive-member-collisions.md`.

## Files

- `crates/cs_assets/tests/accept_f04_d_member_collisions.rs` (new): 2 synthetic
  tests, 1 retail test (`#[ignore = "requires CS_GAME_DIR"]`, panics without
  it) and the evidence harness `evidence_report_t342_writes_the_acceptance_report`.
- `docs/findings/evidence/T342.json`: the validated evidence report.
- No production change.

## Layout (designed, not a claim about the original)

One `ContentSession` holds, all `.retail()`:

- the installation's file mounts (`SessionBuilder::mount_installation`);
- `GOSDATA/ASSETS/crimson.rof` (`shared`) and `crimptch.rof` (`patch`) via the
  production `mount_rof_into`, `install` key space, no world;
- 48 texture archives (`ZBD/<world>/texture.zbd` as `mission_world`, five
  `rtexture*.zbd` as `shared`), `texture` key space, each bound to its world
  group. The texture member index is built in the test from
  `read_zbd_textures` (there is still no production texture archive mounter).

`collision_report` runs under no world and each of the 8 world groups.

## Measured (installation fingerprint `b4e780ab…c631978`)

| Kind | Collisions | Verdicts | Lookups |
| --- | --- | --- | --- |
| installation file | 15 | 15 `distinct_by_path` | 1789 own, 1424 not eligible |
| ROF member | 64 | 63 `distinct_by_path`, 1 `conflicting` | 1674 own, 18 blocked |
| texture member | 1676 | 1676 `conflicting` | 36750 blocked, 294000 not eligible |
| across kinds | 2 | 2 `distinct_by_path` | all own |

- Mounting everything together reproduces the sibling results exactly: the
  one ROF overlap is `ASSETS/SCRIPTS/AIRFRAME.SCRIPT` (`crimson.rof` 703 stored
  bytes vs `crimptch.rof` 670, different bytes), and every texture name is held
  by 6, 12, … 48 archives of one or more worlds, all `conflicting`.
- **Every shadowing is blocked.** All 18 + 36750 eligible lookups of overlaps
  are `blocked_unmeasured_order`; none is `own`, `other` or `ambiguous`.
  `ContentSession::resolve` of the airframe key and of a texture of the
  session's world return `ResolveError::UnmeasuredOrder`. Another world's
  copy is never served (`not_eligible`).
- **New observation: two collisions cross kinds.** The loose files
  `GOSDATA/ASSETS/GRAPHICS/arial8.tga` and `…/font.tga` share a file name with
  the `crimson.rof` members `ASSETS/GRAPHICS/ARIAL8.TGA` and `…/FONT.TGA`
  (different sizes: 45636 vs 5981 and 65580 vs 4392 bytes). They are different
  paths, so different keys; each resolves to itself and the VFS decides
  nothing between them. Whether the original engine reads the loose file or
  the archive member for a graphics request is **unmeasured**; any consumer
  of these must name its source explicitly. No other collision crosses kinds,
  and no texture name is the same file name as a file or ROF member.

## Original lookup behavior: unmeasured

Which archive wins in the original (`crimptch.rof` vs `crimson.rof`,
`texture.zbd` vs `rtexture*.zbd`, loose file vs archive member) is not
measured. It needs the original running (owner-gated `human_play`, F04-D
follow-up 1; #352 for the texture order). The report carries
`"precedence_status": "designed"` and `"original_lookup_behavior":
"unmeasured"`; the evidence claim is only `implemented`.

## Tests

| Test | Pins |
| --- | --- |
| `…each_kind_keeps_its_verdict_in_one_session` | synthetic: file, ROF (compressed patch member) and texture overlaps mount into one session; file repeat `distinct_by_path`, the two overlaps `conflicting` and refused by `resolve`, same-named ROF member/texture never compared, nothing else conflicting |
| `…worlds_stay_isolated_with_every_kind_mounted` | synthetic: each world resolves its own `SHARED_TEX`, no world resolves none |
| `…retail_every_member_collision_is_blocked_or_distinct` | the table above, every count pinned with the fingerprint in the failure message |

Mutation probe (applied, observed, reverted; `crates/cs_assets/src` unchanged):
making `resolve_blocking_unmeasured` always block fails
`…each_kind_keeps_its_verdict_in_one_session`.

## Commands

| Command | Exit |
| --- | --- |
| `cargo test --workspace --locked -- accept_f04_d_member_collisions_ --include-ignored` | 0 (3 passed) |
| evidence harness + `python3 tools/validate_evidence.py private/evidence/T342/acceptance.json --artifact-root private/evidence/T342 --require-pass` | 0 |

Review: none yet; the harness records the implementer as a self-review, which
is not independent evidence.
