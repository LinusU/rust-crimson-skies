# F04-D: the GOS registration order, and what it decided

Date: 2026-10-06. Task #686 `F04-D-order-rof`. Follow-up of
`2026-10-05-f04-d-original-lookup-order.md` section D and
`2026-09-28-f04-d-observed-collisions-and-async-cancel.md`.

## Provenance

The order implemented here is the one `2026-10-05-f04-d-original-lookup-order.md`
(section D) derived from static analysis of the owner-supplied decrypted image
`$CS_GAME_DIR/crimson.decrypted.exe` and `GOSDATA/ASSETS/BINARIES/roffile.dll`:
`AddNewROFDirectory` (`0x10001000`) only `push_back`s, and `MetaOpenFile`
(`0x100016e0`) walks the registered sources in that order and takes the first
that has the name. This task adds no new evidence about the original's code; it
makes that order the rule the VFS applies to GOS-style requests and measures
what follows from it on this installation.

What *is* measured here is retail: the member sets of both containers, the
loose `GOSDATA` tree, and the answers the order produces for them, through
production code (`cs_assets::vfs::gos::SessionBuilder::mount_gos_chain`,
`Vfs::resolve`, `cs_assets::rof` readers). See
`crates/cs_assets/tests/accept_f04_d_gos_registration_order.rs`.

**No run of the original engine produced any of this.** `GOS_ORDER_STATUS` is
`ClaimStatus::Inferred` and stays that way: only owner-supplied original-run
evidence can raise it.

## A. What the order is, and the two cases

