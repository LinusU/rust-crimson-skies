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
//! mismatch can never reach launch. [`admit_hello`] wraps that gate with the
//! host-side [`PeerAllocator`] so the decision the host actually sends —
//! [`HelloReply`] — is defined, bounded and tested at this stage too.

use std::fmt;

use cs_types::content::ContentId;
use cs_types::evidence::ContentHash;
use cs_types::net::{PeerId, SessionId};

use crate::bounds::{MAX_MODS, MAX_SESSION_PEERS};

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
    /// A compatibility record was out of bounds or malformed — the client's
    /// offer *or* the host's own [`SessionParameters`]. The reason names the
    /// defect without blaming a side that may be innocent.
    MalformedSignature(CompatError),
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
    /// The session already holds [`MAX_SESSION_PEERS`] peers, so no peer id is
    /// left to admit this client with.
    SessionFull {
        /// The session's peer cap.
        max: usize,
    },
}

impl fmt::Display for HandshakeReject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedSignature(reason) => {
                write!(f, "malformed compatibility signature: {reason}")
            }
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
            Self::SessionFull { max } => {
                write!(f, "session already holds its maximum of {max} peers")
            }
        }
    }
}

impl std::error::Error for HandshakeReject {}

/// What a session becomes for one accepted client: the session id (the wire
/// epoch every later packet must carry) and the host-allocated peer id.
///
/// Produced by [`admit_hello`]; the client learns nothing else about its
/// membership from the handshake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionGrant {
    /// The session the client joined.
    pub session: SessionId,
    /// The client's identity inside it.
    pub peer: PeerId,
}

/// The host's answer to a [`ClientHello`]: either a grant or the reason for
/// refusal. This is the only pre-session server message; both arms are
/// reliable lifecycle traffic. Built by [`admit_hello`].
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
/// The first mismatch found, in handshake order: a malformed compatibility
/// record ([`HandshakeReject::MalformedSignature`]), then protocol
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
        .map_err(HandshakeReject::MalformedSignature)?;
    params
        .compatibility
        .validate()
        .map_err(HandshakeReject::MalformedSignature)?;
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
    // `missing_mods` answers "what does `other` enable that this signature
    // does not", so the client's copy names what the *session* requires that
    // the client lacks, and the session's copy names the reverse.
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

/// Why a [`PeerId`] could not be allocated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerAllocError {
    /// The session's peer population is already at its cap
    /// ([`MAX_SESSION_PEERS`]), or the nonzero `u16` peer space ran out.
    Full,
}

impl fmt::Display for PeerAllocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full => write!(
                f,
                "session already holds its maximum of {MAX_SESSION_PEERS} peers"
            ),
        }
    }
}

impl std::error::Error for PeerAllocError {}

/// Server-side allocation of [`PeerId`]s for one session.
///
/// The host owns membership (`docs/contracts/UI-NETWORK.md`, ownership table),
/// so a client never names its own peer id. Peer numbers start at 1 and are
/// **not** recycled within a session — a departed peer's id must never alias a
/// later arrival, or a late packet from the old peer would be attributed to
/// the new one. The cap is [`MAX_SESSION_PEERS`]; a full session refuses
/// further allocation rather than minting an id outside the declared bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerAllocator {
    next_peer: u16,
}

impl PeerAllocator {
    /// Starts allocation at peer 1, so peer 0 is never a live peer.
    pub const fn new() -> Self {
        Self { next_peer: 1 }
    }

    /// The peer number the next allocation would issue.
    pub const fn next_peer(&self) -> u16 {
        self.next_peer
    }

    /// Allocates the next peer id for this session.
    ///
    /// # Errors
    ///
    /// [`PeerAllocError::Full`] once the session holds
    /// [`MAX_SESSION_PEERS`] peers. The allocator then stays full: it never
    /// wraps onto an id it already issued.
    pub const fn allocate(&mut self) -> Result<PeerId, PeerAllocError> {
        let Some(peer) = PeerId::new(self.next_peer) else {
            return Err(PeerAllocError::Full);
        };
        if peer.get() as usize > MAX_SESSION_PEERS {
            return Err(PeerAllocError::Full);
        }
        match self.next_peer.checked_add(1) {
            Some(next) => {
                self.next_peer = next;
                Ok(peer)
            }
            None => Err(PeerAllocError::Full),
        }
    }
}

impl Default for PeerAllocator {
    fn default() -> Self {
        Self::new()
    }
}

/// The host's decision about one hello: admit it with a grant, or reject it
/// with the reason to report.
///
/// `session` is the epoch the host already allocated for this session; the
/// host-side source of session generations is runtime work (F54-B/C). The
/// ordering is the point of this function: [`evaluate_hello`] runs first and is
/// pure, so a mismatch never consumes a peer id — no session state is spent on
/// a client that is about to be turned away. Only a compatible offer reaches
/// [`PeerAllocator::allocate`], and a full session is itself a rejection with a
/// clear reason ([`HandshakeReject::SessionFull`]).
pub fn admit_hello(
    session: SessionId,
    params: &SessionParameters,
    hello: &ClientHello,
    peers: &mut PeerAllocator,
) -> HelloReply {
    match evaluate_hello(params, hello) {
        Ok(()) => match peers.allocate() {
            Ok(peer) => HelloReply::Welcome(SessionGrant { session, peer }),
            Err(PeerAllocError::Full) => HelloReply::Rejected(HandshakeReject::SessionFull {
                max: MAX_SESSION_PEERS,
            }),
        },
        Err(reason) => HelloReply::Rejected(reason),
    }
}
