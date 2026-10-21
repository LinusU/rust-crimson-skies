//! Fire, smoke, flak and damage effect definitions read from the reader
//! archive's animation members (task #1805, `PARITY2-DMG-FX-A`).
//!
//! The effects of the wing fire, the smoke trails and the flak puffs are not
//! drawn from a separate emitter table. They are `ANIMATION_DEFINITION`
//! records of `ZBD/zrdr.zbd` (`pufftrails.zrd`, `flame_ball.zrd`,
//! `partial_damage.zrd`, `player.zrd`, ...), and a `PUFFER_STATE` event inside
//! a sequence carries the whole parameter set of one particle emitter. The
//! compiled `cam_anim.zbd` record repeats the same floats in its event stream
//! (cross-checked by the retail test, not re-decoded here).
//!
//! This module gives the token tree of [`super::reader_tokens`] a typed shape:
//! [`EffectAnimation`] > [`EffectSequence`] > [`EffectEvent`], where the three
//! event kinds the fire/smoke/flak effects are made of — [`PufferState`],
//! [`LightState`] and [`CallAnimation`] — are typed field by field, and every
//! other event is kept as its keyword and raw value list. A keyword a typed
//! event does not know is listed in its `other_fields`, never dropped.
//!
//! Units and meanings are the keyword spellings only. Nothing here says what a
//! `DISTANCE_INTERVAL` of `1` is measured in, or how `FADE_RANGE` fades; those
//! are recorded as unknown in
//! `docs/findings/2026-10-21-parity2-dmg-fx-a-effect-definitions.md`.
//!
//! [`read_vehicle_records`] reads the other half of "which effect plays at
//! which damage level": a vehicle's `injure_anims` threshold list.

use std::fmt;

use super::reader_tokens::ReaderNode;

/// Why a record did not have the shape this module reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EffectDefError {
    /// The member is not `(( "ANIMATION_DEFINITIONS" ( "ANIMATION_LIST" (...) ) ))`.
    NotAnAnimationMember,
    /// A field's value list had the wrong number or kind of values.
    FieldShape {
        /// The keyword whose value was malformed.
        keyword: String,
        /// What was expected.
        expected: &'static str,
    },
    /// The member is not a vehicle table.
    NotAVehicleTable,
}

impl fmt::Display for EffectDefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnAnimationMember => f.write_str("not an ANIMATION_DEFINITIONS member"),
            Self::FieldShape { keyword, expected } => {
                write!(f, "field {keyword}: expected {expected}")
            }
            Self::NotAVehicleTable => f.write_str("not a vehicle table"),
        }
    }
}

impl std::error::Error for EffectDefError {}

/// The `AT_NODE` anchor: a node name and the optional offset words after it.
///
/// Retail uses zero, three or six words after the name (`INPUT_NODE`,
/// `INPUT_NODE 0 0 0.2`, `pdp1 0 0 0 0 0 0`). What the six are (offset plus a
/// rotation is plausible) is unmeasured, so they are kept as given.
#[derive(Clone, Debug, PartialEq)]
pub struct AtNode<'a> {
    /// The node name; `INPUT_NODE` means the node the caller passes in.
    pub node: &'a str,
    /// The numbers after the name.
    pub words: Vec<f32>,
}

