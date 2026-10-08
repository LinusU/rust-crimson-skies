//! The mission-scoped placement member, `placezeps.zrd` (task #791).
//!
//! M01's reader archive `ZBD/c1c/m01/zrdr.zbd` carries a `placezeps.zrd` member
//! (1 535 bytes) that is **not** a sibling of `zeppelins.zrd`'s record layout:
//! it is an `ANIMATION_DEFINITIONS` document whose `ON_STARTUP` definitions each
//! hold one `SEQUENCE_DEFINITION` of state statements. The placement is carried
//! by two of them:
//!
//! * `OBJECT_TRANSLATE_STATE` — a `NAME` (the node) and a `STATE` of three
//!   numbers;
//! * `OBJECT_ROTATE_STATE` — a `NAME` and a `STATE` of three numbers.
//!
//! # The measured grammar
//!
//! Every word is a little-endian `u32`, and a node is one of tag `1` (integer),
//! `2` (float), `3` (string: length and bytes) or `4` (list: word `N`, then
//! `N - 1` children) — the same `.zrd` rule [`super::zeppelins`] uses. The
//! member is `list(list("ANIMATION_DEFINITIONS", list("ANIMATION_LIST", ...)))`
//! and every definition is a keyed list: a text key and its list value.
//!
//! # What the executable does with a state statement
//!
//! Read from the owner-supplied decrypted image (sha-256 `43540fc9…6c37d75`,
//! image base `0x400000`; see `docs/findings/2026-10-08-m01-lc-placezeps-fields.md`
//! for every address):
//!
//! * the statement parser for `OBJECT_TRANSLATE_STATE` is `0x5075b0` and for
//!   `OBJECT_ROTATE_STATE` is `0x505ff0`; each builds a `0x20`-byte event record
//!   (opcode byte `7` and `9`);
//! * a `STATE` element of tag `2` is stored as the float; a tag `1` element is
//!   read as a **signed** 32-bit integer and converted to `f32` (`fild`); any
//!   other tag is an error ([`StateNumber`]);
//! * the three numbers land at event `+0x10`, `+0x14`, `+0x18`; the rotate
//!   parser multiplies each by the `f64` constant at `0x6040e8`
//!   (`0.01745329251994`, degrees to radians) before storing it;
//! * `NAME` is resolved to a node index stored at event `+0x1c`.
//!
//! # What stays unknown
//!
//! Which rotation component is the heading, how a node's translated/rotated
//! state composes into the pose the world draws, what `RESET_TIME` (stored
//! `0xffffffff`) means, and every key outside the measured vocabulary. Each is
//! an explicit [`UnknownField`] under a claim id; nothing is ignored.

use std::fmt;

/// The member name the mission reader archives carry this document under.
pub const PLACEZEPS_MEMBER: &str = "placezeps.zrd";

/// The root record every definition member opens with.
pub const ANIMATION_DEFINITIONS_KEY: &str = "ANIMATION_DEFINITIONS";

/// The key of the definitions list inside the root record.
pub const ANIMATION_LIST_KEY: &str = "ANIMATION_LIST";

/// The key of one definition inside the list.
pub const ANIMATION_DEFINITION_KEY: &str = "ANIMATION_DEFINITION";

/// The claim a key outside the measured vocabulary is recorded under.
pub const UNMODELLED_FIELD_CLAIM: &str = "f20-anim.placezeps-field-unmodelled";

/// The claim under which `RESET_TIME`'s meaning stays unknown.
pub const RESET_TIME_CLAIM: &str = "f20-anim.placezeps-reset-time-unmeasured";

/// The `f64` the executable multiplies each `OBJECT_ROTATE_STATE` number by
/// (image `0x6040e8`), degrees to radians.
pub const DEGREES_TO_RADIANS: f64 = 0.017_453_292_519_94;

const TAG_INT: u32 = 1;
const TAG_FLOAT: u32 = 2;
const TAG_TEXT: u32 = 3;
const TAG_LIST: u32 = 4;
const MAX_DEPTH: u32 = 12;
const MIN_NODE_BYTES: usize = 8;

