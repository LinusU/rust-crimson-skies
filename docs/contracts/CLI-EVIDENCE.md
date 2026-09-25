# CLI and evidence contract

These commands are **interfaces the agent must implement**. They are not implemented by this specification pack.

## Inspector

```sh
cs-inspect inventory --cs-path "$CS_GAME_DIR" --out private/inventory.json
cs-inspect catalog --cs-path "$CS_GAME_DIR" --out private/catalog.json
cs-inspect resolve --cs-path "$CS_GAME_DIR" --world <id> --asset <key> --out private/resolve.json
cs-inspect closure --cs-path "$CS_GAME_DIR" --mission <catalog-id> --strict --out private/closure.json
cs-inspect scripts --cs-path "$CS_GAME_DIR" --coverage --out private/scripts.json
cs-inspect handling --cs-path "$CS_GAME_DIR" --all --out private/handling.json
cs-inspect audit --cs-path "$CS_GAME_DIR" --scope all --strict --out private/audit.json
```

All paths above are examples for an owner-controlled **private output directory**, excluded from Git. `--out` uses atomic writes and reports its final path. Human output goes to stderr when JSON is stdout. Exit 0 means the requested check passed; 2 means invalid input/unsupported content; 3 means failed validation; 4 means missing capability; other nonzero means runtime failure. Never return zero after only logging a failure.

## App

```sh
cs --cs-path "$CS_GAME_DIR" --mission <catalog-id> --profile-dir <private-test-profile-dir>
cs --synthetic --headless --ticks 600 --seed 1 --trace <private-trace.jsonl>
cs --cs-path "$CS_GAME_DIR" --mission <id> --input-replay <private-replay> --headless --ticks 12000 --trace <private-trace.jsonl>
cs --cs-path "$CS_GAME_DIR" --mission <id> --cam=<x,y,z,yaw,pitch> --capture-tick 600 --screenshot <private.png>
```

`--cam` uses canonical world meters, yaw/pitch in **degrees for human CLI only**, and a documented rotation convention. Simulation and internal file APIs use radians. Capture mode records overrides and never changes a production profile by default. Headless simulation and GPU capture are distinct; a GPU capture may use offscreen rendering but cannot be faked by an empty window or static fixture.

### `--seed` (owner ruling, 2026-09-25; proposal in `docs/findings/2026-09-23-t335-synthetic-seed-semantics.md`)

`--seed <u64>` is the **root seed of a run**. It is optional; omitting it keeps today's behavior and today's trace bytes exactly. For now only the synthetic headless scene consumes it: `--seed` without `--synthetic` is `Invalid` until another stage specifies its consumer. Input replays will record the root seed when they exist.

- **Typed input:** `SyntheticRequest.seed: Option<u64>`. `None` runs the canonical `SyntheticBodySpec::falling_box` fixture unchanged.
- **Generator:** SplitMix64, hand-written in `cs_types` (no dependency). Every consumer draws its own domain-separated stream: the stream seed is the SplitMix64 output of `root_seed ^ DOMAIN`, with a documented `u64` domain constant per consumer, so adding a consumer never shifts another consumer's values. The synthetic body is the first domain.
- **Unit floats:** convert a draw to `[0, 1)` as `(x >> 11) as f64 * 2^-53` and to `[-a, a]` as `a * (2u - 1)`. This is exact integer-to-float arithmetic, so the values are bit-identical on every platform.
- **What the synthetic seed varies:** only the lateral components. `position_m[0]` and `position_m[2]` each get an offset in `[-2, 2]` m, and `linear_velocity_m_s[0]` and `linear_velocity_m_s[2]` each get an offset in `[-4, 4]` m/s. Drop height, vertical velocity, extents and body kind stay at the fixture values. The result must pass `SyntheticBodySpec::validate`. These bounds are authored development values, not original-game data.
- **Proof in the trace:** the trace header carries `"seed": <u64>` or `"seed": null`, so every trace states which seed produced it.
- **CLI errors** mirror `--ticks`: a missing value is `Unsupported`, a value that is not a `u64` is `Invalid` naming `--seed`, the later of duplicate flags wins, and `--help` documents the flag.
- **Required tests:** the same seed twice gives identical trace bytes. Two different seeds give different tick-0 samples and diverging traces, so an implementation that parses but ignores the seed fails. No seed gives the unchanged canonical trace. The header carries the seed. `--seed abc` and `--seed` without `--synthetic` are rejected.

