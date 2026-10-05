//! The campaign mission's detection-zone member, `dzones.zrd` (task #513).
//!
//! 23 of the installation's campaign mission reader archives list a member of
//! this name (155 to 826 bytes, 9 197 in all). Task #427 left it undecoded
//! because the plausible grammar — `u32` tag, `u32` count, counted values —
//! failed on the first list. The measurement that resolves it: **a list's
//! second word is the child count plus one**, i.e. a list of `N` holds `N - 1`
//! children (the rule `cs_content`'s general `.zrd` decoder already uses, found
//! by F09). Read that way all 23 members decode with no byte left over.
//!
//! # The measured grammar
//!
//! Every word is a little-endian `u32`. A node is one of
//!
//! * tag `1`: an integer, one `u32`;
//! * tag `3`: a string, a `u32` byte length and that many bytes;
//! * tag `4`: a list, a `u32` word `N` and `N - 1` child nodes.
//!
//! Tag `2` (a float in the general `.zrd` grammar) occurs in **no** measured
//! member and is refused here as undefined rather than read on trust.
//!
//! The member is one **record**: a list whose children alternate a string key
//! and that key's value. Three keys occur, each at most once per member:
//!
//! | key | value shape | members |
//! | --- | --- | --- |
//! | `disable` | list of zone-name strings | 16 |
//! | `nosnapshot` | list of zone-name strings | 12 |
//! | `objective_numbers` | list of `[zone-name, integer]` pair lists | 20 |
//!
//! Every name is a `dzpath<N>` string, the vocabulary the world containers'
//! zone nodes use.
//!
//! # What is still unknown
//!
//! The grammar is measured; **what a mission means by it is not**
//! ([`KeyMeaning::Unknown`] for all three keys). `disable` and `nosnapshot` name
//! zones, and what disabling or not snapshotting one does is unmeasured.
//! `objective_numbers` binds a zone to an integer (measured values 18 to 31,
//! unique within a member); the integer is a mission objective number only by
//! the key's own spelling, and which objective it indexes is `objectives.zrd`'s
//! question (F13-D / F39). A zone a member does not name under any key is not
//! thereby unused: nothing measured says a mission must name every zone.
//!
//! Nothing here is `verified_original`: no original run happened.

use std::fmt;

use cs_types::evidence::ClaimStatus;

/// The zone-name list key whose meaning is unmeasured.
pub const KEY_DISABLE: &str = "disable";
/// The zone-name list key whose meaning is unmeasured.
pub const KEY_NO_SNAPSHOT: &str = "nosnapshot";
/// The key binding zone names to integers.
pub const KEY_OBJECTIVE_NUMBERS: &str = "objective_numbers";

const TAG_INT: u32 = 1;
const TAG_TEXT: u32 = 3;
const TAG_LIST: u32 = 4;

/// The deepest nesting the measured grammar needs is three (record, value list,
/// pair); anything deeper is refused instead of recursed into.
const MAX_DEPTH: u32 = 8;

/// The smallest encoded node: a tag word and one payload word.
const MIN_NODE_BYTES: usize = 8;

/// What a key's value means to the original. Only `Unknown` exists, on purpose:
/// no measurement here establishes a meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyMeaning {
    /// The grammar is measured; the original's use of it is not.
    Unknown,
}

impl KeyMeaning {
    /// The evidence class of the meaning: `unknown`, by construction.
    pub const fn evidence(self) -> ClaimStatus {
        ClaimStatus::Unknown
    }
}

/// The three keys a `dzones.zrd` record uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetectionZoneKey {
    /// `disable`: a list of zone names.
    Disable,
    /// `nosnapshot`: a list of zone names.
    NoSnapshot,
    /// `objective_numbers`: a list of `[zone name, integer]` pairs.
    ObjectiveNumbers,
}

impl DetectionZoneKey {
    /// Every key, in the order the table above lists them.
    pub const ALL: [Self; 3] = [Self::Disable, Self::NoSnapshot, Self::ObjectiveNumbers];

    /// The key's stored spelling.
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Disable => KEY_DISABLE,
            Self::NoSnapshot => KEY_NO_SNAPSHOT,
            Self::ObjectiveNumbers => KEY_OBJECTIVE_NUMBERS,
        }
    }

    /// The key a spelling names, if it is one of the three measured.
    pub fn from_spelling(spelling: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.spelling() == spelling)
    }

    /// What the key means to the original: unmeasured for all three.
    pub const fn meaning(self) -> KeyMeaning {
        KeyMeaning::Unknown
    }

    /// What the stored value is, structurally (measured).
    pub const fn value_shape(self) -> &'static str {
        match self {
            Self::Disable | Self::NoSnapshot => "list of zone-name strings",
            Self::ObjectiveNumbers => "list of [zone-name string, integer] pair lists",
        }
    }
}