/// Why a `placezeps.zrd` member was refused. Every variant names the byte
/// offset, relative to the member, where the refusal was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlacezepsError {
    /// A word or string body ran past the end of the member.
    Truncated {
        /// Where the missing bytes were needed.
        offset: u64,
    },
    /// A tag outside `1`, `2`, `3` and `4`.
    UndefinedTag {
        /// Where the tag word is.
        offset: u64,
        /// The tag found.
        tag: u32,
    },
    /// A declared length cannot fit in the bytes left.
    LengthDoesNotFit {
        /// Where the node starts.
        offset: u64,
        /// The declared length word.
        declared: u32,
    },
    /// A list word of `0`, which would mean `-1` children.
    ZeroListWord {
        /// Where the node starts.
        offset: u64,
    },
    /// A string that is not UTF-8.
    InvalidText {
        /// Where the string node starts.
        offset: u64,
    },
    /// Lists nested deeper than the measured shape.
    DepthExceeded {
        /// Where the too-deep node starts.
        offset: u64,
    },
    /// Bytes remain after the root node.
    TrailingBytes {
        /// Where the first unread byte is.
        offset: u64,
        /// How many bytes remain.
        count: u64,
    },
    /// The member is not the `ANIMATION_DEFINITIONS` frame.
    NotAMember {
        /// Where the frame was found wanting.
        offset: u64,
        /// What was expected.
        reason: &'static str,
    },
    /// A key's value is not the measured shape.
    WrongShape {
        /// Where the value starts.
        offset: u64,
        /// The key.
        key: String,
        /// The shape expected.
        expected: &'static str,
    },
    /// A key is spelled twice in one keyed list.
    DuplicateKey {
        /// Where the second spelling is.
        offset: u64,
        /// The key.
        key: String,
    },
    /// A required key is absent.
    MissingKey {
        /// Where the keyed list starts.
        offset: u64,
        /// The key.
        key: &'static str,
    },
}

impl fmt::Display for PlacezepsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { offset } => write!(f, "truncated at byte {offset}"),
            Self::UndefinedTag { offset, tag } => {
                write!(f, "undefined tag {tag} at byte {offset}")
            }
            Self::LengthDoesNotFit { offset, declared } => {
                write!(
                    f,
                    "declared length {declared} at byte {offset} does not fit"
                )
            }
            Self::ZeroListWord { offset } => write!(f, "list word 0 at byte {offset}"),
            Self::InvalidText { offset } => write!(f, "non-UTF-8 string at byte {offset}"),
            Self::DepthExceeded { offset } => write!(f, "nesting too deep at byte {offset}"),
            Self::TrailingBytes { offset, count } => {
                write!(f, "{count} trailing bytes from byte {offset}")
            }
            Self::NotAMember { offset, reason } => {
                write!(f, "not a member at byte {offset}: {reason}")
            }
            Self::WrongShape {
                offset,
                key,
                expected,
            } => write!(f, "{key} at byte {offset} is not {expected}"),
            Self::DuplicateKey { offset, key } => {
                write!(f, "{key} at byte {offset} is spelled twice")
            }
            Self::MissingKey { offset, key } => {
                write!(f, "the list at byte {offset} carries no {key}")
            }
        }
    }
}

impl std::error::Error for PlacezepsError {}

/// A byte range of the member, `[start, end)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemberRange {
    /// First byte, relative to the member.
    pub start: u64,
    /// One past the last byte.
    pub end: u64,
}

/// One number of a `STATE` list as stored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StateNumber {
    /// Tag `1`: the stored word read as a signed 32-bit integer (the image
    /// converts it with `fild`).
    Int(i32),
    /// Tag `2`: the stored `f32`.
    Float(f32),
}

impl StateNumber {
    /// The value the executable keeps: an integer converted to `f32`, a float
    /// as is.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub const fn value(self) -> f32 {
        match self {
            Self::Int(value) => value as f32,
            Self::Float(value) => value,
        }
    }
}

/// A key the measured vocabulary does not model, kept with its claim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownField {
    /// The stored key.
    pub key: String,
    /// The claim the field stays unknown under.
    pub claim_id: &'static str,
    /// Where the key's value is.
    pub range: MemberRange,
}

/// Which state statement a [`StateStatement`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateKind {
    /// `OBJECT_TRANSLATE_STATE` (event opcode `7`).
    Translate,
    /// `OBJECT_ROTATE_STATE` (event opcode `9`).
    Rotate,
}