## Evidence record minimum

Task/feature/mission id; candidate Git tree hash; engine/toolchain versions; timestamp; command argv and working directory; exit code; installation/canonical-content hash; seed and tick range; enabled overrides/assists/mods; capability class; actual test counts; assertions; artifact paths and SHA-256; unresolved issues; reviewer identity/method. See `schemas/evidence.schema.json`.

Capabilities are `synthetic`, `retail`, `gpu`, `audio`, `network_local`, `network_real`, `human_play`, `human_review`. They are non-interchangeable. File existence does not prove a capability was exercised. A WAV generated without an audio device counts as a decode artifact, not audible playback review.

## Verification levels

`implemented`: candidate code exists. `checked`: external build/test and fresh review passed. `verified_original`: fingerprinted original-data/reference evidence supports the specified behavior. `release_approved`: owner approved the aggregate candidate. Each level stores independent evidence; changing code/content invalidates affected approvals. A Rally merge (green CI plus an agent review) only awards `checked`.

## Negative tests are mandatory

The task-specific test prefix must resolve to at least one real test. Every feature needs failure cases that would catch a plausible shortcut. Tests cannot read a fixture expected value and merely repeat it, test only serialization of constants or assert success from a stubbed runtime. Reviewer checks that code under test is production code and a mutation/removal of the implementation would make the test fail.

## Report production during trusted gates

CI has no original data, GPU, audio device or human, so evidence-bound acceptance is produced and checked locally by the implementing agent and again by the reviewing agent.

For a task with non-synthetic capabilities, the selected acceptance test/probe harness writes `acceptance.json` beneath `CS_EVIDENCE_DIR` (use `private/evidence/<TASK-ID>/`, which is ignored by Git). `CS_CANDIDATE_TREE` is the tree of the commit being tested: `git rev-parse HEAD^{tree}` on a clean checkout. The report must match that exact task/tree; old reports cannot be reused for new code. All referenced artifact files are relative to that private evidence directory and must match SHA-256. The declared required capabilities must be covered, and retail tasks require nonnull installation and content hashes. Report timestamps, test counts and assertions must describe the actual execution. Aggregate tasks link the underlying per-mission/media/network artifacts, not just a summary saying everything passed.

Check the report with `python3 tools/validate_evidence.py private/evidence/<TASK-ID>/acceptance.json --artifact-root private/evidence/<TASK-ID> --require-pass`, then commit a copy of `acceptance.json` (hashes and paths only, never original content) as `docs/findings/evidence/<TASK-ID>.json`. Artifacts themselves (screenshots, captures, extracts) stay in `private/`. The reviewer regenerates the report on the rebased commit and compares it. This structural check is not semantic verification or a security boundary: the reviewer and owner must still inspect the evidence and ensure tests exercise real production code.

Task-test discovery uses the unique prefix: `cargo test --workspace --locked -- <prefix> --include-ignored` must execute and pass at least one test, and each discovered test must pass when run alone with `--exact`. An unrelated test with the prefix embedded in its name cannot rescue an ignored or empty task selection. Expensive original-data acceptance tests are marked `#[ignore = "requires CS_GAME_DIR"]` so CI (which has no original data) skips them, but the implementing and reviewing agents must run them with `--include-ignored`; `#[ignore]` is not a bypass. Keep fast synthetic regression tests unignored so CI runs them.