/// One `PUFFER_STATE` event: a particle emitter's parameters or its switch-off.
#[derive(Clone, Debug, PartialEq)]
pub struct PufferState<'a> {
    /// `NAME`: the emitter's identity, scoped to the carrying animation.
    pub name: &'a str,
    /// `ACTIVE_STATE`: `Some(true)` for `ACTIVE`, `Some(false)` for `INACTIVE`.
    pub active: Option<bool>,
    /// `PRIORITY`.
    pub priority: Option<f32>,
    /// `AT_NODE`.
    pub at_node: Option<AtNode<'a>>,
    /// `DISTANCE_INTERVAL`.
    pub distance_interval: Option<f32>,
    /// `TIME_INTERVAL`.
    pub time_interval: Option<f32>,
    /// `LOCAL_VELOCITY`.
    pub local_velocity: Option<[f32; 3]>,
    /// `WORLD_VELOCITY`.
    pub world_velocity: Option<[f32; 3]>,
    /// `MIN_RANDOM_VELOCITY`.
    pub min_random_velocity: Option<[f32; 3]>,
    /// `MAX_RANDOM_VELOCITY`.
    pub max_random_velocity: Option<[f32; 3]>,
    /// `WORLD_ACCELERATION`.
    pub world_acceleration: Option<[f32; 3]>,
    /// `FRICTION`.
    pub friction: Option<f32>,
    /// `SIZE_RANGE`.
    pub size_range: Option<[f32; 2]>,
    /// `NUMBER`: particles per emission.
    pub number: Option<f32>,
    /// `LIFETIME_RANGE`.
    pub lifetime_range: Option<[f32; 2]>,
    /// `GROWTH_FACTOR`.
    pub growth_factor: Option<f32>,
    /// `DEVIATION_DISTANCE`.
    pub deviation_distance: Option<f32>,
    /// `FADE_RANGE`.
    pub fade_range: Option<[f32; 2]>,
    /// `NEAR_FADE`.
    pub near_fade: Option<[f32; 2]>,
    /// `TEXTURES`: texture-cycle names, no times.
    pub textures: Vec<&'a str>,
    /// `TEXTURE_SEQUENCE`: `(time, texture)` frames.
    pub texture_sequence: Vec<(f32, &'a str)>,
    /// `COLORS`: `[time, r, g, b, a]` keys (r, g, b run 0..255 in retail).
    pub colors: Vec<[f32; 5]>,
    /// `START_TIME`: `(clock, seconds)`, e.g. `("EVENT_OFFSET", 0.3)`.
    pub start_time: Option<(&'a str, f32)>,
    /// Keywords of this event that no field above reads.
    pub other_fields: Vec<&'a str>,
}

/// One `LIGHT_STATE` event (the `fire_lt` light and the flak flash).
#[derive(Clone, Debug, PartialEq)]
pub struct LightState<'a> {
    /// `NAME`.
    pub name: &'a str,
    /// `ACTIVE_STATE`.
    pub active: Option<bool>,
    /// `AT_NODE`.
    pub at_node: Option<AtNode<'a>>,
    /// `RANGE`.
    pub range: Option<[f32; 2]>,
    /// `COLOR`: three components, 0..1 in retail.
    pub color: Option<[f32; 3]>,
    /// `SATURATED`.
    pub saturated: Option<bool>,
    /// Keywords of this event that no field above reads.
    pub other_fields: Vec<&'a str>,
}

/// One `CALL_ANIMATION` event: another animation played on a node.
#[derive(Clone, Debug, PartialEq)]
pub struct CallAnimation<'a> {
    /// `NAME`: the called animation's id.
    pub name: &'a str,
    /// `LOCAL_NAME`.
    pub local_name: Option<&'a str>,
    /// `WITH_NODE`: the node the callee binds `INPUT_NODE` to.
    pub with_node: Option<&'a str>,
    /// `AT_NODE`.
    pub at_node: Option<AtNode<'a>>,
    /// `START_TIME`: `(clock, seconds)`.
    pub start_time: Option<(&'a str, f32)>,
    /// Keywords of this event that no field above reads.
    pub other_fields: Vec<&'a str>,
}