impl StateKind {
    /// The stored statement key.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Translate => "OBJECT_TRANSLATE_STATE",
            Self::Rotate => "OBJECT_ROTATE_STATE",
        }
    }
}

/// One `OBJECT_TRANSLATE_STATE` or `OBJECT_ROTATE_STATE` statement.
#[derive(Clone, Debug, PartialEq)]
pub struct StateStatement {
    kind: StateKind,
    node: String,
    stored: [StateNumber; 3],
    state_range: MemberRange,
    node_range: MemberRange,
    unknown_fields: Vec<UnknownField>,
}

impl StateStatement {
    /// Which statement this is.
    #[must_use]
    pub const fn kind(&self) -> StateKind {
        self.kind
    }

    /// The `NAME` the statement addresses.
    #[must_use]
    pub fn node(&self) -> &str {
        &self.node
    }

    /// The three numbers as stored.
    #[must_use]
    pub const fn stored(&self) -> [StateNumber; 3] {
        self.stored
    }

    /// The three numbers the executable keeps at event `+0x10..+0x18` before
    /// the rotate parser's degrees-to-radians multiply: integers converted to
    /// `f32`.
    #[must_use]
    pub fn state(&self) -> [f32; 3] {
        self.stored.map(StateNumber::value)
    }

    /// The numbers as the executable stores them: a translate triple as
    /// parsed, a rotate triple multiplied by [`DEGREES_TO_RADIANS`] in `f64`
    /// and narrowed to `f32` (image `0x506df9`..`0x506e1a`).
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn parsed(&self) -> [f32; 3] {
        let state = self.state();
        match self.kind {
            StateKind::Translate => state,
            StateKind::Rotate => state.map(|v| (f64::from(v) * DEGREES_TO_RADIANS) as f32),
        }
    }

    /// Where the `STATE` list is in the member.
    #[must_use]
    pub const fn state_range(&self) -> MemberRange {
        self.state_range
    }

    /// Where the `NAME` list is in the member.
    #[must_use]
    pub const fn node_range(&self) -> MemberRange {
        self.node_range
    }

    /// Keys of the statement outside `NAME` and `STATE`.
    #[must_use]
    pub fn unknown_fields(&self) -> &[UnknownField] {
        &self.unknown_fields
    }
}

/// One definition's `SEQUENCE_DEFINITION`.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacementSequence {
    name: Option<String>,
    translate: Option<StateStatement>,
    rotate: Option<StateStatement>,
    unknown_fields: Vec<UnknownField>,
    range: MemberRange,
}

impl PlacementSequence {
    /// The sequence's own `NAME` (`placement`, `up_down`).
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The `OBJECT_TRANSLATE_STATE` statement, when the sequence has one.
    #[must_use]
    pub const fn translate(&self) -> Option<&StateStatement> {
        self.translate.as_ref()
    }

    /// The `OBJECT_ROTATE_STATE` statement, when the sequence has one.
    #[must_use]
    pub const fn rotate(&self) -> Option<&StateStatement> {
        self.rotate.as_ref()
    }

    /// Keys of the sequence outside the measured vocabulary.
    #[must_use]
    pub fn unknown_fields(&self) -> &[UnknownField] {
        &self.unknown_fields
    }

    /// Where the sequence is in the member.
    #[must_use]
    pub const fn range(&self) -> MemberRange {
        self.range
    }
}

/// One `ANIMATION_DEFINITION` of the member.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacementDefinition {
    index: usize,
    names: Vec<String>,
    animation_name: String,
    activation: String,
    reset_time: Option<u32>,
    sequence: PlacementSequence,
    unknown_fields: Vec<UnknownField>,
    range: MemberRange,
}

impl PlacementDefinition {
    /// The definition's index in the member.
    #[must_use]
    pub const fn index(&self) -> usize {
        self.index
    }

    /// The definition's `NAME` object selectors.
    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// The `ANIMATION_NAME`.
    #[must_use]
    pub fn animation_name(&self) -> &str {
        &self.animation_name
    }

    /// The `ACTIVATION` spelling.
    #[must_use]
    pub fn activation(&self) -> &str {
        &self.activation
    }

