# VS-M01-RT-MISSION-HOST (#1217): integration check

The three subtasks (#1278 composed per-tick entry, #1279 terminal outcome and
exit mapping, #1280 restart) are merged. This note records the integration run
of the acceptance selection against the merged tree.

`cargo test -p cs_app --locked --test campaign -- accept_vs_m01_runtime_host_ --include-ignored`
with `CS_GAME_DIR`, `CS_ENGINE_IMAGE` and `CS_CAPABILITIES=retail,gpu,audio`
set: 11 tests passed, 0 failed (about 9 minutes, dominated by the three retail
tests that run M01's real program).

The selection must be run per package: a workspace-wide run of the prefix
selection with the retail tests included takes longer than a 10 minute tool
timeout, and a killed run prints no test lines.

## Known limit (unchanged, owned elsewhere)

M01's real program does not reach a terminal headlessly: both `Finish` blocks
(23 Succeeded, 39 Failed) start dormant behind the -1 sentinel and every wake
path needs world facts a headless run does not have. The retail tests assert
that no terminal is settled and no exit is written; terminal behaviour is
proven on a synthetic stage. Controlled success and failure runs through real
controls belong to VS-M01-CONTROLLED-RUNS (#360).