/// One event of a sequence.
#[derive(Clone, Debug, PartialEq)]
pub enum EffectEvent<'a> {
    /// `PUFFER_STATE`.
    Puffer(Box<PufferState<'a>>),
    /// `LIGHT_STATE`.
    Light(LightState<'a>),
    /// `CALL_ANIMATION`.
    CallAnimation(CallAnimation<'a>),
    /// Any other event, verbatim.
    Other {
        /// The event keyword (`IF`, `LOOP`, `CALL_SEQUENCE`, ...).
        keyword: &'a str,
        /// Its value list.
        value: ReaderNode<'a>,
    },
}

/// One `SEQUENCE_DEFINITION`.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectSequence<'a> {
    /// `NAME`, when the sequence has one.
    pub name: Option<&'a str>,
    /// The events in stored order (conditions and loops are events).
    pub events: Vec<EffectEvent<'a>>,
}

/// One `ANIMATION_DEFINITION`.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectAnimation<'a> {
    /// `NAME`: for most records the scene object the animation is authored on.
    pub name: &'a str,
    /// `ANIMATION_NAME`: the id other records call, when it differs from `NAME`.
    pub animation_name: Option<&'a str>,
    /// `ACTIVATION`.
    pub activation: Option<&'a str>,
    /// `RESET_TIME` (one or two numbers).
    pub reset_time: Vec<f32>,
    /// The sequences in stored order.
    pub sequences: Vec<EffectSequence<'a>>,
}

impl<'a> EffectAnimation<'a> {
    /// The id other records call this animation by: `ANIMATION_NAME` when
    /// present, else `NAME`.
    pub fn id(&self) -> &'a str {
        self.animation_name.unwrap_or(self.name)
    }

    /// Every puffer event of every sequence, in stored order.
    pub fn puffers(&self) -> impl Iterator<Item = &PufferState<'a>> + '_ {
        self.sequences
            .iter()
            .flat_map(|sequence| sequence.events.iter())
            .filter_map(|event| match event {
                EffectEvent::Puffer(puffer) => Some(&**puffer),
                _ => None,
            })
    }

    /// Every light event of every sequence, in stored order.
    pub fn lights(&self) -> impl Iterator<Item = &LightState<'a>> + '_ {
        self.sequences
            .iter()
            .flat_map(|sequence| sequence.events.iter())
            .filter_map(|event| match event {
                EffectEvent::Light(light) => Some(light),
                _ => None,
            })
    }

    /// Every `CALL_ANIMATION` event of every sequence, in stored order.
    pub fn calls(&self) -> impl Iterator<Item = &CallAnimation<'a>> + '_ {
        self.sequences
            .iter()
            .flat_map(|sequence| sequence.events.iter())
            .filter_map(|event| match event {
                EffectEvent::CallAnimation(call) => Some(call),
                _ => None,
            })
    }
}

fn shape(keyword: &str, expected: &'static str) -> EffectDefError {
    EffectDefError::FieldShape {
        keyword: keyword.to_owned(),
        expected,
    }
}

fn children<'n, 'a>(value: &'n ReaderNode<'a>) -> &'n [ReaderNode<'a>] {
    value.as_list().unwrap_or(&[])
}

fn number(keyword: &str, value: &ReaderNode<'_>) -> Result<f32, EffectDefError> {
    match children(value) {
        [one] => one.as_number().ok_or_else(|| shape(keyword, "one number")),
        _ => Err(shape(keyword, "one number")),
    }
}

fn numbers<const N: usize>(
    keyword: &str,
    value: &ReaderNode<'_>,
    expected: &'static str,
) -> Result<[f32; N], EffectDefError> {
    let list = children(value);
    if list.len() != N {
        return Err(shape(keyword, expected));
    }
    let mut out = [0.0; N];
    for (slot, node) in out.iter_mut().zip(list) {
        *slot = node.as_number().ok_or_else(|| shape(keyword, expected))?;
    }
    Ok(out)
}

fn word<'a>(keyword: &str, value: &ReaderNode<'a>) -> Result<&'a str, EffectDefError> {
    match children(value) {
        [one] => one.as_str().ok_or_else(|| shape(keyword, "one string")),
        _ => Err(shape(keyword, "one string")),
    }
}

fn at_node<'a>(value: &ReaderNode<'a>) -> Result<AtNode<'a>, EffectDefError> {
    let list = children(value);
    let node = list
        .first()
        .and_then(ReaderNode::as_str)
        .ok_or_else(|| shape("AT_NODE", "a node name first"))?;
    let words = list[1..]
        .iter()
        .map(|word| {
            word.as_number()
                .ok_or_else(|| shape("AT_NODE", "numbers after the name"))
        })
        .collect::<Result<_, _>>()?;
    Ok(AtNode { node, words })
}