    /// The stored `RESET_TIME` word, when present. Its meaning is unknown
    /// ([`RESET_TIME_CLAIM`]).
    #[must_use]
    pub const fn reset_time(&self) -> Option<u32> {
        self.reset_time
    }

    /// The `SEQUENCE_DEFINITION`.
    #[must_use]
    pub const fn sequence(&self) -> &PlacementSequence {
        &self.sequence
    }

    /// Keys of the definition outside the measured vocabulary.
    #[must_use]
    pub fn unknown_fields(&self) -> &[UnknownField] {
        &self.unknown_fields
    }

    /// Where the definition's value list is in the member.
    #[must_use]
    pub const fn range(&self) -> MemberRange {
        self.range
    }
}

/// A decoded `placezeps.zrd` member.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacezepsMember {
    definitions: Vec<PlacementDefinition>,
    list_fields: Vec<UnknownField>,
    byte_len: u64,
}

impl PlacezepsMember {
    /// The definitions in stored order.
    #[must_use]
    pub fn definitions(&self) -> &[PlacementDefinition] {
        &self.definitions
    }

    /// Keys of the `ANIMATION_LIST` outside `ANIMATION_DEFINITION`.
    #[must_use]
    pub fn list_fields(&self) -> &[UnknownField] {
        &self.list_fields
    }

    /// The member's size in bytes.
    #[must_use]
    pub const fn byte_len(&self) -> u64 {
        self.byte_len
    }
}

#[derive(Clone, Debug)]
enum Value {
    Int(u32),
    Float(f32),
    Text(String),
    List(Vec<Node>),
}

#[derive(Clone, Debug)]
struct Node {
    range: MemberRange,
    value: Value,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Cursor<'_> {
    const fn offset(&self) -> u64 {
        self.position as u64
    }

    const fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn word(&mut self) -> Result<u32, PlacezepsError> {
        let end = self.position + 4;
        let slice = self
            .bytes
            .get(self.position..end)
            .ok_or(PlacezepsError::Truncated {
                offset: self.offset(),
            })?;
        self.position = end;
        Ok(u32::from_le_bytes(slice.try_into().expect("four bytes")))
    }

    fn node(&mut self, depth: u32) -> Result<Node, PlacezepsError> {
        let start = self.offset();
        if depth > MAX_DEPTH {
            return Err(PlacezepsError::DepthExceeded { offset: start });
        }
        let value = match self.word()? {
            TAG_INT => Value::Int(self.word()?),
            TAG_FLOAT => Value::Float(f32::from_bits(self.word()?)),
            TAG_TEXT => {
                let declared = self.word()?;
                let length = declared as usize;
                if length > self.remaining() {
                    return Err(PlacezepsError::LengthDoesNotFit {
                        offset: start,
                        declared,
                    });
                }
                let body = &self.bytes[self.position..self.position + length];
                self.position += length;
                let text = std::str::from_utf8(body)
                    .map_err(|_| PlacezepsError::InvalidText { offset: start })?;
                Value::Text(text.to_owned())
            }
            TAG_LIST => {
                let declared = self.word()?;
                let children = (declared as usize)
                    .checked_sub(1)
                    .ok_or(PlacezepsError::ZeroListWord { offset: start })?;
                if children > self.remaining() / MIN_NODE_BYTES {
                    return Err(PlacezepsError::LengthDoesNotFit {
                        offset: start,
                        declared,
                    });
                }
                let mut kids = Vec::with_capacity(children);
                for _ in 0..children {
                    kids.push(self.node(depth + 1)?);
                }
                Value::List(kids)
            }
            tag => {
                return Err(PlacezepsError::UndefinedTag { offset: start, tag });
            }
        };
        Ok(Node {
            range: MemberRange {
                start,
                end: self.offset(),
            },
            value,
        })
    }
}

impl Node {
    const fn list(&self) -> Option<&Vec<Self>> {
        match &self.value {
            Value::List(children) => Some(children),
            _ => None,
        }
    }

