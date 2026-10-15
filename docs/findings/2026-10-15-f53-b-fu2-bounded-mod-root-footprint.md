# F53-B-FU2: Bounding the bytes a mod root asks to index

Date: 2026-10-15. Task: F53-B-FU2 (#745) "F53-B follow-up: bound the bytes a
mod root asks to index when it is mounted"
(`specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
non-negotiable 1, "validate … budgets before enabling"; the follow-up was
filed by F53-B #213's review). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary build/test
only (no `CS_GAME_DIR` read, no evidence report required).

## Owner paths (decided before editing)

Given by the task: `crates/cs_content/src/mods/`,
`crates/cs_assets/src/mods.rs`, `docs/findings/`. Nothing else was touched;
in particular `crates/cs_assets/src/vfs/source.rs` — the F04-B directory
walker, which is also the *installation* walk — was left alone, so the
retail installation index (F04-B) keeps exactly the behavior it had.

## The problem this closes

`mount_mods` (F53-B) mounts each planned mod's root through
`cs_assets::mods::ModRoot::mount`, which walked and SHA-256 hashed **every**
file below that root. The F53-A budgets `MountLimits::max_declared_bytes_per_mod`
/ `max_declared_bytes_total` were re-checked by F53-B only against the
measured sizes of the *declared* override sources, so files no manifest
declares were walked, hashed and indexed with no budget over them: a mod
declaring one 512-byte override and shipping a multi-gigabyte directory was
mounted and indexed in full. `MountLimits`' own documentation says the limits
exist "to keep a hostile or corrupt manifest from asking for an unbounded
amount of work"; that half of the work was unbounded. The F53-B finding
(`docs/findings/2026-10-07-f53-b-safe-mounts-and-compatibility-signatures.md`,
"Limitation recorded, not fixed here") filed it rather than guessing.

Affected content: every mod root mounted through `mount_mods` /
`ModRoot::mount` (any F53 mod directory). Not affected: the original
installation walk (F04-B) — it is not a mod root and no code path there
changed.

## The decision: a new explicit budget, not a repurposed one

The task offered two designs and forbade silently repurposing the declared
budgets (they are documented as declared-size budgets and are folded into
`ModPlan::hash` through `MountLimits::encode()`). **Chosen: a new explicit
`MountLimits` pair measuring what a root *holds*.**

* `max_root_bytes_per_mod` — most bytes one mod's root directory may hold,
  counted across every regular file below it, declared or not. Default
  **256 MiB**.
* `max_root_bytes_total` — most bytes the roots of the whole set may hold,
  measured the same way. Default **1 GiB**.

Why not the alternative ("a mod's measured root footprint must fit an
existing declared budget"): that would make `max_declared_bytes_per_mod` mean
two different things under one name — what a manifest claims *and* what a
root holds — and every later reader would have to discover which meaning a
given check used. Two named fields make each bound's meaning and its default
explicit; the declared pair keeps meaning exactly what F53-A said.

Rationale for the defaults (all designed vocabulary, not measured original
behavior — F53 "Research boundary"; F53-D still applies):

* the per-mod root bound is deliberately **four times** the per-mod declared
  bound (64 MiB), so a mod can never be refused for holding less than its own
  declared payload already accounts for, with room left for the files a real
  mod ships beside the ones it overrides;
* without a total, a set would only be bounded by `max_mods` (64) times the
  per-mod figure — 16 GiB of indexing by construction. The 1 GiB total caps
  that at a figure an owner can re-tune in one place.

Both fields are folded into `MountLimits::encode()`, so two plans computed
under different budgets are different plans (no pinned plan-hash fixture
exists in the workspace; the full suite confirms it). Changing these defaults
later changes every `ModPlan::hash` and every compatibility signature, by
design.

## What was implemented

- **`crates/cs_assets/src/mods.rs`** — `ModRoot::mount(mod_id, root,
  max_root_bytes)`: the bound is now a required argument, so no caller can
  mount a mod root without stating it. Enforcement happens twice:
  1. a metadata-only measurement (`measure_root_bytes`, a directory listing
     plus one stat per file, following no symbolic link and counting exactly
     the files the index walk would mount) refuses a root that is already too
     large **before `mount_directory` opens or hashes anything** — the
     SHA-256 pass, which is the expensive work, never runs over an
     over-bound root;
  2. after the index walk, the members actually recorded are summed and
     checked again against the same bound, so a file that grew between the
     two measurements is refused rather than indexed over the bound.
  A root inside the bound takes the identical path it took before. The
  measured footprint is stored and exposed as `ModRoot::footprint_bytes()`.
  New refusal: `ModMountError::RootFootprintExceeded { mod_id, measured, limit }`.
- **`crates/cs_content/src/mods/mod.rs`** — the two `MountLimits` fields,
  their designed defaults and their `encode()` entries; the struct's doc now
  separates the *declared* pair (plan-time, what a manifest claims) from the
  *root* pair (mount-time, what a root holds), so the "unbounded amount of
  work" sentence is now true of both halves. One existing F53-A test that
  spelled every field out gained `..MountLimits::default()` for the new
  fields; its assertions are unchanged.
- **`crates/cs_content/src/mods/mount.rs`** — `mount_mods` passes
  `limits.max_root_bytes_per_mod` into each root mount, maps the
  `ModMountError` refusal onto the mount's own vocabulary, sums
  `footprint_bytes()` across the set and refuses it against
  `limits.max_root_bytes_total`, all before a single declared source is
  resolved (so nothing is enabled). New refusals with stable codes:
  `MountError::RootFootprintExceeded` → **`mod_root_footprint_exceeded`** and
  `MountError::TotalRootFootprintExceeded` → **`total_root_footprint_exceeded`**.
  Module docs list the new refusal alongside the others.

**One observable failure:** delete the `measured > max_root_bytes` check in
`ModRoot::mount` and `accept_f53_b_fu2_a_root_holding_more_than_the_footprint_budget_is_refused`
fails; delete the `root_bytes > limits.max_root_bytes_total` check in
`mount_mods` and `accept_f53_b_fu2_the_roots_of_the_set_are_held_to_the_total_footprint_budget`
fails.

## Tests

Task prefix `accept_f53_b_fu2_`, 5 tests, all unit tests calling production
code, selected by
`cargo test --workspace --locked -- accept_f53_b_fu2_ --include-ignored`:

1. `cs_assets` `…_a_root_over_the_footprint_bound_is_refused_before_indexing`
   (unix): an over-bound root is refused with the mod, the measured bytes
   and the limit. The probe is a file the *index walk* could not hash
   (`open` denied by mode, `stat` readable): were the bound checked only
   after indexing, this mount would fail as `UnreadableFile` instead, so the
   ordering of the refusal is asserted, not assumed. (A first version used
   a non-UTF-8 file name as the probe; macOS refuses to *create* such a
   name — `EILSEQ` — so that version could not run on this host.)
2. `cs_assets` `…_a_root_within_the_footprint_bound_reports_its_footprint`:
   a root inside the bound mounts, reports every regular file below it
   (undeclared file counted), still serves the hashed bytes, and the bound
   is exact — one byte less refuses, exactly-at mounts.
3. `cs_content` `…_a_root_holding_more_than_the_footprint_budget_is_refused`:
   a declared payload inside every *declared* budget cannot smuggle 512 KiB
   of undeclared files past the mount; the refusal is the mount's own (the
   plan accepted the set), carries `mod_root_footprint_exceeded`, names the
   mod, and the same set mounts unchanged once the budget covers its
   footprint.
4. `cs_content` `…_the_roots_of_the_set_are_held_to_the_total_footprint_budget`:
   three mods, each root inside the per-mod bound, refused against the total
   with `total_root_footprint_exceeded`; the same set mounts under the
   designed default total.
5. `cs_content` `…_a_set_within_the_footprint_budget_serves_exactly_as_before`:
   inside the designed defaults, two conflicting mods plus an undeclared
   file mount, serve both winning payloads with their recorded digests, and
   produce the same compatibility signature twice; the undeclared file is
   counted in both roots' footprints and serves no content id.

All pre-existing `accept_f53_a_*`, `accept_f53_b_*` (including F53-B-FU1's
7 `accept_f53_b_fu1_*`) and the full workspace suite pass unchanged.

## Checks run (all exit 0)

- `cargo fmt --all -- --check` → 0
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` → 0
- `cargo test --workspace --locked -- accept_f53_b_fu2_ --include-ignored` →
  0, 5 tests executed (2 in `cs_assets`, 3 in `cs_content`), all passing
- `cargo test --workspace --locked` → 0, 576 `test result: ok` lines, 0 failures

## Sensitivity probes (run on this branch and reverted; none committed)

Each removed one behavior; the tree was restored with `git checkout --` and
verified clean afterwards, and the selection was re-run green (5/5).

1. **Both per-mod checks removed** (`ModRoot::mount` no longer refuses an
   over-bound root, before or after the walk) → **3** failures:
   `…_a_root_over_the_footprint_bound_is_refused_before_indexing` (the
   chmod-000 probe file makes the un-bounded mount fail as `root_unavailable`
   instead), `…_a_root_within_the_footprint_bound_reports_its_footprint`
   (the exact-bound refusals now mount), and
   `…_a_root_holding_more_than_the_footprint_budget_is_refused`.
2. **The pre-index measurement check removed** (bound enforced only after the
   walk) → exactly **1** failure:
   `…_a_root_over_the_footprint_bound_is_refused_before_indexing`, with
   `cannot mount the root of mod synthetic.repaint: cannot read file
   …/art/locked.bin: Permission denied` — the mount really did reach the
   hashing walk, which is what the probe is for.
3. **`mount_mods` restored to its pre-F53-B-FU2 behavior** (per-mod bound
   passed as `u64::MAX`, set total check disabled) → **2** failures:
   `…_a_root_holding_more_than_the_footprint_budget_is_refused` and
   `…_the_roots_of_the_set_are_held_to_the_total_footprint_budget`, while
   `…_a_set_within_the_footprint_budget_serves_exactly_as_before` still
   passes — the unchanged path really is unchanged.

## Unknowns, limitations and where they are resolved

- **The bounds are designed, not measured.** No original mod behavior is
  measured (F53 "Research boundary"); the numbers are project design the
  owner may re-tune, and re-tuning them changes every plan hash and
  compatibility signature (by design). F53-D / original-run evidence would
  be needed to say anything about the original game's mod support.
- **The measurement walk is bounded in bytes, not in entry count.** A root
  holding millions of tiny files still costs a directory listing and a stat
  each (`max_overrides_per_mod` bounds manifest entries, not shipped files).
  The expensive SHA-256 pass is bounded; the metadata walk is not. Filed as
  follow-up work rather than folded in here, which keeps this change one
  focused behavior.
- **Symlinks and non-regular files are not counted** in the footprint,
  because they are never mounted — the measurement describes exactly the
  member set the index walk admits.
- **The two measurements can disagree** if a file grows between them; the
  second one (the members actually indexed) is the one that counts, and it
  is checked before anything is enabled.
- **Nothing else measures what a mount costs.** The plan is still
  computed before any IO; a set whose *plans* are cheap but whose roots are
  huge is now refused at the root stage, which is where the bytes are.

## Identities

Implementer: `bunny-alpha-1/bunny-alpha-1` (Rally agent `bunny-alpha-1`),
session of 2026-10-15.
Reviewer: recorded on the review, per the 2026-10-01 owner directive; an
agent review does not substitute for the owner's human approval.
No evidence report: the task declares ordinary build/test capability only.
