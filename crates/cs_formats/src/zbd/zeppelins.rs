//! The mission-scoped placed-zeppelin member, `zeppelins.zrd` (task #574).
//!
//! 50 of the installation's 53 mission reader archives
//! `ZBD/<group>/<mission>/zrdr.zbd` carry a member of this name (16 to 7 535
//! bytes, 153 406 in all); `c1/m02`, `c2/m01` and `c5/mp2` are the three that
//! do not, and no installation-scope archive carries it (the F33-D census,
//! `docs/findings/2026-10-03-f33-d-neutral-traffic-population-and-retail-census.md`).
//! Task #574 decoded the member: **every one of the 50 parses under the `.zrd`
//! grammar with no byte left over.**
//!
//! # The measured grammar
//!
//! Every word is a little-endian `u32`. A node is one of
//!
//! * tag `1`: an integer, one `u32`;
//! * tag `2`: a float, one `f32` bit pattern;
//! * tag `3`: a string, a `u32` byte length and that many bytes;
//! * tag `4`: a list, a `u32` word `N` and `N - 1` child nodes (the rule
//!   `cs_content`'s general `.zrd` decoder already uses, found by F09).
//!
//! The member is **one list holding one list of records**: the root list's
//! only child is the record list, and every record is a keyed list — an even
//! number of children alternating a text key and that key's value, the value
//! always a list. Between them the members carry **58** records: 12 members
//! hold none at all (the `c*/mp1` and `c*/mp2` multiplayer missions carry an
//! empty record list), 23 hold one, 11 hold two, three hold three and one
//! holds four.
//!
//! # What a record is
//!
//! A record is one placed zeppelin: a `node` binding into the mission's world
//! (measured names are `piratezep`, `cargozep1..3`, `vostokzep`, `beowulfzep`,
//! `blackhatzep`, `blackswanzep`, `dantezep`, `geminizep`, `workersvoyagezep`,
//! `multiplayer1zep`, `multiplayer2zep`), a `position`/`yaw`/`pitch` pose,
//! motion tuning (`max_speed`, `max_accel`, the rate and pitch-limit floats),
//! a `net` name, gasbag/engine/cannon node bindings and the `targets` the
//! authored cannons name (`player` or a sibling record's `node` — every
//! measured non-`player` target resolves to a record in the same member).
//! Sixteen records carry a `team` spelling (`ally` 12, `enemy` 4) and nine
//! carry `deactivated` (`0` or `1`).
//!
//! # What is still unknown
//!
//! The grammar and the per-key shapes are measured; **what the original does
//! with any of it is not** ([`KeyMeaning::Unknown`] for every key). In
//! particular: whether a record is spawned as an actor, which of them — if
//! any — the original treats as neutral traffic, what `team`, `deactivated`,
//! `num_healthy_required`, the `gasbags`/`cannon_health` floats and the
//! `healthy`/`gasbags` attachment spellings mean at runtime, and how a record
//! maps to a pilot, an airframe catalog id or a faction are all unmeasured.
//! Nothing here is `verified_original`: no original run happened.
//!
//! The member's semantics and the complete per-key measurement live in
//! `docs/findings/2026-10-08-t574-zeppelins-zrd-placed-zeppelin-carrier.md`.

use std::fmt;

use cs_types::evidence::ClaimStatus;

/// The member name the mission reader archives carry this record set under.
pub const ZEPPELINS_MEMBER: &str = "zeppelins.zrd";

const TAG_INT: u32 = 1;
const TAG_FLOAT: u32 = 2;
const TAG_TEXT: u32 = 3;
const TAG_LIST: u32 = 4;

/// The deepest the measured grammar nests: root, record list, record, a row
/// list, a row and its inner lists is six; anything deeper is refused instead
/// of recursed into.
const MAX_DEPTH: u32 = 8;

/// The smallest encoded node: a tag word and one payload word.
const MIN_NODE_BYTES: usize = 8;

/// What a key's value means to the original. Only `Unknown` exists, on
/// purpose: no measurement here establishes a meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyMeaning {
    /// The grammar and value shapes are measured; the original's use of them
    /// is not.
    Unknown,
}

impl KeyMeaning {
    /// The evidence class of the meaning: `unknown`, by construction.
    pub const fn evidence(self) -> ClaimStatus {
        ClaimStatus::Unknown
    }
}

/// A `team` spelling the measured corpus carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasuredTeam {
    /// `ally`: 12 of the 16 team-carrying records.
    Ally,
    /// `enemy`: the other 4.
    Enemy,
}