    fn text(&self) -> Option<&str> {
        match &self.value {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }
}

/// The keyed pairs of a list: an even number of children alternating a text
/// key and its list value.
fn pairs<'n>(
    node: &'n Node,
    expected: &'static str,
    key: &str,
) -> Result<Vec<(&'n str, &'n Node)>, PlacezepsError> {
    let wrong = || PlacezepsError::WrongShape {
        offset: node.range.start,
        key: key.to_owned(),
        expected,
    };
    let children = node.list().ok_or_else(wrong)?;
    if children.len() % 2 != 0 {
        return Err(wrong());
    }
    let (keyed, _) = children.as_chunks::<2>();
    keyed
        .iter()
        .map(|[key, value]| Ok((key.text().ok_or_else(wrong)?, value)))
        .collect()
}

fn single_text(node: &Node, key: &str) -> Result<String, PlacezepsError> {
    match node.list().map(Vec::as_slice) {
        Some([only]) => only.text().map(str::to_owned),
        _ => None,
    }
    .ok_or_else(|| PlacezepsError::WrongShape {
        offset: node.range.start,
        key: key.to_owned(),
        expected: "a list of one string",
    })
}

fn text_list(node: &Node, key: &str) -> Result<Vec<String>, PlacezepsError> {
    let wrong = || PlacezepsError::WrongShape {
        offset: node.range.start,
        key: key.to_owned(),
        expected: "a nonempty list of strings",
    };
    let children = node.list().ok_or_else(wrong)?;
    if children.is_empty() {
        return Err(wrong());
    }
    children
        .iter()
        .map(|child| child.text().map(str::to_owned).ok_or_else(wrong))
        .collect()
}

fn single_int(node: &Node, key: &str) -> Result<u32, PlacezepsError> {
    match node.list().map(Vec::as_slice) {
        Some(
            [
                Node {
                    value: Value::Int(value),
                    ..
                },
            ],
        ) => Some(*value),
        _ => None,
    }
    .ok_or_else(|| PlacezepsError::WrongShape {
        offset: node.range.start,
        key: key.to_owned(),
        expected: "a list of one integer",
    })
}

fn once<T>(slot: &mut Option<T>, value: T, node: &Node, key: &str) -> Result<(), PlacezepsError> {
    if slot.replace(value).is_some() {
        return Err(PlacezepsError::DuplicateKey {
            offset: node.range.start,
            key: key.to_owned(),
        });
    }
    Ok(())
}

fn unknown(key: &str, node: &Node) -> UnknownField {
    UnknownField {
        key: key.to_owned(),
        claim_id: UNMODELLED_FIELD_CLAIM,
        range: node.range,
    }
}

fn state_statement(kind: StateKind, node: &Node) -> Result<StateStatement, PlacezepsError> {
    let mut name: Option<(String, MemberRange)> = None;
    let mut state: Option<([StateNumber; 3], MemberRange)> = None;
    let mut unknown_fields = Vec::new();
    for (key, value) in pairs(node, "a keyed statement", kind.key())? {
        match key {
            "NAME" => once(
                &mut name,
                (single_text(value, key)?, value.range),
                value,
                key,
            )?,
            "STATE" => {
                let wrong = || PlacezepsError::WrongShape {
                    offset: value.range.start,
                    key: key.to_owned(),
                    expected: "a list of three numbers",
                };
                let numbers = value.list().ok_or_else(wrong)?;
                let [a, b, c] = numbers.as_slice() else {
                    return Err(wrong());
                };
                let number = |n: &Node| match n.value {
                    Value::Int(word) => {
                        Ok(StateNumber::Int(i32::from_le_bytes(word.to_le_bytes())))
                    }
                    Value::Float(float) => Ok(StateNumber::Float(float)),
                    _ => Err(wrong()),
                };
                once(
                    &mut state,
                    ([number(a)?, number(b)?, number(c)?], value.range),
                    value,
                    key,
                )?;
            }
            other => unknown_fields.push(unknown(other, value)),
        }
    }
    let missing = |key| PlacezepsError::MissingKey {
        offset: node.range.start,
        key,
    };
    let (node_name, node_range) = name.ok_or_else(|| missing("NAME"))?;
    let (stored, state_range) = state.ok_or_else(|| missing("STATE"))?;
    Ok(StateStatement {
        kind,
        node: node_name,
        stored,
        state_range,
        node_range,
        unknown_fields,
    })
}

