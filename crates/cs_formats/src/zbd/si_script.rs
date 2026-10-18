//! The spline-interp key data (`.zan` scripts) stored inline in an animation
//! container (task #1659, `PARITY-M01-ATMO-E1`).
//!
//! An `OBJECT_MOTION_SI_SCRIPT` event in `zbd/<group>/<mission>/mis_anim.zbd`
//! names a `.zan` script and the object it moves. The container carries the
//! script's keys **inline**, directly after the two NUL-terminated strings:
//!
//! ```text
//! path    NUL-terminated, "..\data\common\zrdr\zeps\<script>.zan"
//! object  NUL-terminated, the name of the object the script moves
//! key*    variable length, back to back, no count and no terminator
//! ```
//!
//! # The key record (measured)
//!
//! ```text
//! +0   u32   tag        0..=3, a bit mask: bit 0 = position block, bit 1 = orientation block
//! +4   f32   t0         seconds; the first key of a script starts at 0.0
//! +8   f32   t1         seconds; t1 > t0, and the next key's t0 is bit-identical to this t1
//! +12  position block      19 x f32, present when bit 0 is set
//! +..  orientation block   19 x f32, present when bit 1 is set
//! ```
//!
//! so a key is 12, 88, 88 or 164 bytes for tag 0, 1, 2, 3. All five record
//! sizes tile the retail scripts exactly: the byte after a script's last key
//! is the next script's path (or the end of what the container stores), with
//! no closing record.
//!
//! Each block is `start[3 or 4]`, `mean_rate[3]` and three cubic
//! coefficient groups `[c0, c1, c2, c3]`, one per axis:
//!
//! * **position block** — `start` is `x y z` followed by one word that holds
//!   uninitialised-looking garbage (denormals and 1.8e31 occur) and is never
//!   read; `mean_rate = (P(t1) - P(t0)) / (t1 - t0)` to rounding;
//!   `P_axis(s) = c0 + c1 s + c2 s^2 + c3 s^3` with `s = t - t0`, and `c0`
//!   equals the axis of `start`. `P(t1)` is the next key's `start` to 5e-7.
//! * **orientation block** — `start` is a unit quaternion `w x y z`;
//!   `mean_rate = r(t1 - t0) / (t1 - t0)` to 7e-9; each axis of the rotation
//!   vector `r(s) = c0 + c1 s + c2 s^2 + c3 s^3` has `c0 = 0`. The next key's
//!   `start` is `exp(r(t1 - t0)) * start` (or its negation: `q` and `-q` are
//!   one rotation and retail stores either), where `exp(v)` is the quaternion
//!   `(cos |v|, sin |v| v / |v|)` (so `|r|` is a **half** angle). **The fit is
//!   exact only for small rotations**: 4.4e-7 on `pzhookup_player`, but up to
//!   9.2e-4 over C3/M01 and 1.6e-2 over all 1068 retail scripts, where the
//!   half-angle reaches ~0.33. Whether the remainder is the exporter's own
//!   fitting error or a different map is **not decided**.
//!
//! The evaluators [`SiPositionBlock::at`] and [`SiOrientationBlock::at`] are
//! exactly those two measured relations. That they are what the original
//! engine evaluates is **not** established (no original run, no engine
//! disassembly was read); they reproduce the stored keys, which is the
//! evidence.
//!
//! # Unknown
//!
//! * the coordinate frame of the position block and the parent the object
//!   moves relative to (hook-ups name `hook_base`, `player`, `camera1`, ...);
//! * what a key without a block does: whether the object holds the last
//!   value, or whether the missing channel is unchanged in the original
//!   (only the tag-0 holds in `pzhookup_playerhold` and the tag-1/tag-2 keys
//!   of `pzhookup_impact` are measured here, not their effect);
//! * the meaning of the position block's fourth `start` word;
//! * the units (the position values are assumed metres only by the world's
//!   own scale, never measured against the original).
//!
//! Evidence: `docs/findings/2026-10-18-m01-si-script-key-layout.md`.