fn start_time<'a>(value: &ReaderNode<'a>) -> Result<(&'a str, f32), EffectDefError> {
    match children(value) {
        [clock, seconds] => Ok((
            clock
                .as_str()
                .ok_or_else(|| shape("START_TIME", "a clock name"))?,
            seconds
                .as_number()
                .ok_or_else(|| shape("START_TIME", "a number"))?,
        )),
        _ => Err(shape("START_TIME", "(clock seconds)")),
    }
}

fn active_state(value: &ReaderNode<'_>) -> Result<bool, EffectDefError> {
    match word("ACTIVE_STATE", value)? {
        "ACTIVE" => Ok(true),
        "INACTIVE" => Ok(false),
        _ => Err(shape("ACTIVE_STATE", "ACTIVE or INACTIVE")),
    }
}

fn puffer<'a>(value: &ReaderNode<'a>) -> Result<PufferState<'a>, EffectDefError> {
    let mut out = PufferState {
        name: "",
        active: None,
        priority: None,
        at_node: None,
        distance_interval: None,
        time_interval: None,
        local_velocity: None,
        world_velocity: None,
        min_random_velocity: None,
        max_random_velocity: None,
        world_acceleration: None,
        friction: None,
        size_range: None,
        number: None,
        lifetime_range: None,
        growth_factor: None,
        deviation_distance: None,
        fade_range: None,
        near_fade: None,
        textures: Vec::new(),
        texture_sequence: Vec::new(),
        colors: Vec::new(),
        start_time: None,
        other_fields: Vec::new(),
    };
    let v3 = "three numbers";
    let v2 = "two numbers";
    for (keyword, value) in value.fields().unwrap_or_default() {
        let Some(keyword) = keyword else { continue };
        match keyword {
            "NAME" => out.name = word(keyword, value)?,
            "ACTIVE_STATE" => out.active = Some(active_state(value)?),
            "PRIORITY" => out.priority = Some(number(keyword, value)?),
            "AT_NODE" => out.at_node = Some(at_node(value)?),
            "DISTANCE_INTERVAL" => out.distance_interval = Some(number(keyword, value)?),
            "TIME_INTERVAL" => out.time_interval = Some(number(keyword, value)?),
            "LOCAL_VELOCITY" => out.local_velocity = Some(numbers(keyword, value, v3)?),
            "WORLD_VELOCITY" => out.world_velocity = Some(numbers(keyword, value, v3)?),
            "MIN_RANDOM_VELOCITY" => out.min_random_velocity = Some(numbers(keyword, value, v3)?),
            "MAX_RANDOM_VELOCITY" => out.max_random_velocity = Some(numbers(keyword, value, v3)?),
            "WORLD_ACCELERATION" => out.world_acceleration = Some(numbers(keyword, value, v3)?),
            "FRICTION" => out.friction = Some(number(keyword, value)?),
            "SIZE_RANGE" => out.size_range = Some(numbers(keyword, value, v2)?),
            "NUMBER" => out.number = Some(number(keyword, value)?),
            "LIFETIME_RANGE" => out.lifetime_range = Some(numbers(keyword, value, v2)?),
            "GROWTH_FACTOR" => out.growth_factor = Some(number(keyword, value)?),
            "DEVIATION_DISTANCE" => out.deviation_distance = Some(number(keyword, value)?),
            "FADE_RANGE" => out.fade_range = Some(numbers(keyword, value, v2)?),
            "NEAR_FADE" => out.near_fade = Some(numbers(keyword, value, v2)?),
            "START_TIME" => out.start_time = Some(start_time(value)?),
            "TEXTURES" => {
                out.textures = children(value)
                    .iter()
                    .map(|name| name.as_str().ok_or_else(|| shape(keyword, "strings")))
                    .collect::<Result<_, _>>()?;
            }
            "TEXTURE_SEQUENCE" => {
                out.texture_sequence = children(value)
                    .iter()
                    .map(|frame| match children(frame) {
                        [time, name] => Ok((
                            time.as_number()
                                .ok_or_else(|| shape(keyword, "(time name)"))?,
                            name.as_str().ok_or_else(|| shape(keyword, "(time name)"))?,
                        )),
                        _ => Err(shape(keyword, "(time name)")),
                    })
                    .collect::<Result<_, _>>()?;
            }
            "COLORS" => {
                out.colors = children(value)
                    .iter()
                    .map(|key| numbers::<5>(keyword, key, "(time r g b a)"))
                    .collect::<Result<_, _>>()?;
            }
            other => out.other_fields.push(other),
        }
    }
    Ok(out)
}

