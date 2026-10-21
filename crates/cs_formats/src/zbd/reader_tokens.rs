//! The reader-archive token stream: the compiled `.zrd` grammar, typed
//! (task #1805, `PARITY2-DMG-FX-A`).
//!
//! A reader archive's `.zrd` members are not text. Each is a sequence of
//! **nodes**, every word a little-endian `u32`:
//!
//! | tag | node | payload |
//! | ---: | --- | --- |
//! | 1 | integer | one `i32` |
//! | 2 | float | one `f32` |
//! | 3 | string | a `u32` byte length, then that many bytes (no NUL) |
//! | 4 | list | a `u32` word `N`, then `N - 1` child nodes |
//!
//! The list rule (children = word − 1) is the one `dzones.zrd` established
//! (`detection_zones`); the float tag is the one `cs_content`'s general
//! decoder already recognises. Measured here: `zbd/zrdr.zbd`'s whole
//! animation vocabulary decodes with this grammar and no byte left over (see
//! `docs/findings/2026-10-21-parity2-dmg-fx-a-effect-definitions.md`).
//!
//! A *record* is written as a flat run of `keyword (values...)` siblings: a
//! string node naming the field, immediately followed by a list node holding
//! its values. [`ReaderNode::fields`] reads a list in that shape.
//!
//! What a keyword *means* to the engine is not decided here; this module only
//! gives bytes a tree. Nothing is `verified_original`.

use std::fmt;

const TAG_INT: u32 = 1;
const TAG_FLOAT: u32 = 2;
const TAG_STRING: u32 = 3;
const TAG_LIST: u32 = 4;

/// The deepest nesting accepted; the measured effect records nest to six.
pub const MAX_READER_DEPTH: u32 = 32;

/// The smallest encoded node: a tag word and one payload word.
const MIN_NODE_BYTES: usize = 8;

/// One decoded node. Strings keep their raw bytes (no UTF-8 assumption).
#[derive(Clone, Debug, PartialEq)]
pub enum ReaderNode<'a> {
    /// Tag 1.
    Int(i32),
    /// Tag 2.
    Float(f32),
    /// Tag 3.
    String(&'a [u8]),
    /// Tag 4: the children, `N - 1` of them.
    List(Vec<ReaderNode<'a>>),
}

impl<'a> ReaderNode<'a> {
    /// The string as text, when this is a string of ASCII.
    pub fn as_str(&self) -> Option<&'a str> {
        match self {
            Self::String(bytes) => std::str::from_utf8(bytes).ok(),
            _ => None,
        }
    }

    /// The list's children, when this is a list.
    pub fn as_list(&self) -> Option<&[ReaderNode<'a>]> {
        match self {
            Self::List(children) => Some(children),
            _ => None,
        }
    }

    /// A number as `f32`: a float, or an integer converted exactly when it fits.
    #[allow(clippy::cast_precision_loss)]
    pub fn as_number(&self) -> Option<f32> {
        match self {
            Self::Float(value) => Some(*value),
            Self::Int(value) => Some(*value as f32),
            _ => None,
        }
    }

    /// The integer, when this is an integer node.
    pub const fn as_int(&self) -> Option<i32> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    /// Reads a list of `keyword (values)` siblings as `(keyword, value-node)`
    /// pairs, where a keyword with no following list pairs with `None`.
    ///
    /// Returns `None` when this is not a list. A string child followed by a
    /// list child is one field; any other child (a bare list, a number) is
    /// returned with the keyword `None` so nothing is dropped.
    pub fn fields(&self) -> Option<Vec<(Option<&'a str>, &ReaderNode<'a>)>> {
        let children = self.as_list()?;
        let mut out = Vec::new();
        let mut index = 0;
        while index < children.len() {
            let node = &children[index];
            if let (Some(name), Some(value @ Self::List(_))) =
                (node.as_str(), children.get(index + 1))
            {
                out.push((Some(name), value));
                index += 2;
            } else {
                out.push((None, node));
                index += 1;
            }
        }
        Some(out)
    }

    /// The first value list stored under `keyword` in this record.
    pub fn field(&self, keyword: &str) -> Option<&ReaderNode<'a>> {
        self.fields()?
            .into_iter()
            .find(|(name, _)| *name == Some(keyword))
            .map(|(_, value)| value)
    }
}