use std::fmt;

use cs_types::evidence::{ClaimStatus, SourceSpan};

/// The suffix that ends a script path.
const SUFFIX: &[u8] = b".zan\0";
/// Every measured script path starts with this.
const PATH_PREFIX: &[u8] = b"..\\";
/// The longest object name the scan accepts.
const MAX_OBJECT_BYTES: usize = 64;
/// The longest path the scan accepts.
const MAX_PATH_BYTES: usize = 160;
/// Bytes of one key's fixed head: tag, `t0`, `t1`.
pub const SI_KEY_HEAD_BYTES: usize = 12;
/// Bytes of one block (19 `f32`).
pub const SI_BLOCK_BYTES: usize = 76;
/// Tag bit for the position block.
pub const SI_TAG_POSITION: u32 = 1;
/// Tag bit for the orientation block.
pub const SI_TAG_ORIENTATION: u32 = 2;

/// Everything the reader decides from bytes it measured; nothing is
/// `verified_original`.
pub const SI_LAYOUT_EVIDENCE: ClaimStatus = ClaimStatus::ObservedTool;

/// Why a key list could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SiScriptError {
    /// No script of that name is stored in the container.
    NotFound { script: String },
    /// A script of that name is stored more than once and the caller did not
    /// say which object.
    Ambiguous {
        script: String,
        objects: Vec<String>,
    },
}

impl fmt::Display for SiScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { script } => write!(f, "no spline-interp script `{script}` is stored"),
            Self::Ambiguous { script, objects } => write!(
                f,
                "script `{script}` is stored for several objects {objects:?}; name the object"
            ),
        }
    }
}

impl std::error::Error for SiScriptError {}

/// The position channel of one key.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SiPositionBlock {
    /// `x y z` at `t0`.
    pub start: [f32; 3],
    /// The word after `z`; garbage in retail, never interpreted.
    pub padding_word: u32,
    /// `(P(t1) - P(t0)) / (t1 - t0)`.
    pub mean_rate: [f32; 3],
    /// Per axis `[c0, c1, c2, c3]` of `P(s)`.
    pub coefficients: [[f32; 4]; 3],
}

impl SiPositionBlock {
    /// `P(s)` for `s` seconds after the key's `t0`.
    pub fn at(&self, s: f32) -> [f32; 3] {
        self.coefficients.map(|c| cubic(c, s))
    }
}

/// The orientation channel of one key.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SiOrientationBlock {
    /// Unit quaternion `w x y z` at `t0`.
    pub start: [f32; 4],
    /// `r(t1 - t0) / (t1 - t0)`.
    pub mean_rate: [f32; 3],
    /// Per axis `[c0, c1, c2, c3]` of the half-angle rotation vector `r(s)`.
    pub coefficients: [[f32; 4]; 3],
}

impl SiOrientationBlock {
    /// `exp(r(s)) * start`, quaternion `w x y z`, for `s` seconds after `t0`.
    pub fn at(&self, s: f32) -> [f32; 4] {
        let r = self.coefficients.map(|c| cubic(c, s));
        let angle = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
        let (w1, x1, y1, z1) = if angle < 1e-12 {
            (1.0, 0.0, 0.0, 0.0)
        } else {
            let k = angle.sin() / angle;
            (angle.cos(), r[0] * k, r[1] * k, r[2] * k)
        };
        let [w2, x2, y2, z2] = self.start;
        [
            w1 * w2 - x1 * x2 - y1 * y2 - z1 * z2,
            w1 * x2 + x1 * w2 + y1 * z2 - z1 * y2,
            w1 * y2 - x1 * z2 + y1 * w2 + z1 * x2,
            w1 * z2 + x1 * y2 - y1 * x2 + z1 * w2,
        ]
    }
}

fn cubic(c: [f32; 4], s: f32) -> f32 {
    c[0] + s * (c[1] + s * (c[2] + s * c[3]))
}