/// Why a `dzones.zrd` member was refused. Every variant names the byte offset,
/// relative to the member, where the refusal was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DetectionZonesError {
    /// A word or string body ran past the end of the member.
    Truncated {
        /// Where the missing bytes were needed.
        offset: u64,
    },
    /// A tag outside the three measured kinds (`1`, `3`, `4`), including the
    /// general grammar's float tag `2`, which no measured member carries.
    UndefinedTag {
        /// Where the tag word is.
        offset: u64,
        /// The tag found.
        tag: u32,
    },
    /// A declared length cannot fit in the bytes left: a string longer than the
    /// remainder, or a list whose `N - 1` children cannot each take a minimum
    /// node.
    LengthDoesNotFit {
        /// Where the node starts.
        offset: u64,
        /// The declared length word.
        declared: u32,
        /// Bytes left after the length word.
        remaining: u64,
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
    /// Nesting deeper than the measured grammar needs.
    DepthExceeded {
        /// Where the node starts.
        offset: u64,
    },
    /// Bytes remain after the root node ends.
    TrailingBytes {
        /// Where the root ended.
        offset: u64,
        /// How many bytes follow it.
        count: u64,
    },
    /// The root is not a list, or a record position holds the wrong kind of
    /// node (a key that is not a string, or an odd child count).
    NotAKeyedRecord {
        /// Where the offending node starts.
        offset: u64,
    },
    /// A record key outside the three measured.
    UnknownKey {
        /// The key found.
        key: String,
    },
    /// A key stated twice in one member.
    DuplicateKey {
        /// The key repeated.
        key: &'static str,
    },
    /// A key's value is not the shape [`DetectionZoneKey::value_shape`] states.
    WrongValueShape {
        /// The key whose value was refused.
        key: &'static str,
    },
}

impl fmt::Display for DetectionZonesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { offset } => write!(f, "truncated at byte {offset}"),
            Self::UndefinedTag { offset, tag } => {
                write!(f, "undefined tag {tag} at byte {offset}")
            }
            Self::LengthDoesNotFit {
                offset,
                declared,
                remaining,
            } => write!(
                f,
                "length {declared} at byte {offset} does not fit in the {remaining} bytes left"
            ),
            Self::ZeroListWord { offset } => write!(f, "list word 0 at byte {offset}"),
            Self::InvalidText { offset } => write!(f, "string at byte {offset} is not UTF-8"),
            Self::DepthExceeded { offset } => write!(f, "nesting too deep at byte {offset}"),
            Self::TrailingBytes { offset, count } => {
                write!(f, "{count} trailing bytes after byte {offset}")
            }
            Self::NotAKeyedRecord { offset } => {
                write!(f, "not a key/value record at byte {offset}")
            }
            Self::UnknownKey { key } => write!(f, "unknown key {key:?}"),
            Self::DuplicateKey { key } => write!(f, "key {key} stated twice"),
            Self::WrongValueShape { key } => {
                write!(f, "the value of {key} is not a {}", shape_of(key))
            }
        }
    }
}

fn shape_of(key: &str) -> &'static str {
    DetectionZoneKey::from_spelling(key).map_or("measured shape", DetectionZoneKey::value_shape)
}

impl std::error::Error for DetectionZonesError {}

/// One decoded `dzones.zrd` member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectionZoneDeclarations {
    keys: Vec<DetectionZoneKey>,
    disable: Option<Vec<String>>,
    no_snapshot: Option<Vec<String>>,
    objective_numbers: Option<Vec<(String, u32)>>,
}

impl DetectionZoneDeclarations {
    /// The keys the member stated, in stored order.
    pub fn keys(&self) -> &[DetectionZoneKey] {
        &self.keys
    }

    /// The `disable` zone names, when the member states that key.
    pub fn disable(&self) -> Option<&[String]> {
        self.disable.as_deref()
    }

    /// The `nosnapshot` zone names, when the member states that key.
    pub fn no_snapshot(&self) -> Option<&[String]> {
        self.no_snapshot.as_deref()
    }

    /// The `objective_numbers` pairs, in stored order, when stated.
    pub fn objective_numbers(&self) -> Option<&[(String, u32)]> {
        self.objective_numbers.as_deref()
    }

    /// Every zone name the member states under any key, with the key it was
    /// stated under, in stored order. A name stated twice is listed twice.
    pub fn named_zones(&self) -> Vec<(DetectionZoneKey, &str)> {
        let mut named = Vec::new();
        for key in &self.keys {
            match key {
                DetectionZoneKey::Disable => {
                    for name in self.disable.iter().flatten() {
                        named.push((*key, name.as_str()));
                    }
                }
                DetectionZoneKey::NoSnapshot => {
                    for name in self.no_snapshot.iter().flatten() {
                        named.push((*key, name.as_str()));
                    }
                }
                DetectionZoneKey::ObjectiveNumbers => {
                    for (name, _) in self.objective_numbers.iter().flatten() {
                        named.push((*key, name.as_str()));
                    }
                }
            }
        }
        named
    }
}

enum Node {
    Int(u32),
    Text(String),
    List(Vec<Self>),
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

    fn word(&mut self) -> Result<u32, DetectionZonesError> {
        let end = self.position + 4;
        let slice = self
            .bytes
            .get(self.position..end)
            .ok_or(DetectionZonesError::Truncated {
                offset: self.offset(),
            })?;
        self.position = end;
        Ok(u32::from_le_bytes(slice.try_into().expect("four bytes")))
    }