/// Why a token stream could not be decoded; `offset` is the byte at fault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReaderTokenError {
    /// The stream ended inside a node.
    UnexpectedEnd {
        /// Offset of the node that did not fit.
        offset: usize,
    },
    /// A tag word outside 1..=4.
    UnknownTag {
        /// Offset of the tag word.
        offset: usize,
        /// The word found.
        tag: u32,
    },
    /// A list word of 0 (a list always counts itself, so the word is ≥ 1).
    EmptyListWord {
        /// Offset of the list.
        offset: usize,
    },
    /// A declared length or count larger than the bytes left.
    LengthOutOfBounds {
        /// Offset of the node.
        offset: usize,
    },
    /// Nesting beyond [`MAX_READER_DEPTH`].
    TooDeep {
        /// Offset of the list that nests too far.
        offset: usize,
    },
}

impl fmt::Display for ReaderTokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEnd { offset } => write!(f, "stream ends inside the node at {offset}"),
            Self::UnknownTag { offset, tag } => write!(f, "unknown tag {tag} at {offset}"),
            Self::EmptyListWord { offset } => write!(f, "list word 0 at {offset}"),
            Self::LengthOutOfBounds { offset } => {
                write!(f, "declared length exceeds the stream at {offset}")
            }
            Self::TooDeep { offset } => write!(f, "nesting too deep at {offset}"),
        }
    }
}

impl std::error::Error for ReaderTokenError {}

fn word(bytes: &[u8], offset: usize) -> Result<u32, ReaderTokenError> {
    bytes
        .get(offset..offset + 4)
        .and_then(|slice| <[u8; 4]>::try_from(slice).ok())
        .map(u32::from_le_bytes)
        .ok_or(ReaderTokenError::UnexpectedEnd { offset })
}

fn node<'a>(
    bytes: &'a [u8],
    offset: usize,
    depth: u32,
) -> Result<(ReaderNode<'a>, usize), ReaderTokenError> {
    let tag = word(bytes, offset)?;
    let payload = word(bytes, offset + 4)?;
    match tag {
        TAG_INT => Ok((ReaderNode::Int(payload as i32), offset + 8)),
        TAG_FLOAT => Ok((ReaderNode::Float(f32::from_bits(payload)), offset + 8)),
        TAG_STRING => {
            let start = offset + 8;
            let end = start
                .checked_add(payload as usize)
                .filter(|end| *end <= bytes.len())
                .ok_or(ReaderTokenError::LengthOutOfBounds { offset })?;
            Ok((ReaderNode::String(&bytes[start..end]), end))
        }
        TAG_LIST => {
            if depth >= MAX_READER_DEPTH {
                return Err(ReaderTokenError::TooDeep { offset });
            }
            let count = (payload as usize)
                .checked_sub(1)
                .ok_or(ReaderTokenError::EmptyListWord { offset })?;
            // Every child costs at least MIN_NODE_BYTES: refuse a count the
            // remaining bytes cannot hold before allocating for it.
            if count > (bytes.len() - offset - 8) / MIN_NODE_BYTES {
                return Err(ReaderTokenError::LengthOutOfBounds { offset });
            }
            let mut children = Vec::with_capacity(count);
            let mut cursor = offset + 8;
            for _ in 0..count {
                let (child, next) = node(bytes, cursor, depth + 1)?;
                children.push(child);
                cursor = next;
            }
            Ok((ReaderNode::List(children), cursor))
        }
        tag => Err(ReaderTokenError::UnknownTag { offset, tag }),
    }
}

/// Decodes one node at the start of `bytes`; returns it and the bytes it used.
///
/// # Errors
///
/// [`ReaderTokenError`] naming the offset at fault.
pub fn read_reader_node(bytes: &[u8]) -> Result<(ReaderNode<'_>, usize), ReaderTokenError> {
    node(bytes, 0, 0)
}

/// Decodes a whole member as consecutive top-level nodes; no byte may remain.
///
/// # Errors
///
/// [`ReaderTokenError`] naming the offset at fault.
pub fn read_reader_nodes(bytes: &[u8]) -> Result<Vec<ReaderNode<'_>>, ReaderTokenError> {
    let mut out = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let (next, end) = node(bytes, cursor, 0)?;
        out.push(next);
        cursor = end;
    }
    Ok(out)
}
