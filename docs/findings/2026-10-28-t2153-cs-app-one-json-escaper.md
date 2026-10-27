# Task #2153: cs_app has one JSON escaper

Date: 2026-10-28. Task #2153 (JSON-ESCAPE-CONSOLIDATE), the follow-up filed by
`2026-10-27-t2152-scenario-trigger-effects-json-escape.md`. Ordinary
build/test only; no original data is read. Test prefix:
`accept_cs_app_json_consolidate_`.

## What changed

The four private `json_string` copies named by #2152 are removed and their
call sites use `crate::json::json_string`:

- `mission_session/evidence.rs` (the `smoke/` submodules and `smoke.rs` now
  import it from `crate::json` instead of `super::`),
- `mission_session/smoke.rs`,
- `playtest/smoke.rs`,
- `ui/front_end/window.rs`.

`ui/front_end/paths.rs` already delegated and is unchanged. The one
hand-typed quote pair left in a JSON writer, `"archive_sha256": "{}"` in
`write_playback`, now interpolates `json_string(&…to_hex())`; its rendered
bytes are unchanged (a hex digest holds nothing escapable). No sibling in the
module typed quotes by hand.

## Bytes that move

The copies differed from `crate::json` only in `\r` and `\t`: they wrote
`\u000d` and `\u0009`, `crate::json` writes `\r` and `\t`. Both are valid
JSON and decode to the same character. Nothing in the workspace (code, tests,
docs) pins either spelling. The affected artifacts are the mission smoke's and
the playtest smoke's `report.json`, the front-end shell smoke's `report.json`
and the mission capture's `trace.jsonl`/`playback.json`, and only when a
string carries a CR or TAB — no value the workspace produces today does.