fn light<'a>(value: &ReaderNode<'a>) -> Result<LightState<'a>, EffectDefError> {
    let mut out = LightState {
        name: "",
        active: None,
        at_node: None,
        range: None,
        color: None,
        saturated: None,
        other_fields: Vec::new(),
    };
    for (keyword, value) in value.fields().unwrap_or_default() {
        let Some(keyword) = keyword else { continue };
        match keyword {
            "NAME" => out.name = word(keyword, value)?,
            "ACTIVE_STATE" => out.active = Some(active_state(value)?),
            "AT_NODE" => out.at_node = Some(at_node(value)?),
            "RANGE" => out.range = Some(numbers(keyword, value, "two numbers")?),
            "COLOR" => out.color = Some(numbers(keyword, value, "three numbers")?),
            "SATURATED" => {
                out.saturated = Some(match word(keyword, value)? {
                    "TRUE" => true,
                    "FALSE" => false,
                    _ => return Err(shape(keyword, "TRUE or FALSE")),
                });
            }
            other => out.other_fields.push(other),
        }
    }
    Ok(out)
}

fn call_animation<'a>(value: &ReaderNode<'a>) -> Result<CallAnimation<'a>, EffectDefError> {
    let mut out = CallAnimation {
        name: "",
        local_name: None,
        with_node: None,
        at_node: None,
        start_time: None,
        other_fields: Vec::new(),
    };
    for (keyword, value) in value.fields().unwrap_or_default() {
        let Some(keyword) = keyword else { continue };
        match keyword {
            "NAME" => out.name = word(keyword, value)?,
            "LOCAL_NAME" => out.local_name = Some(word(keyword, value)?),
            "WITH_NODE" => out.with_node = Some(word(keyword, value)?),
            "AT_NODE" => out.at_node = Some(at_node(value)?),
            "START_TIME" => out.start_time = Some(start_time(value)?),
            other => out.other_fields.push(other),
        }
    }
    Ok(out)
}

fn sequence<'a>(value: &ReaderNode<'a>) -> Result<EffectSequence<'a>, EffectDefError> {
    let mut name = None;
    let mut events = Vec::new();
    for (keyword, value) in value.fields().unwrap_or_default() {
        let Some(keyword) = keyword else { continue };
        match keyword {
            "NAME" => name = Some(word(keyword, value)?),
            "PUFFER_STATE" => events.push(EffectEvent::Puffer(Box::new(puffer(value)?))),
            "LIGHT_STATE" => events.push(EffectEvent::Light(light(value)?)),
            "CALL_ANIMATION" => events.push(EffectEvent::CallAnimation(call_animation(value)?)),
            other => events.push(EffectEvent::Other {
                keyword: other,
                value: value.clone(),
            }),
        }
    }
    Ok(EffectSequence { name, events })
}

