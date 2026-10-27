# Task #2152: the scenario `trigger` record's `effects[]` rows are escaped JSON now

Date: 2026-10-27. Task #2152 "Escape the mission-session capture's JSON
strings through cs_app::json" (PLAYTEST-JSON-ESCAPE-2), the follow-up #1141
filed while escaping the playtest's own lines. Capabilities used: ordinary
build/test only; no original data is read and none is needed. Test prefix:
`accept_scenario_json_escape_`.

## What the task asked measured

The writer in `watch_trigger`
(`crates/cs_app/src/mission_session/scenario.rs`) renders one `effects[]`
row per nap/wake/kill target a completed block's declaration applies. Two
of its three fields were hand-typed JSON:

- `role` was interpolated raw between hand-typed quotes
  (`"role":"{role}"`). The values the workspace produces today are the
  three `&'static str` literals the same loop supplies — `"nap"`, `"wake"`
  and `"kill"` — so the field is a string literal by luck, not by
  construction.
- `state` was `format!("\"{state:?}\"")`, a `{:?}` Debug spelling of
  `cs_script::runtime::ObjectiveLifecycle` with quotes typed around it.
  **Measurement: the enum can never carry a string payload** — its whole
  declaration is `Dormant, Awake, Napping, Done`
  (`crates/cs_script/src/runtime.rs:1100`), so every Debug spelling it can
  produce today is a bare word and the hand-typed quotes happened to be
  correct. Per the task's own instruction the pinning test is the
  acceptance for this field: `accept_scenario_json_escape_todays_rows_
  keep_their_bytes` asserts each variant's rendered row literally, and the
  escaper is what keeps the row correct *by construction* if a variant
  ever grows a payload — Rust writes a payload's Debug as
  `Variant("…\u{1}…")`, and the `\u{1}` spelling is not JSON's, a break
  that would have been silent.

Both fields — and the row's target index — are now rendered by
`mission_session::trigger_effect_row`, which puts every string through
`crate::json::json_string`, the crate's one escaper (#1141).

## Alongside in the same writer

The `trigger` line's other string fields were already escaped, but through
`mission_session::evidence`'s `pub(super)` copy — a second escaper in the
same format call. `member` (a world-member name, original data) and
`player_member` now go through `crate::json::json_string` too, so the one
line uses one escaper. The copies differ only in the `\r`/`\t` spellings
(`\r`/`\t` vs `\u000d`/`\u0009`) and both are valid JSON; no value the
workspace produces carries either byte, so today's rendered bytes are
unchanged.

## Still private copies (out of this task's owner paths, follow-up filed)

The crate still has hand-rolled escapers this task did not touch:
`mission_session::evidence::json_string` (~16 call sites in evidence.rs),
`mission_session::smoke::json_string` (plus its `smoke/` submodule uses),
`playtest::smoke::json_string` and `ui::front_end::window`'s `json_string`.
Routing them through `crate::json` is the consolidation follow-up.