impl MeasuredTeam {
    /// The measured spelling.
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Ally => "ally",
            Self::Enemy => "enemy",
        }
    }

    /// The team a spelling names, when it is one of the two measured.
    pub fn from_spelling(spelling: &str) -> Option<Self> {
        match spelling {
            "ally" => Some(Self::Ally),
            "enemy" => Some(Self::Enemy),
            _ => None,
        }
    }
}

/// The 26 keys a `zeppelins.zrd` record uses, in the order the corpus most
/// often spells them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZeppelinKey {
    /// `deactivated`: an integer (`0` or `1` measured), optional.
    Deactivated,
    /// `node`: the world node the record binds, required.
    Node,
    /// `team`: a side spelling (`ally`/`enemy` measured), optional.
    Team,
    /// `position`: three floats, required.
    Position,
    /// `yaw`: one float, required.
    Yaw,
    /// `pitch`: one float, required.
    Pitch,
    /// `max_speed`: one float, required.
    MaxSpeed,
    /// `max_accel`: one float, required.
    MaxAccel,
    /// `accel_pitch`: one float, required.
    AccelPitch,
    /// `accel_yaw`: one float, required.
    AccelYaw,
    /// `max_rate_yaw`: one float, required.
    MaxRateYaw,
    /// `max_rate_pitch`: one float, required.
    MaxRatePitch,
    /// `min_pitch`: one float, required.
    MinPitch,
    /// `max_pitch`: one float, required.
    MaxPitch,
    /// `net`: the record's net name, required.
    Net,
    /// `healthy`: gasbag/attachment bindings, required.
    Healthy,
    /// `num_healthy_required`: an integer (`2` to `5` measured), required.
    NumHealthyRequired,
    /// `engines`: a list of engine node names, required.
    Engines,
    /// `cannon_fire_delay`: one float, optional.
    CannonFireDelay,
    /// `cannon_fire_range`: one float, optional.
    CannonFireRange,
    /// `cannon_inaccuracy`: one float, optional (3 records).
    CannonInaccuracy,
    /// `left_cannons`: cannon bindings, optional.
    LeftCannons,
    /// `right_cannons`: cannon bindings, optional.
    RightCannons,
    /// `targets`: a list of target names (possibly empty), optional.
    Targets,
    /// `gasbags`: gasbag rows, optional.
    Gasbags,
    /// `cannon_health`: per-cannon rows, optional.
    CannonHealth,
}

impl ZeppelinKey {
    /// Every key, in the order the table above lists them.
    pub const ALL: [Self; 26] = [
        Self::Deactivated,
        Self::Node,
        Self::Team,
        Self::Position,
        Self::Yaw,
        Self::Pitch,
        Self::MaxSpeed,
        Self::MaxAccel,
        Self::AccelPitch,
        Self::AccelYaw,
        Self::MaxRateYaw,
        Self::MaxRatePitch,
        Self::MinPitch,
        Self::MaxPitch,
        Self::Net,
        Self::Healthy,
        Self::NumHealthyRequired,
        Self::Engines,
        Self::CannonFireDelay,
        Self::CannonFireRange,
        Self::CannonInaccuracy,
        Self::LeftCannons,
        Self::RightCannons,
        Self::Targets,
        Self::Gasbags,
        Self::CannonHealth,
    ];