fn animation<'a>(value: &ReaderNode<'a>) -> Result<EffectAnimation<'a>, EffectDefError> {
    let mut out = EffectAnimation {
        name: "",
        animation_name: None,
        activation: None,
        reset_time: Vec::new(),
        sequences: Vec::new(),
    };
    for (keyword, value) in value.fields().unwrap_or_default() {
        let Some(keyword) = keyword else { continue };
        match keyword {
            "NAME" => out.name = word(keyword, value)?,
            "ANIMATION_NAME" => out.animation_name = Some(word(keyword, value)?),
            "ACTIVATION" => out.activation = Some(word(keyword, value)?),
            "RESET_TIME" => {
                out.reset_time = children(value)
                    .iter()
                    .map(|n| n.as_number().ok_or_else(|| shape(keyword, "numbers")))
                    .collect::<Result<_, _>>()?;
            }
            "SEQUENCE_DEFINITION" => out.sequences.push(sequence(value)?),
            _ => {}
        }
    }
    Ok(out)
}

/// Reads one animation member's top-level nodes as its `ANIMATION_DEFINITION`
/// list: `(( "ANIMATION_DEFINITIONS" ( "ANIMATION_LIST" ( "ANIMATION_DEFINITION" (...) ... ) ) ))`.
///
/// # Errors
///
/// [`EffectDefError::NotAnAnimationMember`] when the member has another shape,
/// [`EffectDefError::FieldShape`] when a typed field is malformed.
pub fn read_animation_definitions<'a>(
    nodes: &[ReaderNode<'a>],
) -> Result<Vec<EffectAnimation<'a>>, EffectDefError> {
    let [root] = nodes else {
        return Err(EffectDefError::NotAnAnimationMember);
    };
    let record = children(root)
        .first()
        .ok_or(EffectDefError::NotAnAnimationMember)?;
    let definitions = record
        .field("ANIMATION_DEFINITIONS")
        .ok_or(EffectDefError::NotAnAnimationMember)?;
    let list = definitions
        .field("ANIMATION_LIST")
        .ok_or(EffectDefError::NotAnAnimationMember)?;
    list.fields()
        .unwrap_or_default()
        .into_iter()
        .filter(|(keyword, _)| *keyword == Some("ANIMATION_DEFINITION"))
        .map(|(_, value)| animation(value))
        .collect()
}

/// One `injure_anims` entry: at `threshold`, play `animation`.
#[derive(Clone, Debug, PartialEq)]
pub struct InjureAnim<'a> {
    /// The fraction the entry is keyed on (0.99, 0.72, 0.46, 0.2, 0.15, ...).
    /// Whether it compares against health remaining or damage taken is not
    /// stored in the entry.
    pub threshold: f32,
    /// The animation id.
    pub animation: &'a str,
    /// The node/record the animation is bound to, when stored (a part's
    /// entries name `player_pfighter`; the vehicle-level ones name none).
    pub target: Option<&'a str>,
}

/// One entry of a vehicle's `destroyable_parts`.
#[derive(Clone, Debug, PartialEq)]
pub struct DestroyablePart<'a> {
    /// The part name (`nose`, `tail`, `leftwing`, `rightwing`).
    pub name: &'a str,
    /// The numbers after the name (`25 25`, stored as floats, in the player's
    /// parts); their meaning is not stored beside them.
    pub numbers: Vec<f32>,
    /// The bare words (`critical`, `engine`).
    pub flags: Vec<&'a str>,
    /// `got_hit_anim`: `(animation, target)`.
    pub got_hit_anim: Option<(&'a str, Option<&'a str>)>,
    /// The part's `injure_anims`.
    pub injure_anims: Vec<InjureAnim<'a>>,
}

/// One vehicle record of `vehicle.zrd`, reduced to the damage-effect fields.
#[derive(Clone, Debug, PartialEq)]
pub struct VehicleRecord<'a> {
    /// The record key (`pdevastator`).
    pub key: &'a str,
    /// `nodename` (`player_pfighter`).
    pub node_name: Option<&'a str>,
    /// The vehicle-level `injure_anims`.
    pub injure_anims: Vec<InjureAnim<'a>>,
    /// `destroyable_parts`.
    pub destroyable_parts: Vec<DestroyablePart<'a>>,
}