/// One key record with its place in the container.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SiKey {
    /// The whole record inside the container.
    pub span: SourceSpan,
    /// The raw tag word.
    pub tag: u32,
    /// Start of the key's interval, seconds.
    pub t0: f32,
    /// End of the key's interval, seconds.
    pub t1: f32,
    /// Present when [`SI_TAG_POSITION`] is set.
    pub position: Option<SiPositionBlock>,
    /// Present when [`SI_TAG_ORIENTATION`] is set.
    pub orientation: Option<SiOrientationBlock>,
}

/// One script stored inline: its names and keys.
#[derive(Clone, Debug, PartialEq)]
pub struct SiScript<'a> {
    path: &'a [u8],
    object: &'a [u8],
    path_span: SourceSpan,
    object_span: SourceSpan,
    keys: Vec<SiKey>,
}

impl<'a> SiScript<'a> {
    /// The script path as stored, verbatim.
    pub fn path(&self) -> &'a [u8] {
        self.path
    }

    /// The file name without directory or `.zan`, ASCII-lowercased.
    pub fn name(&self) -> String {
        let base = self
            .path
            .rsplit(|b| *b == b'\\' || *b == b'/')
            .next()
            .unwrap_or(self.path);
        let base = base.strip_suffix(b".zan").unwrap_or(base);
        String::from_utf8_lossy(base).to_ascii_lowercase()
    }

    /// The object the script moves, verbatim.
    pub fn object(&self) -> &'a [u8] {
        self.object
    }

    /// Where the path string sits (without its NUL).
    pub const fn path_span(&self) -> SourceSpan {
        self.path_span
    }

    /// Where the object name sits (without its NUL).
    pub const fn object_span(&self) -> SourceSpan {
        self.object_span
    }

    /// The keys in stored order; never empty.
    pub fn keys(&self) -> &[SiKey] {
        &self.keys
    }

    /// The whole script: path, object and every key.
    pub fn span(&self) -> SourceSpan {
        let last = self.keys.last().map_or(self.object_span, |k| k.span);
        SourceSpan {
            offset: self.path_span.offset,
            length: last.offset + last.length - self.path_span.offset,
        }
    }

    /// Seconds from the first key's `t0` to the last key's `t1`.
    pub fn duration(&self) -> f32 {
        self.keys.last().map_or(0.0, |k| k.t1)
    }
}

/// Every script stored inline in `bytes`, in container order.
///
/// A script is accepted only when its path ends in `.zan`, a non-empty
/// printable object name follows and a valid first key (starting at 0.0)
/// follows that. Bytes that look like neither are skipped, not reported.
pub fn read_si_scripts(bytes: &[u8]) -> Vec<SiScript<'_>> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = find(&bytes[from..], SUFFIX) {
        let end = from + rel + SUFFIX.len() - 1; // index of the NUL
        from = end + 1;
        if let Some(script) = script_ending_at(bytes, end) {
            from = script.span().offset as usize + script.span().length as usize;
            out.push(script);
        }
    }
    out
}