    /// The key's stored spelling.
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Deactivated => "deactivated",
            Self::Node => "node",
            Self::Team => "team",
            Self::Position => "position",
            Self::Yaw => "yaw",
            Self::Pitch => "pitch",
            Self::MaxSpeed => "max_speed",
            Self::MaxAccel => "max_accel",
            Self::AccelPitch => "accel_pitch",
            Self::AccelYaw => "accel_yaw",
            Self::MaxRateYaw => "max_rate_yaw",
            Self::MaxRatePitch => "max_rate_pitch",
            Self::MinPitch => "min_pitch",
            Self::MaxPitch => "max_pitch",
            Self::Net => "net",
            Self::Healthy => "healthy",
            Self::NumHealthyRequired => "num_healthy_required",
            Self::Engines => "engines",
            Self::CannonFireDelay => "cannon_fire_delay",
            Self::CannonFireRange => "cannon_fire_range",
            Self::CannonInaccuracy => "cannon_inaccuracy",
            Self::LeftCannons => "left_cannons",
            Self::RightCannons => "right_cannons",
            Self::Targets => "targets",
            Self::Gasbags => "gasbags",
            Self::CannonHealth => "cannon_health",
        }
    }

    /// The key a spelling names, if it is one of the 26 measured.
    pub fn from_spelling(spelling: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.spelling() == spelling)
    }

    /// Whether every measured record states the key.
    pub const fn required(self) -> bool {
        !matches!(
            self,
            Self::Deactivated
                | Self::Team
                | Self::CannonFireDelay
                | Self::CannonFireRange
                | Self::CannonInaccuracy
                | Self::LeftCannons
                | Self::RightCannons
                | Self::Targets
                | Self::Gasbags
                | Self::CannonHealth
        )
    }

    /// What the key means to the original: unmeasured for all 26.
    pub const fn meaning(self) -> KeyMeaning {
        KeyMeaning::Unknown
    }

    /// What the stored value is, structurally (measured).
    pub const fn value_shape(self) -> &'static str {
        match self {
            Self::Node | Self::Net | Self::Team => "a one-text list",
            Self::Deactivated | Self::NumHealthyRequired => "a one-integer list",
            Self::Position => "a three-float list",
            Self::Yaw
            | Self::Pitch
            | Self::MaxSpeed
            | Self::MaxAccel
            | Self::AccelPitch
            | Self::AccelYaw
            | Self::MaxRateYaw
            | Self::MaxRatePitch
            | Self::MinPitch
            | Self::MaxPitch
            | Self::CannonFireDelay
            | Self::CannonFireRange
            | Self::CannonInaccuracy => "a one-float list",
            Self::Engines | Self::Targets => "a list of texts",
            Self::Healthy => "a list of two-text rows",
            Self::LeftCannons | Self::RightCannons => "a list of three-text rows",
            Self::Gasbags => "a list of [text, float, one-text list, optional text] rows",
            Self::CannonHealth => {
                "a list of [four texts, float, one-text list, two float/text pairs] rows"
            }
        }
    }
}

/// A `healthy` row: two node-name spellings. Every measured row's second
/// spelling is `panels`; what the binding means at runtime is unmeasured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthyBinding {
    node: String,
    attachment: String,
}

impl HealthyBinding {
    /// The row's first spelling (a gasbag node name in the measured corpus).
    pub fn node(&self) -> &str {
        &self.node
    }

    /// The row's second spelling (`panels` in all 316 measured rows).
    pub fn attachment(&self) -> &str {
        &self.attachment
    }
}

/// A `left_cannons`/`right_cannons` row: a cannon node name and the two
/// animation names the corpus spells `deploy_*` and `retract_*`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CannonBinding {
    node: String,
    deploy: String,
    retract: String,
}

impl CannonBinding {
    /// The cannon's node name (`lbroad<N>`/`rbroad<N>` spellings measured).
    pub fn node(&self) -> &str {
        &self.node
    }

    /// The `deploy_`-spelled animation name.
    pub fn deploy(&self) -> &str {
        &self.deploy
    }

    /// The `retract_`-spelled animation name.
    pub fn retract(&self) -> &str {
        &self.retract
    }
}

/// A `gasbags` row: a gasbag node name, a float (six measured values, `80.0`
/// to `400.0`), a one-text list (`*_gasbagtorpedo<N>` spellings measured) and
/// — on the multiplayer records — a fourth spelling (`panels` in all 100
/// measured four-element rows).
#[derive(Clone, Debug, PartialEq)]
pub struct GasbagRow {
    node: String,
    value: f32,
    torpedo: String,
    attachment: Option<String>,
}

impl GasbagRow {
    /// The gasbag's node name.
    pub fn node(&self) -> &str {
        &self.node
    }

    /// The row's float (six measured values, `80.0` to `400.0`; the meaning
    /// is unmeasured).
    pub const fn value(&self) -> f32 {
        self.value
    }

    /// The one-text list's spelling (`*_gasbagtorpedo<N>` measured).
    pub fn torpedo(&self) -> &str {
        &self.torpedo
    }

    /// The fourth element, when the row carries one (`panels` in every
    /// measured four-element row).
    pub fn attachment(&self) -> Option<&str> {
        self.attachment.as_deref()
    }
}

/// A `cannon_health` row: seven elements measured on the instant-action and
/// `mp3` records — four texts (the cannon node, `gunback`, `frame` and a
/// gasbag node), a float (`200.0` measured), a one-text list
/// (`destroy_*` spellings measured) and a two-pair list of a float
/// (`0.6`/`0.3` measured) with a damaged-state node spelling
/// (`60_*`/`30_*` measured).
#[derive(Clone, Debug, PartialEq)]
pub struct CannonHealthRow {
    node: String,
    mount: String,
    frame: String,
    gasbag: String,
    value: f32,
    destroy: String,
    states: [(f32, String); 2],
}

impl CannonHealthRow {
    /// The row's first text: the cannon node name.
    pub fn node(&self) -> &str {
        &self.node
    }

