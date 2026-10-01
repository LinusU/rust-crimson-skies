//! Handshake vocabulary: protocol version, compatibility signature and the
//! accept/reject evaluation (F54-A).
//!
//! Spec F54 non-negotiable behavior 2: "Handshake includes protocol, engine
//! rules, installation/canonical-content compatibility signatures and
//! enabled mods. Mismatches fail before launch." A client announces what it
//! is running in a [`ClientHello`]; the host's [`SessionParameters`] hold the
//! canonical signature the session runs under, and [`evaluate_hello`] either
//! clears the offer or rejects it with a [`HandshakeReject`] that says
//! exactly which signature differed — the F54-A minimum acceptance scenario.
//!
//! Nothing here opens a socket: the evaluation is a pure function the
//! transport stage (F54-B) calls before it commits any session state, so a
//! mismatch can never reach launch.

use std::fmt;

use cs_types::content::ContentId;
use cs_types::evidence::ContentHash;
use cs_types::net::{PeerId, SessionId};

use crate::bounds::MAX_MODS;

/// The wire protocol revision this build speaks.
///
/// `PROTOCOL_VERSION` is the single revision this build accepts; there is no
/// negotiated range yet because no second revision exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolVersion(u16);

impl ProtocolVersion {
    /// Wraps a nonzero revision number.
    pub const fn new(value: u16) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    /// The revision number.
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The only protocol revision this build speaks.
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion(1);

/// The signature a peer must match to join a session: which engine rules,
/// which canonical content and which enabled mods it runs.
///
/// `rules_sha256` covers the engine rules build; `content_sha256` covers the
/// installation/canonical-content identity (spec F54 non-negotiable 2);
/// `mods` is the exact enabled-mod set, compared as a set — two peers agree
/// only when neither enables something the other does not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compatibility {
    /// The engine rules signature.
    pub rules_sha256: ContentHash,
    /// The installation/canonical-content signature.
    pub content_sha256: ContentHash,
    /// The enabled mods, each a catalog id. Order-insensitive; duplicates
    /// and lists past [`MAX_MODS`] are invalid.
    pub mods: Vec<ContentId>,
}

/// Why a [`Compatibility`] record was refused before comparison.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompatError {
    /// The mod list exceeded [`MAX_MODS`].
    TooManyMods {
        /// Its length.
        len: usize,
    },
    /// The mod list named the same content id twice.
    DuplicateMod {
        /// The repeated id.
        id: ContentId,
    },
}

impl fmt::Display for CompatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyMods { len } => {
                write!(f, "mod list has {len} entries, max is {MAX_MODS}")
            }
            Self::DuplicateMod { id } => write!(f, "mod list repeats {id}"),
        }
    }
}

impl std::error::Error for CompatError {}

impl Compatibility {
    /// Checks the signature's own bounds: bounded mod list, no duplicates.
    ///
    /// # Errors
    ///
    /// [`CompatError::TooManyMods`] or [`CompatError::DuplicateMod`].
    pub fn validate(&self) -> Result<(), CompatError> {
        if self.mods.len() > MAX_MODS {
            return Err(CompatError::TooManyMods {
                len: self.mods.len(),
            });
        }
        let mut seen = std::collections::BTreeSet::new();
        for id in &self.mods {
            if !seen.insert(id) {
                return Err(CompatError::DuplicateMod { id: id.clone() });
            }
        }
        Ok(())
    }

    /// The mods `other` enables that this signature does not.
    fn missing_mods(&self, other: &Self) -> Vec<ContentId> {
        other
            .mods
            .iter()
            .filter(|id| !self.mods.contains(id))
            .cloned()
            .collect()
    }
}

/// What a connecting client offers: the protocol revision it speaks and the
/// compatibility signature it runs under.
///
/// The hello deliberately carries no callsign, loadout or roster state —
/// identity and lobby membership are F55 vocabulary layered on an already
/// compatible session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientHello {
    /// The protocol revision the client speaks.
    pub protocol: ProtocolVersion,
    /// The signature the client runs under.
    pub compatibility: Compatibility,
}

/// The host's launch requirements: the canonical signature every peer must
/// match. The host's own compatibility *is* the requirement — a client is
/// compatible when it runs the same rules, the same content and the same
/// mods.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionParameters {
    /// The signature the session runs under.
    pub compatibility: Compatibility,
}