fn sequence(node: &Node) -> Result<PlacementSequence, PlacezepsError> {
    let mut name = None;
    let mut translate = None;
    let mut rotate = None;
    let mut unknown_fields = Vec::new();
    for (key, value) in pairs(node, "a keyed sequence", "SEQUENCE_DEFINITION")? {
        match key {
            "NAME" => once(&mut name, single_text(value, key)?, value, key)?,
            "OBJECT_TRANSLATE_STATE" => once(
                &mut translate,
                state_statement(StateKind::Translate, value)?,
                value,
                key,
            )?,
            "OBJECT_ROTATE_STATE" => once(
                &mut rotate,
                state_statement(StateKind::Rotate, value)?,
                value,
                key,
            )?,
            other => unknown_fields.push(unknown(other, value)),
        }
    }
    Ok(PlacementSequence {
        name,
        translate,
        rotate,
        unknown_fields,
        range: node.range,
    })
}

fn definition(index: usize, node: &Node) -> Result<PlacementDefinition, PlacezepsError> {
    let mut names = None;
    let mut animation_name = None;
    let mut activation = None;
    let mut reset_time = None;
    let mut seq = None;
    let mut unknown_fields = Vec::new();
    for (key, value) in pairs(node, "a keyed definition", ANIMATION_DEFINITION_KEY)? {
        match key {
            "NAME" => once(&mut names, text_list(value, key)?, value, key)?,
            "ANIMATION_NAME" => once(&mut animation_name, single_text(value, key)?, value, key)?,
            "ACTIVATION" => once(&mut activation, single_text(value, key)?, value, key)?,
            "RESET_TIME" => once(&mut reset_time, single_int(value, key)?, value, key)?,
            "SEQUENCE_DEFINITION" => once(&mut seq, sequence(value)?, value, key)?,
            other => unknown_fields.push(unknown(other, value)),
        }
    }
    let missing = |key| PlacezepsError::MissingKey {
        offset: node.range.start,
        key,
    };
    Ok(PlacementDefinition {
        index,
        names: names.ok_or_else(|| missing("NAME"))?,
        animation_name: animation_name.ok_or_else(|| missing("ANIMATION_NAME"))?,
        activation: activation.ok_or_else(|| missing("ACTIVATION"))?,
        reset_time,
        sequence: seq.ok_or_else(|| missing("SEQUENCE_DEFINITION"))?,
        unknown_fields,
        range: node.range,
    })
}

/// Decodes a `placezeps.zrd` member field by field.
///
/// # Errors
///
/// A [`PlacezepsError`] naming the byte offset of the first byte outside the
/// measured grammar: a frame that is not `ANIMATION_DEFINITIONS`, a `STATE`
/// that is not three numbers, a repeated or missing required key.
pub fn read_placezeps_member(bytes: &[u8]) -> Result<PlacezepsMember, PlacezepsError> {
    let mut cursor = Cursor { bytes, position: 0 };
    let root = cursor.node(0)?;
    if cursor.remaining() != 0 {
        return Err(PlacezepsError::TrailingBytes {
            offset: cursor.offset(),
            count: cursor.remaining() as u64,
        });
    }
    let not_member = |reason| PlacezepsError::NotAMember { offset: 0, reason };
    let [record] = root
        .list()
        .map(Vec::as_slice)
        .ok_or_else(|| not_member("the root must be a list"))?
    else {
        return Err(not_member("the root must hold exactly one record"));
    };
    let fields = pairs(record, "a keyed record", ANIMATION_DEFINITIONS_KEY)?;
    let [(ANIMATION_DEFINITIONS_KEY, body)] = fields.as_slice() else {
        return Err(not_member(
            "the record must be exactly ANIMATION_DEFINITIONS",
        ));
    };
    let body_fields = pairs(body, "a keyed body", ANIMATION_DEFINITIONS_KEY)?;
    let [(ANIMATION_LIST_KEY, list)] = body_fields.as_slice() else {
        return Err(not_member("the body must be exactly ANIMATION_LIST"));
    };
    let mut definitions = Vec::new();
    let mut list_fields = Vec::new();
    for (key, value) in pairs(list, "a keyed list", ANIMATION_LIST_KEY)? {
        if key == ANIMATION_DEFINITION_KEY {
            definitions.push(definition(definitions.len(), value)?);
        } else {
            list_fields.push(unknown(key, value));
        }
    }
    Ok(PlacezepsMember {
        definitions,
        list_fields,
        byte_len: bytes.len() as u64,
    })
}
