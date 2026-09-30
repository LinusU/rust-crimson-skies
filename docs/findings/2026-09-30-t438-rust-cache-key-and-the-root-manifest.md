# T438: the `rust-cache` key cannot see a profile, because a profile can only live in a file it never hashes

Date: 2026-09-30. Task: #438 "Make the CI build cache key follow Cargo.toml so
a profile change stops re-uploading the old tree" (`allowProtectedChanges: false`).
Follow-up of #430; see
`docs/findings/2026-09-30-t430-rust-lld-sigbus-in-ci.md`, "The cache key does
not follow the profile". Capabilities used: ordinary build/test only — no
`CS_GAME_DIR`, GPU, audio or network capability was read or claimed, so no
`acceptance.json` is produced. Machine: macOS aarch64, rustc/cargo 1.98.1.

## Status: this is an owner task

**Nothing was changed in this repository.** The whole of task #438 is one
input on one step of `.github/workflows/ci.yml`, and `.github/` is protected
for agents (`AGENTS.md` rule 2; `docs/TASK-SPLITTING.md`; the task's own
`suggested change` is labelled "owner decision; `.github/` is protected for
agents"). The task also names no test prefix and has no production-code
surface outside that file, so there is nothing for an agent to write a test
against; no Rust shim was invented to manufacture one (owner directive,
2026-09-28 audit follow-up). Task #438 is therefore `block_task`ed on the
owner, and this finding is the research the owner needs before editing the
workflow: the mechanism, a correction to the task's premise, and a corrected
version of the line it suggests.

## What the key is actually made of

Read from the `v2` branch of `Swatinem/rust-cache` (the ref `@v2` resolves to),
2026-09-30, commit-agnostic:

* `action.yml`: `shared-key` — "A cache key that is used **instead of** the
  automatic `job`-based key"; `key` — "An **additional** cache key that is added
  **alongside** the automatic `job`-based cache key";
  `add-rust-environment-hash-key` — "a hash of all Cargo.toml/Cargo.lock files,
  rust-toolchain files, and .cargo/config.toml files … Defaults to `true`".
* `src/config.ts` builds the key as
  `[prefix-key]-[shared-key | key + GITHUB_JOB]-{runnerOS}-{runnerArch}-[environment hash]-[lock hash]`,
  and the two hash suffixes are both inside
  `if (core.getInput("add-rust-environment-hash-key") == "true")`.
* Inside that block, the manifests hashed are
  `workspaceMembers.map((member) => path.join(member.path, "Cargo.toml"))`,
  plus `path.join(workspace.root, "Cargo.lock")` (parsed, keeping only packages
  that have a `source` or a `checksum`), plus the raw bytes of any
  `.cargo/config.toml` and `rust-toolchain`/`rust-toolchain.toml` found by
  globbing at the workspace root and under it.
* `src/workspace.ts`: `getWorkspaceMembers()` is
  `cargo metadata --all-features --format-version 1 --no-deps`, and it maps
  `meta.packages` through `path.dirname(pkg.manifest_path)`. So the hashed
  manifests are exactly the `packages` cargo reports — never the workspace root
  manifest, which for a virtual workspace is not a package.
* Normalisation before hashing (`config.ts`): `[package] version` is forced to
  `0.0.0`, and a `path` dependency's `version` and `path` are blanked.

**Correction to the task's premise.** Task #438 says the key covers "the
toolchain and `Cargo.lock`, not on `Cargo.toml`". Member manifests *are*
hashed. The accurate statement is: the key covers every workspace **member**
`Cargo.toml`, the root `Cargo.lock` and the toolchain files — and not the
**root workspace `Cargo.toml`**, which is the one file a profile can live in.

## Why a profile can never reach that key

Two measurements on this machine: the first on a scratch workspace under
`target/`, the second on this one.

**1. Cargo ignores a profile that is not in the root manifest.** A scratch
workspace under `target/` with one member:

| case | `[profile.dev]` in the member manifest | `[profile.dev]` at the root | what rustc was actually given |
|---|---|---|---|
| A | `opt-level = 3` | — | no `-C opt-level=3` (left at the default) |
| B | — | `opt-level = 3` | `-C opt-level=3` |
| C | `debug = 2` | `debug = "line-tables-only"` | `-C debuginfo=line-tables-only` |

In every case cargo printed, before compiling:

```
warning: profiles for the non root package will be ignored, specify profiles at the workspace root:
```

(Read from `cargo build -v`; runs with `env -u CARGO_PROFILE_DEV_DEBUG`, because
the shell's own `CARGO_PROFILE_DEV_DEBUG` would otherwise supply the value and
make the measurement meaningless — see the #430 finding.)

**2. The root manifest is not in the set the action hashes.** The committed root
`Cargo.toml` is a virtual workspace (`[workspace]` with no `[package]`). Run
over it exactly as the action runs cargo:

```sh
cargo metadata --all-features --format-version 1 --no-deps
```

returns 10 packages, none of them at the root. Their `manifest_path`s,
relative to the workspace root — i.e. the ten manifests the action hashes:

```
crates/cs_app/Cargo.toml
crates/cs_assets/Cargo.toml
crates/cs_content/Cargo.toml
crates/cs_formats/Cargo.toml
crates/cs_net/Cargo.toml
crates/cs_script/Cargo.toml
crates/cs_sim/Cargo.toml
crates/cs_types/Cargo.toml
tools/cs_inspect/Cargo.toml
tools/cs_xtask/Cargo.toml
```

plus the root `Cargo.lock`. So the hashed set is: those 10 member manifests, the
lockfile, and the toolchain files. The root manifest is in none of them.

Together: `[profile.*]` is only effective in the root workspace manifest, and
the root workspace manifest is not hashed into the key. A profile change
therefore *cannot* re-key this cache — not by accident of configuration, but
structurally. The same applies to the root manifest's `[workspace.package]` and
`[workspace.dependencies]` tables; dependency changes happen to be covered by
the `Cargo.lock` hash, so the profile (and any future root-only build setting)
is the real exposure.

## What the log in #430 shows, and why

Run **36750916306** (quoted in the #430 finding) reported, for a branch whose
only relevant change was `[profile.dev] debug = "line-tables-only"`:

```
Cache Size: ~2577 MB (2702354412 B)
Cache restored successfully
… 395 crates recompiled into the same target/ …
Cache up-to-date.
```

Every line of that is explained by "the key did not change": the restore
brought back the pre-#430 entry, the rebuild happened on top of the restored
full-DWARF tree, and nothing was re-uploaded. The excerpt quoted in the #430
finding does not include the restore step's `full match:` line, so *how* it
matched is an inference from these lines; the next section measures it.

That inference can be read off the action's own source. `save.ts` begins with
`if (isCacheUpToDate()) { core.info("Cache up-to-date."); return; }`, and
`isCacheUpToDate()` is `core.getState(STATE_CONFIG) === ""`. The restore step
calls `config.saveState()` — which is what clears that check — in exactly two
cases: `No cache found.` (nothing matched) or a *partial* match
(`Restored from cache key "…" full match: false`, after which it also
pre-cleans the target directory). So `Cache up-to-date.` means the save step
declined to run, and of the two ways to reach that, the only one consistent
with a multi-minute `Cache Size:` restore is the **exact** hit: the save step's
`saveCache` also cannot overwrite an existing key.

## Confirmed on a later run

CI run **36762497676**, the push of this finding's own branch (both jobs
green). It is a second, independent observation of the same mechanism, two
commits past the `main` that carries #430:

```
Run Swatinem/rust-cache@v2   Cache Size: ~2577 MB (2702354412 B)
Run Swatinem/rust-cache@v2   Cache restored successfully
Run Swatinem/rust-cache@v2   Restored from cache key "v0-rust-rust-Linux-x64-516c6f62-2429d581" full match: true.
Post Run Swatinem/rust-cache@v2   Cache up-to-date.
```

Three things are now measured rather than inferred:

* **`full match: true` and `Cache up-to-date.` are the same event.** The restore
  step's `saveState()` is not called on an exact match, so the save step's
  guard fires. The control flow read from `save.ts`/`restore.ts` above is
  exactly what the log does.
* **The key is unchanged and has no manifest component.** Read left to right
  it is `v0-rust` (prefix-key) + `rust` (`GITHUB_JOB`, no `key` input) +
  `Linux-x64` + `516c6f62` (environment hash) + `2429d581` (lock hash) — the
  construction in `config.ts`, with nothing between the prefix and the job
  name. That gap is exactly where the `key:` line below would insert one, so
  the first row of the log-reading table is a thing the owner can read, not a
  hope.
* **The saving is still uncollected, and it still costs a full rebuild.** The
  entry is the same 2,702,354,412 B as in run 36750916306 — the pre-#430
  full-DWARF tree is still the one in the cache, so the line-tables-only tree
  #430 built has still never been stored. The `cargo test` step emitted
  **395** `Compiling` lines (`bevy_pbr v0.19.1`, `wgpu v29.0.4`,
  `naga v29.0.4`, `avian3d v0.7.0` among them) and ran **18m24s**
  (19:03:18 → 19:21:42). The 395 is the same count #430 reported, re-measured.

What this run cannot show is the thing the task asks for: with the workflow
unchanged there is no new key, so there is no smaller `Cache Size:` to read
and no run that skips the 395. That is the whole of the remaining gap.

The branch was then rebased onto the `main` that carries #434 and pushed again
as `f31902f`; CI run **36765988179** (green) reports the identical key, the
identical 2,702,354,412 B entry, `full match: true`, `Cache up-to-date.` and
395 `Compiling` lines again. Three runs, one key, one entry, and the smaller
tree still unstored. (The run above is identified by its run id rather than a
head SHA: the rebase rewrote that commit to `7fd75ff` with the same tree, and
the finding's author was the same agent for both — the run ids are what make
the logs citable.)

## The correction to the line #438 suggests

`shared-key` and `key` are not interchangeable, and for this change `key` is
the honest form. From `config.ts`:

```ts
const sharedKey = core.getInput("shared-key");
if (sharedKey) {
  key += `-${sharedKey}`;                 // `key` input and GITHUB_JOB are both skipped
} else {
  const inputKey = core.getInput("key");
  if (inputKey) key += `-${inputKey}`;    // both components are kept
  const job = process.env.GITHUB_JOB;
  if (job && core.getInput("add-job-id-key") == "true") key += `-${job}`;
}
```

The runner OS/arch, the environment hash and the lock hash are appended either
way, so `shared-key` would not lose those. What it does lose is the job
component, and it loses it silently: today there is one `rust` job, so both
forms behave identically, and the day a second job caches `target/` the
`shared-key` form would let those two jobs share one entry. `key` states the
intent — *add* the manifest hash to the key the action already computes.

```diff
       - uses: Swatinem/rust-cache@v2
         if: steps.workspace.outputs.present == 'true'
+        with:
+          key: ${{ hashFiles('**/Cargo.toml') }}
```

`hashFiles('**/Cargo.toml')` matches the root manifest as well as the members: a
leading `**/` matches zero directories as well as many, which is why the
ecosystem's `**/package-lock.json` idiom catches the root lockfile. That idiom
is *not* re-verified here — GitHub's `hashFiles` is implemented runner-side,
not with the `@actions/glob` the action itself uses — which is exactly why the
first row of the log-reading table below is a check to run rather than an
assumption to make: read `.. Prefix:` in the run after the edit and see the
extra component. The member manifests the pattern also matches are already
hashed by the action, so the line adds the root manifest and costs nothing
extra in re-keying. The narrower `hashFiles('Cargo.toml')` would be equivalent
for the profile, and would not depend on the `**/` behaviour at all — which is
the tie-breaker if the owner prefers the least clever expression.

## What the owner should read in the run log

The acceptance criterion for this task is a *measured* run, so this is what to
look at, and what each line can and cannot prove. Nothing here is measured yet;
the first run after the edit is the measurement.

| what to read | expected after the edit | what it proves |
|---|---|---|
| `Cache Configuration` group in the restore step, `.. Prefix:` | today it reads `v0-rust-rust-Linux-x64-…` (measured above); after the edit the manifest hash sits between `v0-rust` and the job name `rust` | the workflow's `key` reached the action's prefix; a miss here means the expression did not evaluate |
| restore step, first line after `... Restoring cache ...` | `No cache found.` — a miss, not `Restored from cache key "…" full match: true` | the new key is genuinely new |
| the `cargo test` step | a full build of the Bevy/Avian graph, no restored `target/` | expected: the fix costs exactly one such run |
| save step, the new `Cache Size:` | a new entry stored, *not* `Cache up-to-date.` | the smaller tree is now what the cache holds |
| a later run's restore `Cache Size:` | the value the run above stored, and the `cargo test` step does **not** recompile the 395-crate graph | the saving is collected, which is the actual goal |

The expected new size is an **estimate with a stated method, not a number to
assert**: the #430 finding measured the `target/` tree shrinking 12.19 GiB →
9.34 GiB (−23.4%) on this machine, and the quoted 2702354412 B covers the whole
entry (`$CARGO_HOME/registry`, `$CARGO_HOME/git`, `$CARGO_HOME/bin` and
`target/`, per `config.ts`'s `cachePaths`), so the saving applies to the
`target/` share of it, not to all of it. A pass/fail threshold should not be
written until that value is measured once.

One consequence worth knowing before scheduling the run: the run that pays for
the fix is the *lowest* disk-pressure run CI can make. It starts with nothing
restored, so it does not also carry the 2.5 GB full-DWARF tree that today's runs
carry and that the remaining rust-lld `SIGBUS` (#430, #439) is sensitive to.

## Options, with verdicts

| option | verdict |
|---|---|
| **`key: ${{ hashFiles('**/Cargo.toml') }}`** on the cache step | **recommended.** One line, additive, re-keys on the one file a profile can be in. Requires `.github/`. |
| `shared-key:` (as #438 suggests) | works today, same effect, but drops the job-id component of the key (see above). Prefer `key:`. |
| `add-rust-environment-hash-key: false` | rejected: it would drop the member-manifest and lock hashes too, so a dependency change would restore an incompatible tree. |
| commit a `.cargo/config.toml` to rotate the key once | possible — the action hashes that file's raw bytes, and a brand-new file is hashed too — so a one-shot rotation needs no `.github/` change. Rejected as a *fix*: it rotates one key, it does not make a future profile edit re-key anything, and it leaves a file in the repository whose only purpose is to change a byte in a cache key. Not done here. |
| `cache-restore`/`cache-save` split (the alternative #438 offers the owner) | more machinery than this needs. Worth it only together with something it enables, e.g. caching `$CARGO_HOME` and `target/` under separate keys so a target wipe does not also drop the registry. Not needed to collect the #430 saving. |
| bump `Swatinem/rust-cache` to a different ref | no effect. Member-only manifest hashing is the documented behaviour of the `v2` line (CHANGELOG 2.7.2, "Only key by `Cargo.toml` and `Cargo.lock` files of workspace members"), so no `v2` ref hashes the root manifest. |

## Limits of what this finding proves

* The mechanism and the `shared-key`/`key` difference are **read from the
  action's source**; what `@v2` resolved to on the #430 run date cannot be
  proven from here, and the finding does not claim it. What the source
  predicts is confirmed by run 36762497676 above.
* The two local measurements (cargo ignoring a non-root profile; the
  `cargo metadata` package list) are macOS/rustc 1.98.1 on this machine. The
  CI runner's own member list is the same committed workspace; run
  36762497676 confirms the key it builds, not the member list, on the runner.
* The 395 crates and the ~19-minute `cargo test` step were re-measured on run
  36762497676 (395 lines, 18m24s); the 2,577 MB entry is that run's own
  `Cache Size:` line. The `du -sk target` ratio behind the size estimate is
  still the #430 finding's local macOS measurement and is *not* a measurement
  of any runner's peak or of any entry's composition.
* No CI run was made *for the fix*, because with `.github/` protected there is
  nothing in it to run. The expected-after-the-edit column of the log-reading
  table is a prediction from the action's source, not an observation.

## Commands run

| command | exit | result |
|---|---|---|
| `cargo metadata --all-features --format-version 1 --no-deps` | 0 | 10 packages, root manifest not among them |
| `cargo build -p member -v` in four scratch workspaces under `target/` | 0 | cargo's "profiles for the non root package will be ignored" warning in all four; a member-only `opt-level = 3` never reached rustc, the same setting at the root did |
| web fetch of `Swatinem/rust-cache` `action.yml`, `CHANGELOG.md`, `src/config.ts`, `src/workspace.ts`, `src/restore.ts`, `src/save.ts` on ref `v2` | 0 | the key construction, the hashed-file set and the `Cache up-to-date.` condition quoted above |
| `cargo fmt --all -- --check` | 0 | clean |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 | clean |
| `cargo test --workspace --locked` | 0 | 156 test binaries, 1,455 tests, 0 failed, 98 ignored |
| `cargo test --workspace --locked -- accept_f00_c_ --include-ignored` | 0 | 16 selected, 16 passed |
| `cargo test --workspace --locked -- accept_t430_ --include-ignored` | 0 | 6 selected, 6 passed |
| CI run 36762497676 (push of this branch) | 0 | `rust` and `pack` both green; cache lines quoted in "Confirmed on a later run" |
| CI run 36765988179 (rebase onto #434, head `f31902f`) | 0 | `rust` and `pack` both green; same key, same entry, 395 `Compiling` lines again |

**No task-prefix selection was run, and none is claimed.** Task #438 names no
test prefix and adds no production code for one to exercise (see "Status"), and
the owner directive forbids inventing a Rust shim to manufacture a prefix. The
two selections in the table are the ones the task's own acceptance criteria
name (`accept_f00_c_*`, the CI-gate guard) and the ones that guard what #430
left in the manifest (`accept_t430_*`). The `accept_f00_c_*` run is reported
for completeness, not as evidence that a gate survived a workflow edit: no
workflow was edited.

## Sources

`https://github.com/Swatinem/rust-cache` at ref `v2`, read 2026-09-30:
`action.yml`, `CHANGELOG.md`, `src/config.ts`, `src/workspace.ts`,
`src/restore.ts`, `src/save.ts`. Run 36750916306 and the `du -sk target`
A/B, both via `docs/findings/2026-09-30-t430-rust-lld-sigbus-in-ci.md`; run
36762497676 read from this repository's own CI. Local `cargo metadata` and the
scratch-workspace `cargo build -v` runs above. No original game data was read;
`CS_GAME_DIR` was not used.
