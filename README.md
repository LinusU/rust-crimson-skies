# Crimson Skies in Rust

An independent reimplementation of **Crimson Skies** (2000, PC) in Rust, [Bevy](https://bevyengine.org) and [Avian](https://github.com/Jondolf/avian), loading the game data from an original installation that you own.

This repository currently holds the **specification**: no game code exists yet. The work is split into feature and mission tasks that autonomous agents pick up through [Rally](https://github.com/LinusU/rally).

No original assets, executables, extracted scripts, manuals or decompiled code are part of this repository, and none may be added. See [NOTICE.md](NOTICE.md).

## Where things are

| Path | What |
| --- | --- |
| [AGENTS.md](AGENTS.md) | The rules every agent follows. Start here. |
| [docs/00-SCOPE.md](docs/00-SCOPE.md), [docs/01-ARCHITECTURE.md](docs/01-ARCHITECTURE.md) | Product scope and crate architecture. |
| [specs/](specs/README.md) | 65 feature specifications (F00–F64), each in four stages A–D. |
| [missions/](missions/README.md) | 24 mission work orders (M01–M24). |
| [docs/contracts/](docs/contracts/) | Shared contracts: identity, flight physics, scripting, state, UI/network, CLI/evidence. |
| [docs/research/](docs/research/FINDINGS.md) | Research findings, format notes and sources. |
| [docs/TASK-SPLITTING.md](docs/TASK-SPLITTING.md) | How oversized tasks are split. |
| [schemas/](schemas/README.md), [tools/](tools/) | Evidence and binding schemas, evidence validator, synthetic fixture generator. |
| [fixtures/synthetic/](fixtures/synthetic/README.md) | Tiny newly authored test files, not original data. |

## Running agents

Each agent works in its own checkout with the Rally MCP server configured and these environment variables:

```sh
export CS_GAME_DIR="/absolute/path/to/Crimson Skies"   # read-only original installation
export CS_CAPABILITIES="retail,gpu,audio"             # what this machine can really provide
```

One more environment rule (task #383): **`CARGO_TARGET_DIR` must be private
to the checkout.** Leave it unset — Cargo then uses the worktree-local
`target/` — or point it at a directory that names this checkout, e.g.
`export CARGO_TARGET_DIR="$PWD/target"` or
`export CARGO_TARGET_DIR="$PWD/../target/$(basename "$PWD")"`. A
`CARGO_TARGET_DIR` shared between checkouts lets concurrent builds reuse
each other's artifacts, so a `cargo test` result is no longer evidence
about the tree it ran in. `cargo run -p cs_xtask -- verify-target-dir`
reports the directory cargo will really use and fails when it is shared.

[`opencode.json`](opencode.json) configures the `rally` MCP server for opencode from `RALLY_AGENT_TOKEN`.

## License

New code and documentation: see [LICENSE](LICENSE). This does not license the original game.
