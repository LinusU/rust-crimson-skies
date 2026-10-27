//! One JSON string escaper for every hand-rolled JSON writer in this crate.
//!
//! The playtest's `playtest sources:` and `playtest flight:` stdout lines, the
//! smoke's `report.json`, the evidence harness's `playtest-flight.json` and
//! the front-end review artifacts are all built with `format!`, so a value is
//! only JSON once it is a **string literal**: every character a JSON string
//! cannot hold raw has to be escaped before the value is interpolated, or a
//! `"` inside one original-data name (a node name, an engine name, a texture
//! name) or one host path closes the string early and the whole line stops
//! being JSON at all.
//!
//! Before this module the workspace carried four private copies of that loop
//! (the playtest's `json_escape`, the texture report's `esc`, the front-end
//! review's `json` and `json_string`, plus a fifth one in the evidence
//! harness) and two renderers interpolated without any escape at all — the
//! bug Rally #1141 was opened for. One escaper, called from every writer, is
//! the point: a writer cannot forget it and two writers cannot disagree about
//! what a control byte becomes.
//!
//! Nothing here parses or reorders JSON; this crate writes JSON-shaped strings
//! and the acceptance tests parse them with a real parser
//! (`accept_playtest_json_escape_*`).

/// Escapes `value` for the inside of a JSON string literal.
///
/// The characters RFC 8259 names — `"` and `\` — become `\"` and `\\`, the
/// three whitespace controls become `\n`, `\r` and `\t`, and every other
/// character below `0x20` becomes a `\u00xx` escape in lower-case hex (the
/// spelling the front-end review artifacts were already written in, so their
/// bytes do not move). Everything from `0x20` up is copied verbatim, so a
/// UTF-8 value survives unchanged and its bytes are the same ones that went
/// in.
///
/// A value that holds nothing escapable — every original-data name and path
/// the playtest carries today — therefore round-trips to exactly the bytes it
/// interpolated as before this helper existed.
#[must_use]
pub fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out
}

/// `value` as one complete JSON string literal, both quotes included.
///
/// This is what a writer interpolates where a JSON string belongs: the quotes
/// are part of the contract, so a caller cannot add its own and double-escape
/// the value.
#[must_use]
pub fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    out.push_str(&json_escape(value));
    out.push('"');
    out
}