fn injure_entry<'a>(entry: &ReaderNode<'a>) -> Result<InjureAnim<'a>, EffectDefError> {
    match children(entry) {
        [threshold, animation, rest @ ..] if rest.len() <= 1 => Ok(InjureAnim {
            threshold: threshold
                .as_number()
                .ok_or_else(|| shape("injure_anims", "(threshold animation [target])"))?,
            animation: animation
                .as_str()
                .ok_or_else(|| shape("injure_anims", "(threshold animation [target])"))?,
            target: rest.first().and_then(ReaderNode::as_str),
        }),
        _ => Err(shape("injure_anims", "(threshold animation [target])")),
    }
}

fn injure_list<'a>(value: &ReaderNode<'a>) -> Result<Vec<InjureAnim<'a>>, EffectDefError> {
    children(value).iter().map(injure_entry).collect()
}

fn part<'a>(entry: &ReaderNode<'a>) -> Result<DestroyablePart<'a>, EffectDefError> {
    let list = children(entry);
    let name = list
        .first()
        .and_then(ReaderNode::as_str)
        .ok_or_else(|| shape("destroyable_parts", "a part name first"))?;
    let mut out = DestroyablePart {
        name,
        numbers: Vec::new(),
        flags: Vec::new(),
        got_hit_anim: None,
        injure_anims: Vec::new(),
    };
    let mut index = 1;
    while index < list.len() {
        match (&list[index], list.get(index + 1)) {
            (number @ (ReaderNode::Int(_) | ReaderNode::Float(_)), _) => {
                out.numbers.extend(number.as_number());
            }
            (ReaderNode::String(_), Some(value @ ReaderNode::List(_))) => {
                let keyword = list[index].as_str().unwrap_or_default();
                match keyword {
                    "got_hit_anim" => {
                        let mut names = children(value).iter().filter_map(ReaderNode::as_str);
                        let animation = names
                            .next()
                            .ok_or_else(|| shape(keyword, "an animation id"))?;
                        out.got_hit_anim = Some((animation, names.next()));
                    }
                    "injure_anims" => out.injure_anims = injure_list(value)?,
                    _ => {}
                }
                index += 1;
            }
            (node @ ReaderNode::String(_), _) => out.flags.push(node.as_str().unwrap_or_default()),
            _ => {}
        }
        index += 1;
    }
    Ok(out)
}

/// Reads `vehicle.zrd` as a table of vehicle records and keeps the
/// damage-effect fields of each (`nodename`, `injure_anims`,
/// `destroyable_parts`).
///
/// # Errors
///
/// [`EffectDefError::NotAVehicleTable`] when the member is not a one-record
/// table, [`EffectDefError::FieldShape`] when an `injure_anims` entry is
/// malformed.
pub fn read_vehicle_records<'a>(
    nodes: &[ReaderNode<'a>],
) -> Result<Vec<VehicleRecord<'a>>, EffectDefError> {
    let [root] = nodes else {
        return Err(EffectDefError::NotAVehicleTable);
    };
    let table = children(root)
        .first()
        .ok_or(EffectDefError::NotAVehicleTable)?;
    let mut out = Vec::new();
    for (key, record) in table.fields().ok_or(EffectDefError::NotAVehicleTable)? {
        let Some(key) = key else { continue };
        let mut vehicle = VehicleRecord {
            key,
            node_name: None,
            injure_anims: Vec::new(),
            destroyable_parts: Vec::new(),
        };
        for (field, value) in record.fields().unwrap_or_default() {
            match field {
                Some("nodename") => {
                    vehicle.node_name = children(value).first().and_then(ReaderNode::as_str);
                }
                Some("injure_anims") => vehicle.injure_anims = injure_list(value)?,
                Some("destroyable_parts") => {
                    vehicle.destroyable_parts =
                        children(value).iter().map(part).collect::<Result<_, _>>()?;
                }
                _ => {}
            }
        }
        out.push(vehicle);
    }
    Ok(out)
}