    /// The second text (`gunback` in all 144 measured rows).
    pub fn mount(&self) -> &str {
        &self.mount
    }

    /// The third text (`frame` in all 144 measured rows).
    pub fn frame(&self) -> &str {
        &self.frame
    }

    /// The fourth text (a `gasbag<N>` node name in all 144 measured rows).
    pub fn gasbag(&self) -> &str {
        &self.gasbag
    }

    /// The row's float (`200.0` in all 144 measured rows; the meaning is
    /// unmeasured).
    pub const fn value(&self) -> f32 {
        self.value
    }

    /// The one-text list's spelling (`destroy_*` measured).
    pub fn destroy(&self) -> &str {
        &self.destroy
    }

    /// The two `(float, text)` pairs: a fraction (`0.6`/`0.3` measured) and a
    /// damaged-state node spelling (`60_*`/`30_*` measured).
    pub fn states(&self) -> &[(f32, String); 2] {
        &self.states
    }
}

/// Why a `zeppelins.zrd` member was refused. Every variant names the byte
/// offset, relative to the member, where the refusal was found.
#[derive(Clone, Debug, PartialEq)]
pub enum ZeppelinsError {
    /// A word or string body ran past the end of the member.
    Truncated {
        /// Where the missing bytes were needed.
        offset: u64,
    },
    /// A tag outside the four measured kinds (`1`, `2`, `3`, `4`).
    UndefinedTag {
        /// Where the tag word is.
        offset: u64,
        /// The tag found.
        tag: u32,
    },
    /// A declared length cannot fit in the bytes left: a string longer than
    /// the remainder, or a list whose `N - 1` children cannot each take a
    /// minimum node.
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
    /// The member's frame is not the measured one: the root is not a
    /// one-child list, the record list or a record is not a list, or a record
    /// position holds the wrong kind of node (a key that is not a string, a
    /// value that is not a list, or an odd child count).
    NotAMember {
        /// Where the offending node starts.
        offset: u64,
        /// Which frame rule the node broke.
        reason: &'static str,
    },
    /// A record key outside the 26 measured.
    UnknownKey {
        /// The key found.
        key: String,
    },
    /// A key stated twice in one record.
    DuplicateKey {
        /// The key repeated.
        key: &'static str,
    },
    /// A key every measured record states is absent from this one.
    MissingKey {
        /// The absent key.
        key: &'static str,
    },
    /// A key's value is not the shape [`ZeppelinKey::value_shape`] states.
    WrongValueShape {
        /// The key whose value was refused.
        key: &'static str,
    },
}

impl fmt::Display for ZeppelinsError {
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
            Self::NotAMember { offset, reason } => {
                write!(
                    f,
                    "not the member's measured frame at byte {offset}: {reason}"
                )
            }
            Self::UnknownKey { key } => write!(f, "unknown key {key:?}"),
            Self::DuplicateKey { key } => write!(f, "key {key} stated twice"),
            Self::MissingKey { key } => {
                write!(f, "key {key} is absent, which every measured record states")
            }
            Self::WrongValueShape { key } => {
                write!(f, "the value of {key} is not a {}", shape_of(key))
            }
        }
    }
}

fn shape_of(key: &str) -> &'static str {
    ZeppelinKey::from_spelling(key).map_or("measured shape", ZeppelinKey::value_shape)
}

impl std::error::Error for ZeppelinsError {}

/// One decoded `zeppelins.zrd` record.
#[derive(Clone, Debug, PartialEq)]
pub struct ZeppelinRecord {
    keys: Vec<ZeppelinKey>,
    node: String,
    position: [f32; 3],
    yaw: f32,
    pitch: f32,
    max_speed: f32,
    max_accel: f32,
    accel_pitch: f32,
    accel_yaw: f32,
    max_rate_yaw: f32,
    max_rate_pitch: f32,
    min_pitch: f32,
    max_pitch: f32,
    net: String,
    healthy: Vec<HealthyBinding>,
    num_healthy_required: u32,
    engines: Vec<String>,
    deactivated: Option<u32>,
    team: Option<String>,
    targets: Option<Vec<String>>,
    gasbags: Option<Vec<GasbagRow>>,
    cannon_fire_delay: Option<f32>,
    cannon_fire_range: Option<f32>,
    cannon_inaccuracy: Option<f32>,
    left_cannons: Option<Vec<CannonBinding>>,
    right_cannons: Option<Vec<CannonBinding>>,
    cannon_health: Option<Vec<CannonHealthRow>>,
}

impl ZeppelinRecord {
    /// The keys the record stated, in stored order.
    pub fn keys(&self) -> &[ZeppelinKey] {
        &self.keys
    }

