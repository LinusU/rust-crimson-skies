//! `accept_scenario_json_escape_*` acceptance tests (Rally #2152,
//! PLAYTEST-JSON-ESCAPE-2).
//!
//! The windowed capture's `trigger` record — the line `watch_trigger` writes
//! when a watched block completes — carries an `effects[]` row per nap/wake/
//! kill target the completed block's declaration applies. That row was built
//! with `format!`, interpolating the role between hand-typed quotes with no
//! escape and the lifecycle between its own hand-typed quotes as a `{:?}`
//! Debug spelling, so a `"` in a role or a Rust-style `\u{1}` in a payload
//! would end the string early and the record would stop being JSON.
//!
//! The renderer now routes both strings through the crate's one escaper,
//! `cs_app::json::json_string` (Rally #1141). These tests drive the
//! production row renderer and parse what it produces with a real parser
//! (`serde_json`), so "it looks balanced" is never the acceptance: the
//! object either parses and round-trips or it does not. Nothing needs the
//! installation — the values are supplied here, and no original data is read
//! or committed.
//!
//! What the hostile cases can and cannot reach is measured in
//! `docs/findings/2026-10-27-t2152-scenario-trigger-effects-json-escape.md`:
//! `role` is any `&str` the declaration walk produces, so a hostile spelling
//! is injected straight in; `state` is a `ObjectiveLifecycle`, a unit enum,
//! so no value the workspace holds can produce a Debug spelling that needs
//! escaping — the pinning test is the acceptance for that field, and the
//! escaper keeps the row correct by construction if a variant ever grows a
//! payload.

use cs_app::json::json_string;
use cs_app::mission_session::trigger_effect_row;
use cs_script::runtime::ObjectiveLifecycle;

/// A role holding every character a JSON string cannot carry raw: both
/// string delimiters, the three whitespace controls, a below-`0x20` control
/// byte, DEL and a non-ASCII character that must survive verbatim.
const HOSTILE_ROLE: &str = "wake\"\\ \r\t nl\n ctl\u{1} del\u{7f} caf\u{e9}";

/// **A hostile role survives as one JSON object.**
///
/// The task's own failure mode: the row interpolated `role` between
/// hand-typed quotes, so a `"` in it closed the string early and the
/// `effects[]` row stopped being JSON. The rendered row now has to parse
/// with a real parser and hand the value back unchanged — and removing the
/// escaper from the role field fails this test, because the hostile bytes
/// then end the literal early.
#[test]
fn accept_scenario_json_escape_hostile_role_stays_one_json_object() {
    let rendered = trigger_effect_row(HOSTILE_ROLE, 7, Some(ObjectiveLifecycle::Awake));
    let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap_or_else(|error| {
        panic!("the effect row is not JSON ({error}): {rendered}");
    });
    assert_eq!(parsed["role"].as_str(), Some(HOSTILE_ROLE), "{rendered}");
    assert_eq!(parsed["target"].as_u64(), Some(7), "{rendered}");
    assert_eq!(parsed["state"].as_str(), Some("Awake"), "{rendered}");
}

/// **Every lifecycle the model projects renders one parseable row.**
///
/// `ObjectiveLifecycle` is a unit enum — `Dormant`, `Awake`, `Napping`,
/// `Done` — so the Debug spelling the row carries can never hold a payload
/// (see the findings note). The states the workspace holds today still have
/// to land inside a JSON string, and the absent case still has to be JSON
/// `null`, not the string `"null"` or a missing key.
#[test]
fn accept_scenario_json_escape_every_lifecycle_and_absence_parse() {
    for (state, word) in [
        (ObjectiveLifecycle::Dormant, "Dormant"),
        (ObjectiveLifecycle::Awake, "Awake"),
        (ObjectiveLifecycle::Napping, "Napping"),
        (ObjectiveLifecycle::Done, "Done"),
    ] {
        let rendered = trigger_effect_row("wake", 3, Some(state));
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap_or_else(|error| {
            panic!("the {word} row is not JSON ({error}): {rendered}");
        });
        assert_eq!(parsed["state"].as_str(), Some(word), "{rendered}");
        assert_eq!(parsed["role"].as_str(), Some("wake"), "{rendered}");
        assert_eq!(parsed["target"].as_u64(), Some(3), "{rendered}");
    }

    let rendered = trigger_effect_row("kill", 0, None);
    let parsed: serde_json::Value = serde_json::from_str(&rendered)
        .unwrap_or_else(|error| panic!("the absent-state row is not JSON ({error}): {rendered}"));
    assert_eq!(parsed["state"], serde_json::Value::Null, "{rendered}");
}

/// **The rows the capture writes today render byte for byte what they
/// rendered before.**
///
/// Escaping is a no-op for a value that holds nothing escapable, so the
/// rows the workspace produces — the three literal roles, the four
/// unit-variant spellings, the absent state — have to keep the exact bytes
/// the hand-typed quotes produced. The expected strings below are the
/// renderer's templates as they were written before this task, with the
/// same values interpolated the way they were interpolated then.
#[test]
fn accept_scenario_json_escape_todays_rows_keep_their_bytes() {
    assert_eq!(
        trigger_effect_row("nap", 4, Some(ObjectiveLifecycle::Napping)),
        "{\"role\":\"nap\",\"target\":4,\"state\":\"Napping\"}"
    );
    assert_eq!(
        trigger_effect_row("wake", 2, Some(ObjectiveLifecycle::Awake)),
        "{\"role\":\"wake\",\"target\":2,\"state\":\"Awake\"}"
    );
    assert_eq!(
        trigger_effect_row("kill", 11, Some(ObjectiveLifecycle::Done)),
        "{\"role\":\"kill\",\"target\":11,\"state\":\"Done\"}"
    );
    assert_eq!(
        trigger_effect_row("kill", 11, None),
        "{\"role\":\"kill\",\"target\":11,\"state\":null}"
    );
}

/// **A Debug spelling that carried a payload would still land inside one
/// JSON string.**
///
/// The unit enum can never produce this today — this is the construction
/// the escaper buys, shown concretely rather than assumed: Rust writes a
/// payload-carrying variant's Debug as `Variant("…\u{1}…")`, whose `\u{1}`
/// spelling JSON does not accept, and the escaper is what keeps that row a
/// string anyway.
#[test]
fn accept_scenario_json_escape_a_payload_debug_spelling_stays_a_string() {
    // The spelling Rust's Debug produces for `Special("x\u{1}")`: the brace
    // escape is not JSON's, so only the escaper keeps the literal closed.
    let payload_spelling = "Special(\"x\\u{1}\")";
    let literal = json_string(payload_spelling);
    let parsed = serde_json::from_str::<String>(&literal).unwrap_or_else(|error| {
        panic!("the escaped Debug spelling is not JSON: {literal}: {error}")
    });
    assert_eq!(parsed, payload_spelling);
}