    fn node(&mut self, depth: u32) -> Result<Node, DetectionZonesError> {
        let start = self.offset();
        if depth > MAX_DEPTH {
            return Err(DetectionZonesError::DepthExceeded { offset: start });
        }
        match self.word()? {
            TAG_INT => Ok(Node::Int(self.word()?)),
            TAG_TEXT => {
                let declared = self.word()?;
                let length = declared as usize;
                if length > self.remaining() {
                    return Err(DetectionZonesError::LengthDoesNotFit {
                        offset: start,
                        declared,
                        remaining: self.remaining() as u64,
                    });
                }
                let body = &self.bytes[self.position..self.position + length];
                self.position += length;
                let text = std::str::from_utf8(body)
                    .map_err(|_| DetectionZonesError::InvalidText { offset: start })?;
                Ok(Node::Text(text.to_owned()))
            }
            TAG_LIST => {
                let declared = self.word()?;
                let children = (declared as usize)
                    .checked_sub(1)
                    .ok_or(DetectionZonesError::ZeroListWord { offset: start })?;
                if children > self.remaining() / MIN_NODE_BYTES {
                    return Err(DetectionZonesError::LengthDoesNotFit {
                        offset: start,
                        declared,
                        remaining: self.remaining() as u64,
                    });
                }
                let mut kids = Vec::with_capacity(children);
                for _ in 0..children {
                    kids.push(self.node(depth + 1)?);
                }
                Ok(Node::List(kids))
            }
            tag => Err(DetectionZonesError::UndefinedTag { offset: start, tag }),
        }
    }
}

fn names(key: DetectionZoneKey, value: Node) -> Result<Vec<String>, DetectionZonesError> {
    let wrong = || DetectionZonesError::WrongValueShape {
        key: key.spelling(),
    };
    let Node::List(items) = value else {
        return Err(wrong());
    };
    items
        .into_iter()
        .map(|item| match item {
            Node::Text(name) => Ok(name),
            _ => Err(wrong()),
        })
        .collect()
}

fn pairs(key: DetectionZoneKey, value: Node) -> Result<Vec<(String, u32)>, DetectionZonesError> {
    let wrong = || DetectionZonesError::WrongValueShape {
        key: key.spelling(),
    };
    let Node::List(items) = value else {
        return Err(wrong());
    };
    items
        .into_iter()
        .map(|item| match item {
            Node::List(pair) => match <[Node; 2]>::try_from(pair) {
                Ok([Node::Text(name), Node::Int(number)]) => Ok((name, number)),
                _ => Err(wrong()),
            },
            _ => Err(wrong()),
        })
        .collect()
}

/// Decodes one `dzones.zrd` member through the grammar measured over all 23
/// retail members.
///
/// # Errors
///
/// [`DetectionZonesError`], by name: `Truncated` for a word or body past the
/// end, `UndefinedTag` for a tag outside `1`/`3`/`4`, `LengthDoesNotFit` for a
/// length the remaining bytes cannot hold, `TrailingBytes` for bytes after the
/// root, and the structural refusals (`NotAKeyedRecord`, `UnknownKey`,
/// `DuplicateKey`, `WrongValueShape`) for a record outside the measured shape.
pub fn read_detection_zones(
    bytes: &[u8],
) -> Result<DetectionZoneDeclarations, DetectionZonesError> {
    let mut cursor = Cursor { bytes, position: 0 };
    let root = cursor.node(0)?;
    if cursor.remaining() != 0 {
        return Err(DetectionZonesError::TrailingBytes {
            offset: cursor.offset(),
            count: cursor.remaining() as u64,
        });
    }
    let Node::List(children) = root else {
        return Err(DetectionZonesError::NotAKeyedRecord { offset: 0 });
    };
    if children.len() % 2 != 0 {
        return Err(DetectionZonesError::NotAKeyedRecord { offset: 0 });
    }
    let mut out = DetectionZoneDeclarations {
        keys: Vec::new(),
        disable: None,
        no_snapshot: None,
        objective_numbers: None,
    };
    let mut children = children.into_iter();
    while let (Some(key), Some(value)) = (children.next(), children.next()) {
        let Node::Text(spelling) = key else {
            return Err(DetectionZonesError::NotAKeyedRecord { offset: 0 });
        };
        let key = DetectionZoneKey::from_spelling(&spelling)
            .ok_or(DetectionZonesError::UnknownKey { key: spelling })?;
        if out.keys.contains(&key) {
            return Err(DetectionZonesError::DuplicateKey {
                key: key.spelling(),
            });
        }
        out.keys.push(key);
        match key {
            DetectionZoneKey::Disable => out.disable = Some(names(key, value)?),
            DetectionZoneKey::NoSnapshot => out.no_snapshot = Some(names(key, value)?),
            DetectionZoneKey::ObjectiveNumbers => {
                out.objective_numbers = Some(pairs(key, value)?);
            }
        }
    }
    Ok(out)
}