    /// The `node` spelling: the record's world-node binding.
    pub fn node(&self) -> &str {
        &self.node
    }

    /// The `position` triple.
    pub const fn position(&self) -> [f32; 3] {
        self.position
    }

    /// The `yaw` float.
    pub const fn yaw(&self) -> f32 {
        self.yaw
    }

    /// The `pitch` float.
    pub const fn pitch(&self) -> f32 {
        self.pitch
    }

    /// The `max_speed` float.
    pub const fn max_speed(&self) -> f32 {
        self.max_speed
    }

    /// The `max_accel` float.
    pub const fn max_accel(&self) -> f32 {
        self.max_accel
    }

    /// The `accel_pitch` float.
    pub const fn accel_pitch(&self) -> f32 {
        self.accel_pitch
    }

    /// The `accel_yaw` float.
    pub const fn accel_yaw(&self) -> f32 {
        self.accel_yaw
    }

    /// The `max_rate_yaw` float.
    pub const fn max_rate_yaw(&self) -> f32 {
        self.max_rate_yaw
    }

    /// The `max_rate_pitch` float.
    pub const fn max_rate_pitch(&self) -> f32 {
        self.max_rate_pitch
    }

    /// The `min_pitch` float.
    pub const fn min_pitch(&self) -> f32 {
        self.min_pitch
    }

    /// The `max_pitch` float.
    pub const fn max_pitch(&self) -> f32 {
        self.max_pitch
    }

    /// The `net` spelling: the record's net name.
    pub fn net(&self) -> &str {
        &self.net
    }

    /// The `healthy` bindings.
    pub fn healthy(&self) -> &[HealthyBinding] {
        &self.healthy
    }

    /// The `num_healthy_required` integer.
    pub const fn num_healthy_required(&self) -> u32 {
        self.num_healthy_required
    }

    /// The `engines` node names.
    pub fn engines(&self) -> &[String] {
        &self.engines
    }

    /// The `deactivated` integer, when the record states the key.
    pub const fn deactivated(&self) -> Option<u32> {
        self.deactivated
    }

    /// The `team` spelling, when the record states the key.
    pub fn team(&self) -> Option<&str> {
        self.team.as_deref()
    }

    /// The `team` spelling resolved to a [`MeasuredTeam`], or `None` when the
    /// record states no `team` or an unmeasured one.
    pub fn measured_team(&self) -> Option<MeasuredTeam> {
        self.team.as_deref().and_then(MeasuredTeam::from_spelling)
    }

    /// The `targets` list, when the record states the key — including when it
    /// states an empty one (the eight measured instant-action records).
    pub fn targets(&self) -> Option<&[String]> {
        self.targets.as_deref()
    }

    /// The `gasbags` rows, when the record states the key.
    pub fn gasbags(&self) -> Option<&[GasbagRow]> {
        self.gasbags.as_deref()
    }

    /// The `cannon_fire_delay` float, when the record states the key.
    pub const fn cannon_fire_delay(&self) -> Option<f32> {
        self.cannon_fire_delay
    }

    /// The `cannon_fire_range` float, when the record states the key.
    pub const fn cannon_fire_range(&self) -> Option<f32> {
        self.cannon_fire_range
    }

    /// The `cannon_inaccuracy` float, when the record states the key.
    pub const fn cannon_inaccuracy(&self) -> Option<f32> {
        self.cannon_inaccuracy
    }

    /// The `left_cannons` bindings, when the record states the key.
    pub fn left_cannons(&self) -> Option<&[CannonBinding]> {
        self.left_cannons.as_deref()
    }

    /// The `right_cannons` bindings, when the record states the key.
    pub fn right_cannons(&self) -> Option<&[CannonBinding]> {
        self.right_cannons.as_deref()
    }

    /// The `cannon_health` rows, when the record states the key.
    pub fn cannon_health(&self) -> Option<&[CannonHealthRow]> {
        self.cannon_health.as_deref()
    }
}

/// One decoded `zeppelins.zrd` member: the record list it carries.
#[derive(Clone, Debug, PartialEq)]
pub struct ZeppelinMember {
    records: Vec<ZeppelinRecord>,
}

impl ZeppelinMember {
    /// The member's records, in stored order.
    pub fn records(&self) -> &[ZeppelinRecord] {
        &self.records
    }

    /// How many records the member carries.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the member carries no record: the `c*/mp1` and `c*/mp2`
    /// members' measured state.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

enum Node {
    Int(u32),
    Float(f32),
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

    fn word(&mut self) -> Result<u32, ZeppelinsError> {
        let end = self.position + 4;
        let slice = self
            .bytes
            .get(self.position..end)
            .ok_or(ZeppelinsError::Truncated {
                offset: self.offset(),
            })?;
        self.position = end;
        Ok(u32::from_le_bytes(slice.try_into().expect("four bytes")))
    }