/// The script named `name` (file name without `.zan`, case-insensitive),
/// optionally for one `object`.
pub fn find_si_script<'a>(
    bytes: &'a [u8],
    name: &str,
    object: Option<&str>,
) -> Result<SiScript<'a>, SiScriptError> {
    let wanted = name.to_ascii_lowercase();
    let mut hits: Vec<SiScript<'a>> = read_si_scripts(bytes)
        .into_iter()
        .filter(|s| s.name() == wanted)
        .filter(|s| object.is_none_or(|o| s.object.eq_ignore_ascii_case(o.as_bytes())))
        .collect();
    match hits.len() {
        0 => Err(SiScriptError::NotFound { script: wanted }),
        1 => Ok(hits.remove(0)),
        _ => Err(SiScriptError::Ambiguous {
            script: wanted,
            objects: hits
                .iter()
                .map(|s| String::from_utf8_lossy(s.object).into_owned())
                .collect(),
        }),
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn printable(b: u8) -> bool {
    (0x20..0x7f).contains(&b)
}

/// `end` is the index of the NUL that follows `.zan`.
fn script_ending_at(bytes: &[u8], end: usize) -> Option<SiScript<'_>> {
    let mut start = end;
    while start > 0 && end - start < MAX_PATH_BYTES && printable(bytes[start - 1]) {
        start -= 1;
    }
    let run = &bytes[start..end];
    let prefix = run
        .windows(PATH_PREFIX.len())
        .rposition(|w| w == PATH_PREFIX)?;
    let start = start + prefix;
    let path = &bytes[start..end];

    let object_start = end + 1;
    let object_len = bytes
        .get(object_start..)?
        .iter()
        .take(MAX_OBJECT_BYTES + 1)
        .position(|b| *b == 0)?;
    let object = &bytes[object_start..object_start + object_len];
    if object.is_empty() || !object.iter().all(|b| printable(*b)) {
        return None;
    }

    let mut at = object_start + object_len + 1;
    let mut keys: Vec<SiKey> = Vec::new();
    while let Some(key) = read_key(bytes, at, keys.last().map(|k| k.t1)) {
        at += key.span.length as usize;
        keys.push(key);
    }
    if keys.is_empty() {
        return None;
    }
    Some(SiScript {
        path,
        object,
        path_span: SourceSpan {
            offset: start as u64,
            length: path.len() as u64,
        },
        object_span: SourceSpan {
            offset: object_start as u64,
            length: object_len as u64,
        },
        keys,
    })
}

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn float(bytes: &[u8], at: usize) -> Option<f32> {
    word(bytes, at).map(f32::from_bits)
}

fn floats<const N: usize>(bytes: &[u8], at: usize) -> Option<[f32; N]> {
    let mut out = [0.0; N];
    for (i, v) in out.iter_mut().enumerate() {
        *v = float(bytes, at + 4 * i)?;
    }
    Some(out)
}

fn read_key(bytes: &[u8], at: usize, previous_t1: Option<f32>) -> Option<SiKey> {
    let tag = word(bytes, at)?;
    if tag > SI_TAG_POSITION | SI_TAG_ORIENTATION {
        return None;
    }
    let t0 = float(bytes, at + 4)?;
    let t1 = float(bytes, at + 8)?;
    let expected_t0 = previous_t1.unwrap_or(0.0);
    if t0.to_bits() != expected_t0.to_bits()
        || t1.partial_cmp(&t0) != Some(std::cmp::Ordering::Greater)
        || !t1.is_finite()
    {
        return None;
    }
    let mut cursor = at + SI_KEY_HEAD_BYTES;
    let position = if tag & SI_TAG_POSITION != 0 {
        let b: [f32; 19] = floats(bytes, cursor)?;
        cursor += SI_BLOCK_BYTES;
        Some(SiPositionBlock {
            start: [b[0], b[1], b[2]],
            padding_word: b[3].to_bits(),
            mean_rate: [b[4], b[5], b[6]],
            coefficients: [
                [b[7], b[8], b[9], b[10]],
                [b[11], b[12], b[13], b[14]],
                [b[15], b[16], b[17], b[18]],
            ],
        })
    } else {
        None
    };
    let orientation = if tag & SI_TAG_ORIENTATION != 0 {
        let b: [f32; 19] = floats(bytes, cursor)?;
        cursor += SI_BLOCK_BYTES;
        Some(SiOrientationBlock {
            start: [b[0], b[1], b[2], b[3]],
            mean_rate: [b[4], b[5], b[6]],
            coefficients: [
                [b[7], b[8], b[9], b[10]],
                [b[11], b[12], b[13], b[14]],
                [b[15], b[16], b[17], b[18]],
            ],
        })
    } else {
        None
    };
    Some(SiKey {
        span: SourceSpan {
            offset: at as u64,
            length: (cursor - at) as u64,
        },
        tag,
        t0,
        t1,
        position,
        orientation,
    })
}
