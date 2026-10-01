//! Persisted identities for the outcome transaction (F43-A).
//!
//! `docs/contracts/STATE-TRANSACTIONS.md` fixes the outcome key:
//! `OutcomeId = (profile_id, campaign_run_id, session_id,
//! terminal_event_id)` — "or an equally stable persisted identity". These
//! are the newtypes that tuple is built from. Profile ids are persistent
//! identifiers, never display names or recycled indexes (contract,
//! "Persistence"); this module only validates their shape — allocation lives
//! with the profile store (F48).

use std::fmt;

use cs_script::runtime::{EventKey, SessionGeneration};

/// The longest accepted profile/run/difficulty/node-key text.
pub const MAX_IDENTITY_LENGTH: usize = 64;

fn validate_id(text: &str, what: &'static str) -> Result<String, IdentityError> {
    if text.is_empty() {
        return Err(IdentityError::Empty { what });
    }
    if text.len() > MAX_IDENTITY_LENGTH {
        return Err(IdentityError::TooLong {
            what,
            length: text.len(),
        });
    }
    for (position, ch) in text.char_indices() {
        if !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-' | ':')) {
            return Err(IdentityError::InvalidCharacter { what, ch, position });
        }
    }
    Ok(text.to_owned())
}

/// Why an identity string was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityError {
    /// The id was empty.
    Empty {
        /// Which identity refused.
        what: &'static str,
    },
    /// The id exceeded [`MAX_IDENTITY_LENGTH`].
    TooLong {
        /// Which identity refused.
        what: &'static str,
        /// The rejected length.
        length: usize,
    },
    /// The id held a character outside `[A-Za-z0-9_.:-]`.
    InvalidCharacter {
        /// Which identity refused.
        what: &'static str,
        /// The offending character.
        ch: char,
        /// Its byte position.
        position: usize,
    },
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { what } => write!(f, "a {what} cannot be empty"),
            Self::TooLong { what, length } => write!(
                f,
                "a {what} cannot exceed {MAX_IDENTITY_LENGTH} characters ({length} given)"
            ),
            Self::InvalidCharacter { what, ch, position } => write!(
                f,
                "{what} holds {ch:?} at {position}; only [A-Za-z0-9_.:-] is allowed"
            ),
        }
    }
}

impl std::error::Error for IdentityError {}

macro_rules! identity {
    ($name:ident, $doc:literal, $what:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);

        impl $name {
            /// Validates the id's shape.
            ///
            /// # Errors
            ///
            /// [`IdentityError`].
            pub fn new(text: &str) -> Result<Self, IdentityError> {
                validate_id(text, $what).map(Self)
            }

            /// The id text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

identity!(
    ProfileId,
    "A persistent profile identifier — stable across renames, never \
     recycled after deletion (contract, \"Persistence\").",
    "profile id"
);
identity!(
    CampaignRunId,
    "The identity of one playthrough of a campaign definition under a \
     profile: replaying a mission keeps the run; starting a new campaign \
     allocates a new one.",
    "campaign run id"
);
identity!(
    DifficultyId,
    "The difficulty a run was started under — an opaque selector whose \
     *semantics* (which difficulty levels the original offers and what \
     they change) are unmeasured until F43-D.",
    "difficulty id"
);
identity!(
    CampaignNodeKey,
    "The runtime key of a campaign graph node — the lowered twin of the \
     declared `cs_content::campaign::CampaignNodeId`.",
    "campaign node key"
);

/// The stable identity of one outcome transaction — the contract's tuple
/// `(profile_id, campaign_run_id, session_id, terminal_event_id)`.
///
/// A result packet replayed after a crash carries the *same* `OutcomeId`
/// (it is the persisted identity of the terminal event), so the ledger
/// dedups it (spec F43 behavior 2). A genuinely new session's terminal event
/// is a different outcome; whether it may pay again is the graph's declared
/// grant rule, not the ledger's concern.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OutcomeId {
    /// The profile the outcome belongs to.
    pub profile: ProfileId,
    /// The campaign run inside the profile.
    pub run: CampaignRunId,
    /// The session generation the terminal event came from.
    pub session: SessionGeneration,
    /// The terminal event's total-order key — session-qualified, so a
    /// restarted mission can never collide with the attempt it replaced.
    pub terminal_event: EventKey,
}
