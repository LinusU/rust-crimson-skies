# F48-XPLAT: the crash/recovery matrix on Linux x86-64, and what Windows still needs

Date: 2026-10-01. Task: F48-XPLAT "Run the F48-D crash/recovery matrix on Linux
and Windows and record what each platform does", the cross-platform remainder
of `### F48-D` in `specs/F48-profiles-saves-settings-migration-and-recovery.md`
(contract `docs/contracts/STATE-TRANSACTIONS.md`, "Persistence"). The matrix
itself and its macOS arm64 run are recorded in
`2026-10-01-f48-d-crash-recovery-matrix.md`; this file records the same suite
on a second platform and states plainly what the third still needs.

Capabilities used: ordinary build/test on this machine, plus the project's own
GitHub CI runners. No `retail` data, no original files. Every byte the tests
write is newly authored synthetic data under a temporary directory.

## What ran where

| Platform | How it ran | Result |
| --- | --- | --- |
| macOS arm64 (APFS) | natively, `cargo test` on this machine | 11/11 cs_content tests + 7/7 cs_app tests pass; 30/30 matrix rows whole |
| Linux x86-64 (btrfs, then overlayfs) | `docker run --platform=linux/amd64`, `rust:bookworm` (digest `sha256:93ce27a8…83971e`), toolchain `1.98.1-x86_64-unknown-linux-gnu` from `rust-toolchain.toml`, OrbStack VM kernel `7.0.14-orbstack-00380`, `uname -m` inside the container reports `x86_64`, system libs as CI installs them | 12/12 cs_content tests and 7/7 cs_app tests pass on btrfs; cs_content also clean on overlayfs; 30/30 matrix rows whole |
| Linux x86-64 (GitHub runner `ubuntu-24.04`, ext4) | the project's own CI, run `36861865689` on the F48-D branch push | all 18 `accept_f48_d_` tests pass (`cargo test --workspace`); the row table is not in the log because `cargo` captures a passing test's stdout |
| Windows x86-64 | **did not run** — no Windows host exists in this environment | see "Not run: Windows" |

Two caveats apply to the containerised Linux row and are recorded rather than
smoothed over:

- The container shares the OrbStack VM's kernel, which reports `x86_64` to the
  container but runs on arm64 hardware underneath (x86-64 userland via
  Rosetta). Filesystem semantics — `rename(2)`, `fsync`, directory sync — are
  the kernel's and do not differ by userspace architecture, so the result is a
  genuine *Linux* result. The GitHub Actions `ubuntu-24.04` run above is the
  corroborating run on real x86-64 hardware, and it agrees.
- `/matrix-tmp` is a docker volume backed by btrfs on the VM's disk
  (`/dev/vdb1`, `nodatacow`); `/tmp` is overlayfs over the same disk. The
  matrix is identical on both. ext4 specifically is covered by the GitHub
  runner, whose workspace and `$TMPDIR` sit on ext4.

`std::env::consts::OS` reported by the matrix test: `macos`, `linux`
(asserted equal to the compiled platform, never a claim written into the
report by hand). `replacement_semantics()`: `rename(2)` on both. 
`directory_sync_supported()`: `true` on both — Linux is unix, so `SyncDir` is
a real directory `fsync` there, same as macOS.

## The full row table on Linux x86-64

Verbatim from `accept_f48_d_the_matrix_runs_and_reports_this_platform
--nocapture`, run on the btrfs volume (`TMPDIR=/matrix-tmp`). Byte-identical
to the overlayfs run and to the macOS run:

```
save writes use rename(2) replacement; directory sync is supported
F48-D crash/recovery matrix on linux: 30 rows, 12 recovered from profile.sav, 18 through a reported fallback, 0 lost
  one-revision             WriteTemp        before -> Some(1) from Some(Current) [profile.tmp did not decode and was ignored: save has no trailing checksum line]
  one-revision             WriteTemp        after  -> Some(2) from Some(Temp) [the newest state was recovered from profile.tmp instead of profile.sav]
  one-revision             SyncTemp         before -> Some(2) from Some(Temp) [the newest state was recovered from profile.tmp instead of profile.sav]
  one-revision             SyncTemp         after  -> Some(2) from Some(Temp) [the newest state was recovered from profile.tmp instead of profile.sav]
  one-revision             RotateBackup     before -> Some(2) from Some(Temp) [the newest state was recovered from profile.tmp instead of profile.sav]
  one-revision             RotateBackup     after  -> Some(2) from Some(Temp) [the newest state was recovered from profile.tmp instead of profile.sav]
  one-revision             InstallCurrent   before -> Some(2) from Some(Temp) [the newest state was recovered from profile.tmp instead of profile.sav]
  one-revision             InstallCurrent   after  -> Some(2) from Some(Current)
  one-revision             SyncDir          before -> Some(2) from Some(Current)
  one-revision             SyncDir          after  -> Some(2) from Some(Current)
  current+backup           WriteTemp        before -> Some(2) from Some(Current) [profile.tmp did not decode and was ignored: save has no trailing checksum line]
  current+backup           WriteTemp        after  -> Some(3) from Some(Temp) [the newest state was recovered from profile.tmp instead of profile.sav]
  current+backup           SyncTemp         before -> Some(3) from Some(Temp) [the newest state was recovered from profile.tmp instead of profile.sav]
  current+backup           SyncTemp         after  -> Some(3) from Some(Temp) [the newest state was recovered from profile.tmp instead of profile.sav]
  current+backup           RotateBackup     before -> Some(3) from Some(Temp) [the newest state was recovered from profile.tmp instead of profile.sav]
  current+backup           RotateBackup     after  -> Some(3) from Some(Temp) [the newest state was recovered from profile.tmp instead of profile.sav]
  current+backup           InstallCurrent   before -> Some(3) from Some(Temp) [the newest state was recovered from profile.tmp instead of profile.sav]
  current+backup           InstallCurrent   after  -> Some(3) from Some(Current)
  current+backup           SyncDir          before -> Some(3) from Some(Current)
  current+backup           SyncDir          after  -> Some(3) from Some(Current)
  newer-uninstalled-temp   WriteTemp        before -> Some(3) from Some(Current) [profile.tmp did not decode and was ignored: save has no trailing checksum line]
  newer-uninstalled-temp   WriteTemp        after  -> Some(4) from Some(Temp) [the newest state was recovered from profile.tmp instead of profile.sav]
  newer-uninstalled-temp   SyncTemp         before -> Some(4) from Some(Temp) [the newest state was recovered from profile.tmp instead of profile.sav]
  newer-uninstalled-temp   SyncTemp         after  -> Some(4) from Some(Temp) [the newest state was recovered from profile.tmp instead of profile.sav]
  newer-uninstalled-temp   RotateBackup     before -> Some(3) from Some(Temp) [the newest state was recovered from profile.tmp instead of profile.sav]
  newer-uninstalled-temp   RotateBackup     after  -> Some(3) from Some(Temp) [the newest state was recovered from profile.tmp instead of profile.sav]
  newer-uninstalled-temp   InstallCurrent   before -> Some(3) from Some(Temp) [the newest state was recovered from profile.tmp instead of profile.sav]
  newer-uninstalled-temp   InstallCurrent   after  -> Some(3) from Some(Current)
  newer-uninstalled-temp   SyncDir          before -> Some(3) from Some(Current)
  newer-uninstalled-temp   SyncDir          after  -> Some(3) from Some(Current)
```

## Rows the matrix did not cover, now covered

The task named three per-platform cases no test exercised. They are now
`crates/cs_content/tests/accept_f48_d_platform_semantics.rs`, written so the
assertions split on `cfg!(unix)` and the Windows arm encodes the documented
`MoveFileExW` behaviour — a run on Windows confirms it or produces the
finding, which is the point. Measured on macOS arm64 and Linux x86-64:

- **`rename` over a read-only destination** — `Replaced` on both unixes
  (POSIX charges the rename to the directory's permissions, not the file's).
  The test makes `profile.bak` read-only — the *destination* of
  `rotate_backup`'s replace is the discriminating case; a read-only
  `profile.sav` alone does not discriminate because it is only ever the
  rename's source and is vacated before `install_current` uses its name. On
  Windows, `MoveFileExW` is documented to refuse a read-only destination, so
  the test asserts `Refused` there: the commit errors and the offered revision
  is still recoverable whole from the uninstalled `profile.tmp`.
- **`rename` over a destination held open** — `Replaced` on both unixes, and
  the held handle keeps reading the replaced inode (revision 1) while the
  directory names revisions 2 and 3. On Windows the expected result is also
  `Replaced`: `std::fs::File::open` requests `FILE_SHARE_DELETE`, so a std
  handle does not block the replace. The genuinely different Windows case — a
  destination open *without* delete sharing, e.g. another process holding the
  save — cannot be produced through portable `std` calls and is recorded as
  unmeasured, not asserted.
- **directory-handle sync** — `File::open(dir).and_then(sync_all)` returns
  `Ok(())` on both unixes, and `DirStorage::sync_dir()` returns `Ok` as the
  production contract requires. Off unix `File::open` on a directory is
  expected to be refused (`std` does not pass `FILE_FLAG_BACKUP_SEMANTICS`),
  which is exactly why the phase is a compiled no-op there; the test prints
  the raw probe's result so the first Windows run records which it was.

One platform defect was found and fixed in owner paths: the matrix test bound
`canary_before` outside its `#[cfg(unix)]` block, so `cargo check
--target x86_64-pc-windows-msvc` reported an unused variable — a hard failure
under `cargo clippy -D warnings` on any Windows build. The binding is now
`#[cfg(unix)]` like the assertion that uses it. `cargo check -p cs_content
--all-targets --locked --target x86_64-pc-windows-msvc` is now clean.

## Not run: Windows x86-64

No Windows host, VM image, or emulation chain exists in this environment, and
none can be provisioned here: QEMU/UTM/VMware/Parallels are absent, installing
a hypervisor plus a Windows ISO is outside what an agent may do to this
machine, and adding a `windows-latest` job to `.github/workflows/ci.yml` is a
protected path requiring an explicit owner-authorized change. What is needed
to close this row:

1. a Windows x86-64 machine (bare metal or a VM with a controllable
   filesystem), or
2. an owner-authorized addition of a Windows runner to CI.

Until then: `cargo check -p cs_content --all-targets --target
x86_64-pc-windows-msvc` proves the write-path crate and its tests compile for
Windows; the `cfg!(windows)` arms in the new test file encode the documented
expectations so the first real run is a measurement, not an exploration; and
`replacement_semantics()` still *reports* `MoveFileExW` without anything
having measured it. `cargo check` of `cs_app` for the same target cannot run
on macOS at all: `blake3`'s build script shells out to `ml64.exe` (the MSVC
assembler), which does not exist off Windows — a cross-check toolchain
limitation, not a code finding.

## Not measured: durability across a real power loss

Same status as the macOS run, restated because it bounds every row above: a
process kill cannot discriminate a synced write from an unsynced one — the
bytes sit in the page cache either way. Neither this machine's laptop
filesystem nor the OrbStack VM's `docker stop` is a power cut to the physical
disk, so nothing here records a durability claim. What is needed: bare metal
or a VM whose storage power can actually be cut (a hardware power harness, or
a VM configured for write-through storage that is hard-killed), running the
matrix twice — once with `sync_temp` and once without — and reporting which
temp files survive each way.

## Difference summary

No row behaved differently between macOS arm64 and Linux x86-64: the table,
the 12/18 test verdicts, and the three platform-semantics cases are identical
on APFS, btrfs, overlayfs and the CI runner's ext4. That is a result — the
write path's behaviour is POSIX-level identical across the unixes — not an
absence of measurement. The differences the contract warns about
(`MoveFileExW` refusing read-only or foreign-held destinations, `SyncDir`
being a no-op) are all expected on Windows, which remains unrun.
