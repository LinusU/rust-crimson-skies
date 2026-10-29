//! The AI maneuver catalog, `maneuvers.zrd` (task #2204, `T452-REG-MANEUVER-CHOOSER`).
//!
//! The member lives in the shared reader archive `zbd/zrdr.zbd` and decodes with
//! the [`reader_tokens`](super::reader_tokens) grammar. It is one list whose
//! children alternate a maneuver name (string) and that maneuver's record (a
//! list of `keyword (values...)` siblings). The engine's record constructor
//! `0x45f3e0` looks these keys up on the record: `bias` (float), `natural_touch`
//! (integer), and the presence-only flags `relative`, `nitro` and
//! `autogyro_allowed`, plus `steps`, a list of lists of numbers.
//!
//! **Measured** (`data/owner-static/engine-facts.toml`, `maneuver_chooser.*`):
//! the key names, the defaults a missing key leaves (`natural_touch` `0x63`,
//! flags clear, `bias` zero) and that a flag is read by *presence* of its key,
//! never by its value list. **Not decoded:** what a step row means. The
//! installation writes every step as four numbers; they are kept verbatim and
//! unnamed.

use std::fmt;

use super::reader_tokens::ReaderNode;

/// The value of `natural_touch` when the key is absent (`0x45f400`).
pub const DEFAULT_NATURAL_TOUCH: i32 = 0x63;

/// One maneuver node of `maneuvers.zrd`.
#[derive(Clone, Debug, PartialEq)]
pub struct ManeuverNode<'a> {
    /// The maneuver's name, the string the engine's constructor is paired with.
    pub name: &'a str,
    /// `natural_touch`, or [`DEFAULT_NATURAL_TOUCH`] when absent.
    pub natural_touch: i32,
    /// Whether the node carries a `natural_touch` key.
    pub has_natural_touch: bool,
    /// `bias`, or zero when absent.
    pub bias: f32,
    /// Presence of the `relative` key.
    pub relative: bool,
    /// Presence of the `nitro` key.
    pub nitro: bool,
    /// Presence of the `autogyro_allowed` key.
    pub autogyro_allowed: bool,
    /// The `steps` rows, verbatim: each row's numbers, unnamed.
    pub steps: Vec<Vec<f32>>,
}

/// Why `maneuvers.zrd` could not be lowered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManeuverError {
    /// The member is not one list of `name, record` pairs.
    NotNamedRecords,
    /// A name node is not valid UTF-8.
    NameNotUtf8,
    /// A record key holds a value of the wrong shape.
    BadValue {
        /// The maneuver.
        maneuver: String,
        /// The key.
        key: &'static str,
    },
}

impl fmt::Display for ManeuverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotNamedRecords => {
                f.write_str("maneuvers.zrd is not one list of name/record pairs")
            }
            Self::NameNotUtf8 => f.write_str("a maneuver name is not UTF-8"),
            Self::BadValue { maneuver, key } => {
                write!(
                    f,
                    "maneuver {maneuver}: `{key}` has an unexpected value shape"
                )
            }
        }
    }
}

impl std::error::Error for ManeuverError {}

fn number(node: &ReaderNode<'_>) -> Option<f32> {
    node.as_number()
}

/// Lowers the decoded nodes of `maneuvers.zrd` (as returned by
/// [`read_reader_nodes`](super::read_reader_nodes)) into maneuver nodes, in file
/// order.
///
/// # Errors
///
/// [`ManeuverError`] when the member is not the measured shape.
pub fn read_maneuver_nodes<'a>(
    nodes: &[ReaderNode<'a>],
) -> Result<Vec<ManeuverNode<'a>>, ManeuverError> {
    let [root] = nodes else {
        return Err(ManeuverError::NotNamedRecords);
    };
    let pairs = root.fields().ok_or(ManeuverError::NotNamedRecords)?;
    let mut out = Vec::with_capacity(pairs.len());
    for (name, record) in pairs {
        let name = name.ok_or(ManeuverError::NotNamedRecords)?;
        let bad = |key| ManeuverError::BadValue {
            maneuver: name.to_string(),
            key,
        };
        let first = |key: &str| {
            record
                .field(key)
                .and_then(ReaderNode::as_list)
                .and_then(<[_]>::first)
        };
        let natural_touch = match first("natural_touch") {
            None => None,
            Some(ReaderNode::Int(value)) => Some(*value),
            Some(_) => return Err(bad("natural_touch")),
        };
        let bias = match first("bias") {
            None => 0.0,
            Some(node) => number(node).ok_or_else(|| bad("bias"))?,
        };
        let mut steps = Vec::new();
        if let Some(rows) = record.field("steps").and_then(ReaderNode::as_list) {
            for row in rows {
                let row = row.as_list().ok_or_else(|| bad("steps"))?;
                steps.push(
                    row.iter()
                        .map(|n| number(n).ok_or_else(|| bad("steps")))
                        .collect::<Result<Vec<_>, _>>()?,
                );
            }
        }
        out.push(ManeuverNode {
            name,
            natural_touch: natural_touch.unwrap_or(DEFAULT_NATURAL_TOUCH),
            has_natural_touch: natural_touch.is_some(),
            bias,
            relative: record.field("relative").is_some(),
            nitro: record.field("nitro").is_some(),
            autogyro_allowed: record.field("autogyro_allowed").is_some(),
            steps,
        });
    }
    Ok(out)
}
