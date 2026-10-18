# #1543: `cs_app` as five integration-test binaries, not 249 — and why not one

Date: 2026-10-17. Task: **#1543** "One integration-test binary for cs_app (249
test files)", following `docs/findings/2026-10-17-t1542-one-test-binary-per-crate.md`.
Owner ruling of 2026-10-17 (task note): option (b), a small fixed number of
binaries declared in the crate's manifest; the #336 `__eh_frame` gate stays as
it is. Capabilities used: `retail,gpu,audio`; `CS_GAME_DIR` and
`CS_ENGINE_IMAGE` set; `CS_EVIDENCE_DIR` unset.

## 1. What changed

- `crates/cs_app/Cargo.toml`: `autotests = false`, five `[[test]]` targets
  (`cs_app_tests_a` … `_e`, `tests/suite_<x>.rs`), the allowance
  `[package.metadata.cs_test_layout] test_targets = 5`, and
  `shared_test_sources = ["tests/campaign/common.rs"]`. `cs_xtask` is a
  dev-dependency (the acceptance test calls the guard's library function).
- `tests/suite_a.rs` … `suite_e.rs` declare every former test file and
  directory as a module. The 12 former `tests/<dir>/main.rs` became `mod.rs`
  with `crate::` rewritten to `crate::<dir>::`; `tests/campaign/mod.rs` is the
  module root of `suite_d.rs` only, and the other binaries declare the rest of
  the campaign tree in an inline `mod campaign { … }` so `crate::campaign::…`
  paths resolve everywhere. No test body changed.
- `f64_c_support`, `f64_d_support` and `world/import_retail.rs` are declared
  once (clippy's `duplicate_mod`); `vs_m01_geometry_verdict` sits with `world/`
  because it uses `world::import_retail`. `campaign/common.rs` (217 lines, no
  tests) is compiled by the binaries that use it, hence its
  `#![allow(dead_code)]`: each binary uses a subset.
- `accept_f48_d_crash_recovery_matrix.rs`: the crash-child re-exec used
  `--exact <bare name>`; libtest's exact match is on the full module path, so
  the constant is module-qualified. It was the only test that relied on being
  its own process image by name.
- `tools/cs_xtask/src/test_layout.rs` (owner paths extended by the task note):
  - `test_targets = <n>` is the only way to declare more than one target; the
    guard then requires exactly `n` (`TestTargetCount` now carries `allowed`).
  - A file reached by two targets is a `DoublyReachedTestFile` finding.
  - **Extension beyond the ruling, for the reviewer:** `shared_test_sources`
    lists test-free helper files that several targets may compile; the guard
    still fails if such a file holds a `#[test]` (`SharedTestSourceHasTests`),
    and an unlisted shared file is still a double-reach failure. Needed
    because `campaign/common.rs` is imported by 33 modules that cannot all sit
    in one binary (section 3). Without it the only alternative was copying the
    helper.
  - Bug fix kept from the first attempt: `#[path]` on a plain file module
    resolves against the declaring file's directory, not `<dir>/<stem>`.
  - Tests: `accept_test_bin_guard_allows_a_declared_number_of_targets_and_no_file_twice`
    (clean split, file reached twice, stray file, wrong count, shared helper
    ok, shared helper with a test refused) and
    `accept_test_bin_guard_resolves_path_attributes_beside_a_plain_file`.
- `tools/cs_xtask/src/main.rs`: `test-layout` prints the target count.
- `tests/accept_test_bin_cs_app.rs`: `accept_test_bin_cs_app_*` (2 tests) ask
  the guard that cs_app is migrated, declares exactly its five targets, and
  reaches every `.rs` file once.

## 2. Test names, before and after

`cargo test -p cs_app --locked --tests -- --list --include-ignored`: 2923
tests = 178 library unit tests + **2745** integration tests. origin/main's
`tests/` holds **2743** `#[test]` functions in 249 files (grep over its
sources), plus the 2 new `accept_test_bin_cs_app_*`: the lists match. No name
is duplicated across the five binaries. (The old 249-binary layout itself was
not run: 249 fresh Bevy links with a ~1 min macOS scan each is the cost this
task removes.) Per binary, non-ignored run: a 868, b 260, c 504, d 407, e 11
passed (0 failed); ignored 246 / 54 / 62 / 323 / 10.

## 3. Why five, and why not "about 12 MiB" each

`docs/findings/2026-09-23-t336-eh-frame-unwind-table.md`: a macOS image whose
`__eh_frame` passes 16 MiB (16,777,216 B) gets corrupt compact-unwind entries.
`accept_t336_linked_images_keep_eh_frame_under_the_encoding_limit` guards the
test binary itself and is unchanged.

The first attempt (one binary) measured 24.4 MB, and `[profile.dev.package.cs_app]`
`opt-level` 1/3/s only reached 17.4-17.6 MB. Splitting by line count then did
**not** work either: three binaries of ~110k lines each measured 18.9 / 18.3 /
18.5 MB. The reason is that a test binary's `__eh_frame` is almost all
*dependency* code its tests reach (Bevy, avian, `cs_app`'s own library), not
the test code: `nm` shows ~3 MB of ~95 MB of text belongs to the test crate.
An empty binary measures 0.1 MB, a 533-line `hud` suite 5.7 MB, a 3.5k-line
F64 group 6.1 MB, and a single 445-line file, `campaign/vs_m01_runtime.rs`,
adds ~5 MB to any binary it joins. The reached code overlaps between suites,
so a union of suites is far below the sum of its parts (everything: 24.4 MB)
but is set by *which* suites share a binary, not by how many lines.

So the grouping is by measurement: each suite group was built alone, then packed
first-fit under a 15.0 MB budget by building each candidate union (≈ 90
builds); dependency-coupled groups (`campaign/` modules that import each other,
the F64 supports with `ui/`, `world/` with `vs_m01_geometry_verdict`) stay
together. Measured `__eh_frame` of the final binaries (`size -m`, macOS
aarch64, default `test` profile):

| binary | contents | `__eh_frame` |
| --- | --- | --- |
| `cs_app_tests_a` | `accept_f25`-`f29`, `f40`-`f44`, `f47`-`f49`, `f53`-`f59`, all 44 `accept_m01_*`, `accept_t*`, the other 46 `evidence_report_*` (m01, t463-t542, retail), `environment/`, `flight/`, and 40 campaign modules (the M01 play/window families, `m02`-`m05`, …) | 15,226,348 B (14.52 MiB) |
| `cs_app_tests_b` | `accept_f20`, `f22`, `f24`, 5 `playtest_*` files, `render/` | 14,852,400 B (14.16 MiB) |
| `cs_app_tests_c` | `accept_f00`, `f16`, `f17`, `f60`, `f64`, `evidence_report_f64_*`, `world/` (+ `vs_m01_geometry_verdict`), `ui/`, `camera/`, `text/`, `accessibility/`, `scrapbook/`, the F64 supports | 15,343,804 B (14.63 MiB) |
| `cs_app_tests_d` | `accept_f30`-`f36`, `f39`, 13 `evidence_report_*` (f18-f21, f33, f39, flight), `physics/`, `hud/`, and the campaign module root (`m06`-`m24`, `f50`, `evidence/`, …) | 13,441,964 B (12.82 MiB) |
| `cs_app_tests_e` | the `vs_m01_*` runtime-host suites incl. `vs_m01_runtime`, `m01_play_world_evidence` | 15,394,972 B (14.68 MiB) |

All five are under the 16 MiB limit by 1.2-3.5 MiB (re-measured after the final rebase onto main, which added `m01_window_world_*`, `regenerate`, `accept_m01_music_restore` and `accept_m01_smoke_no_teleport`; the ignored-set runs in section 5 were taken just before it). The owner's aim of about
12 MiB each is not reachable: the lightest non-trivial binary is already ~5.5
MB and any binary that runs the app costs 11-12 MB before its own tests. 15.0
MB was the budget; a smaller one would add binaries without making any of the
heavy ones lighter. `cs_xtask test-layout` keeps the count honest, but
nothing automatically watches the sizes: the #336 gate is the safety net, and
a suite added to a binary near the limit will trip it there. Add new suites to
`cs_app_tests_d` (13.1 MB), the lightest.

Before/after binary count: 261 test executables (249 + the 12 directory
targets, per the task title) → **5**. cs_app's library and `cs` binary are
unchanged.

## 4. Runs (host: macOS aarch64)

- `cargo test --locked -p cs_app --no-fail-fast` (no ignored tests), warm
  build: exit 0 in 2 min 25 s wall (a 44.7 s, b 10.6 s, c 27.2 s, d 40.3 s, e
  0.0 s of test time; the rest is the first launch of each fresh binary).
  This includes the t336 gate and the f48_d crash re-exec test, both green in
  the shared process.
- Incremental builds: all five test binaries 18-25 s after a one-file change
  to a test module; cold build of all five ~3 min on a quiet host. The
  single-binary attempt took 35-55 min cold on a busy host and was never
  linkable under the limit.
- `cargo test --locked -p cs_xtask -- accept_test_bin`: 11 passed.

## 5. The ignored (retail) set, `--include-ignored`, `CS_GAME_DIR` + `CS_ENGINE_IMAGE` set, `CS_EVIDENCE_DIR` unset

Each binary run as `cargo test --locked -p cs_app --test cs_app_tests_<x> --
--include-ignored` (the first one as part of `cargo test -p cs_app`, then cut
over to per-binary runs to save wall time on a host at load average ~26). All
exit codes are 101 (a failing test), and every failure is accounted for:

| binary | passed | failed | wall (loaded host) | the failures |
| --- | --- | --- | --- | --- |
| a | 1067 | 47 | 11,542 s | 46 `evidence_report_*` (`CS_EVIDENCE_DIR is not set`) + 1 audio-loop assertion (below) |
| b | 307 | 7 | 1,950 s | 7 `evidence_report_*` (`CS_EVIDENCE_DIR is not set`) |
| c | 556 | 10 | 1,889 s | 9 `evidence_report_*` + `world::audit::accept_f18_d_a_gpu_capture_proves_the_stored_geometry_was_drawn` (needs the `private/evidence/F18-D/` directory the evidence sequence creates: `No such file or directory`) |
| d | 653 | 77 | 2,723 s | 75 `evidence_report_*` + `regenerate_m01_b_…` and `regenerate_m06_b_fu4_…` (they assert `CS_REGENERATE_*=1` is set) |
| e | 19 | 2 | 2,085 s | 2 `evidence_report_*` |

That is 2602 passed and 143 failed of 2745, none of them a layout effect: 139
need `CS_EVIDENCE_DIR`, 2 need `CS_REGENERATE_*`, 1 needs the evidence
directory, 1 is the audio assertion, and the t336 `__eh_frame` gate, which
failed in the single-binary attempt, **passes** in all five. The windowed
mission capture (`accept_m01_play_world_windowed_mission_capture_draws_world_pixels`),
unattributed in the first attempt, passes here (this Mac has a window and a GPU).

**Audio assertion.** `campaign::vs_m01_rt_evidence_audio::accept_vs_m01_runtime_evidence_audio_the_launch_plays_the_engine_loop_through_the_device`
fails with `exactly the engine loop is bound` (`left: 9, right: 1`) both inside
binary a and **alone** (`0 passed; 1 failed`), so shared process state is not the
cause and the split does not change it. Its body is untouched; the likely cause
(not verified in the old layout) is the M01 music work that landed after the
test (`3e21ec9f`, `186bef9f`, `86b91f88`, `ef91823e`) binding more loops in the
same composition. Filed as Rally #1579.

**Process-global state.** The tests that relied on their own process (env vars,
current directory, one Bevy `App`, GPU/audio singletons, the f48_d re-exec)
all passed in one process per binary; no `serial` lock was needed beyond the
f48_d re-exec fix in section 1. The 8-thread parallel run of the retail tests is
the slow part: each fingerprints the whole installation (SHA-256, unoptimised
`cs_assets`), which dominated the 3 h wall time of binary a on a loaded host.

Not measured: the old 249-binary layout's compile and test time (see section 2).