    fn node(&mut self, depth: u32) -> Result<Node, ZeppelinsError> {
        let start = self.offset();
        if depth > MAX_DEPTH {
            return Err(ZeppelinsError::DepthExceeded { offset: start });
        }
        match self.word()? {
            TAG_INT => Ok(Node::Int(self.word()?)),
            TAG_FLOAT => Ok(Node::Float(f32::from_bits(self.word()?))),
            TAG_TEXT => {
                let declared = self.word()?;
                let length = declared as usize;
                if length > self.remaining() {
                    return Err(ZeppelinsError::LengthDoesNotFit {
                        offset: start,
                        declared,
                        remaining: self.remaining() as u64,
                    });
                }
                let body = &self.bytes[self.position..self.position + length];
                self.position += length;
                let text = std::str::from_utf8(body)
                    .map_err(|_| ZeppelinsError::InvalidText { offset: start })?;
                Ok(Node::Text(text.to_owned()))
            }
            TAG_LIST => {
                let declared = self.word()?;
                let children = (declared as usize)
                    .checked_sub(1)
                    .ok_or(ZeppelinsError::ZeroListWord { offset: start })?;
                if children > self.remaining() / MIN_NODE_BYTES {
                    return Err(ZeppelinsError::LengthDoesNotFit {
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
            tag => Err(ZeppelinsError::UndefinedTag { offset: start, tag }),
        }
    }
}

fn wrong(key: ZeppelinKey) -> ZeppelinsError {
    ZeppelinsError::WrongValueShape {
        key: key.spelling(),
    }
}

fn one_text(key: ZeppelinKey, value: Node) -> Result<String, ZeppelinsError> {
    let Node::List(items) = value else {
        return Err(wrong(key));
    };
    match <[Node; 1]>::try_from(items) {
        Ok([Node::Text(text)]) => Ok(text),
        _ => Err(wrong(key)),
    }
}

fn one_int(key: ZeppelinKey, value: Node) -> Result<u32, ZeppelinsError> {
    let Node::List(items) = value else {
        return Err(wrong(key));
    };
    match <[Node; 1]>::try_from(items) {
        Ok([Node::Int(value)]) => Ok(value),
        _ => Err(wrong(key)),
    }
}

fn one_float(key: ZeppelinKey, value: Node) -> Result<f32, ZeppelinsError> {
    let Node::List(items) = value else {
        return Err(wrong(key));
    };
    match <[Node; 1]>::try_from(items) {
        Ok([Node::Float(value)]) => Ok(value),
        _ => Err(wrong(key)),
    }
}

fn three_floats(key: ZeppelinKey, value: Node) -> Result<[f32; 3], ZeppelinsError> {
    let Node::List(items) = value else {
        return Err(wrong(key));
    };
    match <[Node; 3]>::try_from(items) {
        Ok([Node::Float(x), Node::Float(y), Node::Float(z)]) => Ok([x, y, z]),
        _ => Err(wrong(key)),
    }
}

fn text_list(key: ZeppelinKey, value: Node) -> Result<Vec<String>, ZeppelinsError> {
    let Node::List(items) = value else {
        return Err(wrong(key));
    };
    items
        .into_iter()
        .map(|item| match item {
            Node::Text(text) => Ok(text),
            _ => Err(wrong(key)),
        })
        .collect()
}

fn healthy_rows(key: ZeppelinKey, value: Node) -> Result<Vec<HealthyBinding>, ZeppelinsError> {
    let Node::List(items) = value else {
        return Err(wrong(key));
    };
    items
        .into_iter()
        .map(|item| match <[Node; 2]>::try_from(item_list(key, item)?) {
            Ok([Node::Text(node), Node::Text(attachment)]) => {
                Ok(HealthyBinding { node, attachment })
            }
            _ => Err(wrong(key)),
        })
        .collect()
}

fn item_list(key: ZeppelinKey, item: Node) -> Result<Vec<Node>, ZeppelinsError> {
    match item {
        Node::List(children) => Ok(children),
        _ => Err(wrong(key)),
    }
}

fn cannon_rows(key: ZeppelinKey, value: Node) -> Result<Vec<CannonBinding>, ZeppelinsError> {
    let Node::List(items) = value else {
        return Err(wrong(key));
    };
    items
        .into_iter()
        .map(|item| match <[Node; 3]>::try_from(item_list(key, item)?) {
            Ok([Node::Text(node), Node::Text(deploy), Node::Text(retract)]) => Ok(CannonBinding {
                node,
                deploy,
                retract,
            }),
            _ => Err(wrong(key)),
        })
        .collect()
}

fn gasbag_rows(key: ZeppelinKey, value: Node) -> Result<Vec<GasbagRow>, ZeppelinsError> {
    let Node::List(items) = value else {
        return Err(wrong(key));
    };
    items
        .into_iter()
        .map(|item| {
            let mut row = item_list(key, item)?.into_iter();
            let (Some(Node::Text(node)), Some(Node::Float(value)), Some(torpedo)) =
                (row.next(), row.next(), row.next())
            else {
                return Err(wrong(key));
            };
            let torpedo = match <[Node; 1]>::try_from(item_list(key, torpedo)?) {
                Ok([Node::Text(text)]) => text,
                _ => return Err(wrong(key)),
            };
            let attachment = match (row.next(), row.next()) {
                (None, None) => None,
                (Some(Node::Text(text)), None) => Some(text),
                _ => return Err(wrong(key)),
            };
            Ok(GasbagRow {
                node,
                value,
                torpedo,
                attachment,
            })
        })
        .collect()
}

fn cannon_health_rows(
    key: ZeppelinKey,
    value: Node,
) -> Result<Vec<CannonHealthRow>, ZeppelinsError> {
    let Node::List(items) = value else {
        return Err(wrong(key));
    };
    items
        .into_iter()
        .map(|item| {
            let Ok(
                [
                    Node::Text(node),
                    Node::Text(mount),
                    Node::Text(frame),
                    Node::Text(gasbag),
                    Node::Float(value),
                    destroy,
                    states,
                ],
            ) = <[Node; 7]>::try_from(item_list(key, item)?)
            else {
                return Err(wrong(key));
            };
            let destroy = match <[Node; 1]>::try_from(item_list(key, destroy)?) {
                Ok([Node::Text(text)]) => text,
                _ => return Err(wrong(key)),
            };
            let states = <[Node; 2]>::try_from(item_list(key, states)?)
                .map_err(|_| wrong(key))?
                .into_iter()
                .map(|state| match state {
                    Node::List(pair) => match <[Node; 2]>::try_from(pair) {
                        Ok([Node::Float(fraction), Node::Text(spelling)]) => {
                            Ok((fraction, spelling))
                        }
                        _ => Err(wrong(key)),
                    },
                    _ => Err(wrong(key)),
                })
                .collect::<Result<Vec<(f32, String)>, ZeppelinsError>>()?;
            let states: [(f32, String); 2] = states.try_into().map_err(|_| wrong(key))?;
            Ok(CannonHealthRow {
                node,
                mount,
                frame,
                gasbag,
                value,
                destroy,
                states,
            })
        })
        .collect()
}

fn record(children: Vec<Node>) -> Result<ZeppelinRecord, ZeppelinsError> {
    if !children.len().is_multiple_of(2) {
        return Err(ZeppelinsError::NotAMember {
            offset: 0,
            reason: "a record's children must alternate a key and a value",
        });
    }
    let mut out = ZeppelinRecord {
        keys: Vec::with_capacity(children.len() / 2),
        node: String::new(),
        position: [0.0; 3],
        yaw: 0.0,
        pitch: 0.0,
        max_speed: 0.0,
        max_accel: 0.0,
        accel_pitch: 0.0,
        accel_yaw: 0.0,
        max_rate_yaw: 0.0,
        max_rate_pitch: 0.0,
        min_pitch: 0.0,
        max_pitch: 0.0,
        net: String::new(),
        healthy: Vec::new(),
        num_healthy_required: 0,
        engines: Vec::new(),
        deactivated: None,
        team: None,
        targets: None,
        gasbags: None,
        cannon_fire_delay: None,
        cannon_fire_range: None,
        cannon_inaccuracy: None,
        left_cannons: None,
        right_cannons: None,
        cannon_health: None,
    };
    let mut children = children.into_iter();
    while let (Some(key), Some(value)) = (children.next(), children.next()) {
        let Node::Text(spelling) = key else {
            return Err(ZeppelinsError::NotAMember {
                offset: 0,
                reason: "a record key must be a text node",
            });
        };
        let key = ZeppelinKey::from_spelling(&spelling)
            .ok_or(ZeppelinsError::UnknownKey { key: spelling })?;
        if out.keys.contains(&key) {
            return Err(ZeppelinsError::DuplicateKey {
                key: key.spelling(),
            });
        }
        let Node::List(_) = value else {
            return Err(ZeppelinsError::NotAMember {
                offset: 0,
                reason: "a record value must be a list node",
            });
        };
        out.keys.push(key);
        match key {
            ZeppelinKey::Deactivated => out.deactivated = Some(one_int(key, value)?),
            ZeppelinKey::Node => out.node = one_text(key, value)?,
            ZeppelinKey::Team => out.team = Some(one_text(key, value)?),
            ZeppelinKey::Position => out.position = three_floats(key, value)?,
            ZeppelinKey::Yaw => out.yaw = one_float(key, value)?,
            ZeppelinKey::Pitch => out.pitch = one_float(key, value)?,
            ZeppelinKey::MaxSpeed => out.max_speed = one_float(key, value)?,
            ZeppelinKey::MaxAccel => out.max_accel = one_float(key, value)?,
            ZeppelinKey::AccelPitch => out.accel_pitch = one_float(key, value)?,
            ZeppelinKey::AccelYaw => out.accel_yaw = one_float(key, value)?,
            ZeppelinKey::MaxRateYaw => out.max_rate_yaw = one_float(key, value)?,
            ZeppelinKey::MaxRatePitch => out.max_rate_pitch = one_float(key, value)?,
            ZeppelinKey::MinPitch => out.min_pitch = one_float(key, value)?,
            ZeppelinKey::MaxPitch => out.max_pitch = one_float(key, value)?,
            ZeppelinKey::Net => out.net = one_text(key, value)?,
            ZeppelinKey::Healthy => out.healthy = healthy_rows(key, value)?,
            ZeppelinKey::NumHealthyRequired => {
                out.num_healthy_required = one_int(key, value)?;
            }
            ZeppelinKey::Engines => out.engines = text_list(key, value)?,
            ZeppelinKey::CannonFireDelay => {
                out.cannon_fire_delay = Some(one_float(key, value)?);
            }
            ZeppelinKey::CannonFireRange => {
                out.cannon_fire_range = Some(one_float(key, value)?);
            }
            ZeppelinKey::CannonInaccuracy => {
                out.cannon_inaccuracy = Some(one_float(key, value)?);
            }
            ZeppelinKey::LeftCannons => out.left_cannons = Some(cannon_rows(key, value)?),
            ZeppelinKey::RightCannons => out.right_cannons = Some(cannon_rows(key, value)?),
            ZeppelinKey::Targets => out.targets = Some(text_list(key, value)?),
            ZeppelinKey::Gasbags => out.gasbags = Some(gasbag_rows(key, value)?),
            ZeppelinKey::CannonHealth => out.cannon_health = Some(cannon_health_rows(key, value)?),
        }
    }
    for key in ZeppelinKey::ALL {
        if key.required() && !out.keys.contains(&key) {
            return Err(ZeppelinsError::MissingKey {
                key: key.spelling(),
            });
        }
    }
    Ok(out)
}

/// Decodes one `zeppelins.zrd` member through the grammar measured over all
/// 50 retail members.
///
/// # Errors
///
/// [`ZeppelinsError`], by name: `Truncated` for a word or body past the end,
/// `UndefinedTag` for a tag outside `1`/`2`/`3`/`4`, `LengthDoesNotFit` for a
/// length the remaining bytes cannot hold, `ZeroListWord` for a list word of
/// `0`, `InvalidText` for non-UTF-8 text, `DepthExceeded` for nesting past the
/// measured depth, `TrailingBytes` for bytes after the root, `NotAMember` for
/// a frame outside the measured shape, and the record refusals
/// (`UnknownKey`, `DuplicateKey`, `MissingKey`, `WrongValueShape`) for a
/// record outside the measured vocabulary.
pub fn read_zeppelins_member(bytes: &[u8]) -> Result<ZeppelinMember, ZeppelinsError> {
    let mut cursor = Cursor { bytes, position: 0 };
    let root = cursor.node(0)?;
    if cursor.remaining() != 0 {
        return Err(ZeppelinsError::TrailingBytes {
            offset: cursor.offset(),
            count: cursor.remaining() as u64,
        });
    }
    let Node::List(frame) = root else {
        return Err(ZeppelinsError::NotAMember {
            offset: 0,
            reason: "the root must be a list",
        });
    };
    let Ok([Node::List(records)]) = <[Node; 1]>::try_from(frame) else {
        return Err(ZeppelinsError::NotAMember {
            offset: 0,
            reason: "the root must hold exactly one record list",
        });
    };
    let mut decoded = Vec::with_capacity(records.len());
    for row in records {
        let Node::List(children) = row else {
            return Err(ZeppelinsError::NotAMember {
                offset: 0,
                reason: "a record must be a list",
            });
        };
        decoded.push(record(children)?);
    }
    Ok(ZeppelinMember { records: decoded })
}