/// Why a [`ClientHello`] was rejected. Every variant names the exact
/// mismatch so the client can report it ("rejected with a clear reason",
/// spec F54 AC01) instead of failing as a bare disconnect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HandshakeReject {
    /// The offer itself was out of bounds or malformed.
    MalformedHello(CompatError),
    /// The client speaks a protocol revision this build does not.
    UnsupportedProtocol {
        /// What the client offered.
        offered: ProtocolVersion,
        /// What this build speaks.
        supported: ProtocolVersion,
    },
    /// The engine rules signatures differ.
    RulesMismatch {
        /// The session's rules signature.
        expected: ContentHash,
        /// The client's rules signature.
        offered: ContentHash,
    },
    /// The installation/canonical-content signatures differ.
    ContentMismatch {
        /// The session's content signature.
        expected: ContentHash,
        /// The client's content signature.
        offered: ContentHash,
    },
    /// The enabled-mod sets differ.
    ModSetMismatch {
        /// Mods the session requires that the client does not enable.
        missing: Vec<ContentId>,
        /// Mods the client enables that the session does not run.
        unexpected: Vec<ContentId>,
    },
}

impl fmt::Display for HandshakeReject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedHello(reason) => write!(f, "invalid hello: {reason}"),
            Self::UnsupportedProtocol { offered, supported } => write!(
                f,
                "unsupported protocol version {offered}: this session speaks {supported}"
            ),
            Self::RulesMismatch { expected, offered } => write!(
                f,
                "engine rules mismatch: session {expected}, client {offered}"
            ),
            Self::ContentMismatch { expected, offered } => {
                write!(f, "content mismatch: session {expected}, client {offered}")
            }
            Self::ModSetMismatch {
                missing,
                unexpected,
            } => write!(
                f,
                "mod set mismatch: missing {missing:?}, unexpected {unexpected:?}"
            ),
        }
    }
}

impl std::error::Error for HandshakeReject {}

/// What a session becomes for one accepted client: the session id (the wire
/// epoch every later packet must carry) and the host-allocated peer id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionGrant {
    /// The session the client joined.
    pub session: SessionId,
    /// The client's identity inside it.
    pub peer: PeerId,
}

/// The host's answer to a [`ClientHello`]: either a grant or the reason for
/// refusal. This is the only pre-session server message; both arms are
/// reliable lifecycle traffic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HelloReply {
    /// The client may join: the grant hands it its session epoch and peer id.
    Welcome(SessionGrant),
    /// The client may not join: the reason says exactly why.
    Rejected(HandshakeReject),
}

/// Evaluates a client's hello against the session's launch requirements.
///
/// This is the compatibility gate spec F54 non-negotiable behavior 2
/// requires to run *before* launch: it is pure — it allocates nothing,
/// commits nothing and cannot fail halfway — so a mismatch can never consume
/// session state or reach asset loading. On success the host may allocate a
/// [`SessionGrant`] and answer [`HelloReply::Welcome`]; on failure it
/// answers [`HelloReply::Rejected`] with the reason verbatim.
///
/// # Errors
///
/// The first mismatch found, in handshake order: a malformed offer
/// ([`HandshakeReject::MalformedHello`]), then protocol
/// ([`HandshakeReject::UnsupportedProtocol`]), rules
/// ([`HandshakeReject::RulesMismatch`]), content
/// ([`HandshakeReject::ContentMismatch`]) and mod set
/// ([`HandshakeReject::ModSetMismatch`]).
pub fn evaluate_hello(
    params: &SessionParameters,
    hello: &ClientHello,
) -> Result<(), HandshakeReject> {
    hello
        .compatibility
        .validate()
        .map_err(HandshakeReject::MalformedHello)?;
    params
        .compatibility
        .validate()
        .map_err(HandshakeReject::MalformedHello)?;
    if hello.protocol != PROTOCOL_VERSION {
        return Err(HandshakeReject::UnsupportedProtocol {
            offered: hello.protocol,
            supported: PROTOCOL_VERSION,
        });
    }
    if hello.compatibility.rules_sha256 != params.compatibility.rules_sha256 {
        return Err(HandshakeReject::RulesMismatch {
            expected: params.compatibility.rules_sha256,
            offered: hello.compatibility.rules_sha256,
        });
    }
    if hello.compatibility.content_sha256 != params.compatibility.content_sha256 {
        return Err(HandshakeReject::ContentMismatch {
            expected: params.compatibility.content_sha256,
            offered: hello.compatibility.content_sha256,
        });
    }
    let missing = hello.compatibility.missing_mods(&params.compatibility);
    let unexpected = params.compatibility.missing_mods(&hello.compatibility);
    if !missing.is_empty() || !unexpected.is_empty() {
        return Err(HandshakeReject::ModSetMismatch {
            missing,
            unexpected,
        });
    }
    Ok(())
}