1. `crimptch.rof` under `<EXE Path>\GOSDATA\Assets\` — **only if it exists**,
   where `<EXE Path>` comes from
   `HKLM\SOFTWARE\Microsoft\Microsoft Games\Crimson Skies\1.0`;
2. `<UIAssetPath>\Assets\crimson.rof`, `UIAssetPath` defaulting to `GOSData`;
3. the loose `<UIAssetPath>\` directory;
4. the current directory.

Step 1 is conditional on a **registry** fact that no file in the installation
can settle, so the registry outcome is an explicit input
(`ExePathOrigin::inspect_registry`, which answers by looking for the
container) and every chain records which of three cases it is
(`PatchRegistration`):

| case | what the original does | what the chain registers |
| --- | --- | --- |
| `Registered` | the patch is step 1 | `patch_container → main_container → loose_ui_assets → [current_directory]` |
| `ContainerAbsent` | the key exists, the container does not, so step 1 is skipped | `main_container → loose_ui_assets → [current_directory]` |
| `RegistryKeyAbsent` | the fallback path does not exist, so the patch is never registered | same, and the report names the missing step |

Both non-registered cases start at `crimson.rof`; they are kept apart because
they are different facts about a machine (a missing key is not a missing file),
and because "the patch was never registered" is what a report must be able to
say, as distinct from "the patch lost".

The **unconditional** part, and therefore what the chain pins, is the relative
order of steps 2, 3 and 4 and the position of step 1 when it is registered.

## B. Measured on this installation

Both containers and the loose tree, mounted through the production chain
(`accept_f04_d_gos_registration_order_retail_order_over_the_measured_members`):

| source | mount | members |
| --- | --- | --- |
| `crimptch.rof` | `gos-patch` | 1 |
| `crimson.rof` | `gos-main` | 846 |
| loose `GOSDATA` | `gos-loose` | 18 |

The one key both containers hold is
`ASSETS/SCRIPTS/AIRFRAME.SCRIPT`: 670 stored / 1641 decoded, sha256
`05fa0136…` in `crimptch.rof`, against 703 stored / 1813 decoded, sha256
`e0ba801b…` in `crimson.rof`. Different stored digests, so the shadowing is a
real difference of bytes and not a naming artefact.

With the registry case `Registered`, `ASSETS/SCRIPTS/AIRFRAME.SCRIPT` is served
by `crimptch.rof` and the trace records `gos-main` as a candidate that lost; with
`RegistryKeyAbsent`, the **same request over the same files** is served by
`crimson.rof`, and the patch is not an attempt at all. That pair is the order's
two answers for one request, measured on one installation.

Every one of the chain's 862 keys (846 container keys ∪ the 18 loose keys,
overlapping where the case-only pairs are) resolves to a registered source
under the folding rule, and every lookup's trace reports
`GosRegistration`/`inferred`.

## C. The case-only pairs: the order does not decide this

`crimson.rof` stores `ASSETS/GRAPHICS/ARIAL8.TGA` and
`ASSETS/GRAPHICS/FONT.TGA`; the loose tree stores the same two images as
`ASSETS/GRAPHICS/arial8.tga` and `.../font.tga`. Both spellings were measured to
exist, so **which of the two a GOS request gets depends on
`MetaOpenFile`'s matching rule, which is unmeasured** (#693). This task does
not settle it and does not let the order pretend to:

* `GosNameMatch::AsciiInsensitive` (the documented default, the rule the rest
  of this VFS applies to legacy spellings) serves the container member and the
  loose copy never comes up;
* `GosNameMatch::ExactSpelling` answers only a byte-equal name, so the
  container's uppercase name is missed and the loose spelling is answered by
  the loose file.

Both rules are implemented and the rule is an explicit chain input, recorded
with the chain and reported on every lookup; a second, different rule in one
VFS is refused rather than silently taking effect. The part of the order that
does **not** depend on the rule is the patch-over-main case above, which holds
under either.

## D. Consequences for the rest of the VFS

* **A new key space, `gos`.** Registration order and precedence order are
  different rules; mixing these sources into `install` would let the *designed*
  precedence order decide them and would change every existing F04 lookup that
  shares those mounts.
* **The trace states what decided.** `ResolutionTrace::precedence_status`
  became `order: LookupOrderStatus`, carrying `LookupOrder` (`Precedence` or
  `GosRegistration`) and that order's status (`designed` or `inferred`). Every
  existing report that printed a precedence status now prints the order as
  well; `cs-inspect resolve` bumped to report version
  `cs-inspect-resolve/2`.
* **`UnmeasuredOrder` no longer applies to a GOS answer.** That refusal exists
  because the *designed* order has no original-behavior evidence. The GOS order
  is the original's own order, read out of `roffile.dll`; refusing it would
  refuse every GOS answer over a status the order does not depend on. The
  answer is served and reports `inferred`, and raising that needs an original
  run, not this task.
* **The unmeasured matching rule is scoped to the `gos` key space.** `GosNameMatch`
  is a property of the VFS, because `MetaOpenFile` is the one lookup that walks
  every registered source — but it applies **only** to keys of the `gos`
  namespace. A session that also holds `install`, `reader` or `world` mounts
  keeps folding ASCII case in those key spaces (spec F04 non-negotiable
  behavior 1) whatever a GOS chain states, so an unmeasured rule for one key
  space cannot quietly change another. `Vfs::matching_for` is where that scope
  lives.
* **Container paths resolve as the host spells them.** The original asks for
  `GOSDATA` then `Assets`; the installation spells them `GOSDATA` and `ASSETS`.
  Both components are therefore resolved case-insensitively, the way
  `<UIAssetPath>` already had to be. Without that a case-sensitive host cannot
  mount a chain at all, and `ExePathOrigin::inspect_registry` would report "no
  patch container" for a container that is present — a wrong answer about the
  installation rather than a missing file. `ExePathOrigin::patch_container`
  still returns the *original's* spelling, because that is the path the original
  asks for and what a report should name as where it looked.
* **Collisions stay consistent, and name the order that decided them.** The
  existing F04 collision machinery compares a member under each context through
  the same `resolve_blocking_unmeasured` the session uses, so a GOS collision is
  reported as served by the earlier registration (`other_different_bytes`)
  rather than as blocked. Because a collision is grouped by file name across
  *every* key space, each `MemberLookup` now carries the order and status that
  decided **it**: the report's own `precedence_status` can only describe the
  precedence order, and a reader must not take a GOS verdict for a product of
  the designed one. The `accept_f04_d_rof_member_collisions` verdicts for the
  *installation* namespace are unchanged: those mounts are still decided by
  precedence and still blocked.
* **A container member is read through the container reader.** A ROF `Mount`
  indexes its members' locations and digests but has no host backing, so
  `ContentSession::read_all` refuses it with `ReadError::NoBacking`;
  `GosChain::read` uses `RofSource::read` for containers and the session's own
  read for directory-backed sources. The chain keeps the sources, because
  dropping them would register members nothing could read.

## E. Still unknown

| what | why | who settles it |
| --- | --- | --- |
| `MetaOpenFile`'s case-matching rule | static analysis did not settle it; it decides the `ARIAL8.TGA` / `FONT.TGA` pair | #693 |
| the registry state of any machine | no agent has registry capability; `HKLM\…\Crimson Skies\1.0` is not readable here | the owner, by supplying the key's value or an original-run capture |
| whether `crimptch.rof` shadows `crimson.rof` at runtime | the order is code-derived; nothing here ran the original | an owner-supplied original run |

## Test

`crates/cs_assets/tests/accept_f04_d_gos_registration_order.rs`, prefix
`accept_f04_d_gos_registration_order_`. Eleven synthetic tests pin the four-step
order, both registry-present cases and both absent ones, the shadowed
candidate in the trace, steps 3 before 4, an unregistered step 4, the two name
matching rules and their disagreement, that the matching rule does not leak out
of the `gos` key space, the isolation of the `gos` namespace from `install`,
the collision report's verdict and deciding order for a GOS collision, and a
missing `crimson.rof` naming its step. Two retail tests
(`#[ignore = "requires CS_GAME_DIR"]`) pin section B's measurements and read
the member each registry case chose.

## Evidence

`docs/findings/evidence/T686.json`, generated by
`evidence_report_t686_writes_the_acceptance_report` from the recorded log,
production discovery of `$CS_GAME_DIR`, both production GOS chains of that
installation and a production read of the member each registry case chose.

It validates with `tools/validate_evidence.py` **without** `--require-pass`,
because this report's `unknowns` are the pinned properties themselves (section
E), not failed assertions: `--require-pass` rejects any report carrying an
unresolved issue, and removing them to satisfy it would state that this task
knows things it does not. `claim` is `implemented`; the acceptance run itself
was green (exit 0, 12 selected tests, 12 passed, 0 failed).