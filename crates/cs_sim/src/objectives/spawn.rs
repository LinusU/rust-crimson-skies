//! Idempotent spawn-group and dialogue-cue emission.

use std::collections::BTreeMap;

use cs_script::ir::ActorId;
use cs_script::runtime::SessionGeneration;

/// The idempotency key of one cue or spawn: stable per authored emission.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IdempotencyKey(pub String);

/// What was admitted for a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Emission {
    /// A spawn group created these instances.
    Spawn(Vec<ActorId>),
    /// A dialogue cue was played.
    Cue,
}

/// The ledger's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Admission {
    /// First time this key is seen this session: do it.
    Admitted,
    /// Already done this session: do nothing. Carries the original emission.
    Repeated(Emission),
}

/// A refused call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerError {
    /// The caller's session is not the ledger's: a stale cue from before a
    /// retry.
    StaleSession {
        ledger: SessionGeneration,
        given: SessionGeneration,
    },
}

/// Per-session ledger of keys already emitted. A retry makes a new ledger, so
/// nothing from an old session survives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmissionLedger {
    session: SessionGeneration,
    done: BTreeMap<IdempotencyKey, Emission>,
}

impl EmissionLedger {
    #[must_use]
    pub fn new(session: SessionGeneration) -> Self {
        Self {
            session,
            done: BTreeMap::new(),
        }
    }

    #[must_use]
    pub const fn session(&self) -> SessionGeneration {
        self.session
    }

    /// Admits `key` once, recording `emission`.
    ///
    /// # Errors
    ///
    /// [`LedgerError::StaleSession`] when `session` is not this ledger's.
    pub fn admit(
        &mut self,
        session: SessionGeneration,
        key: IdempotencyKey,
        emission: Emission,
    ) -> Result<Admission, LedgerError> {
        if session != self.session {
            return Err(LedgerError::StaleSession {
                ledger: self.session,
                given: session,
            });
        }
        if let Some(prior) = self.done.get(&key) {
            return Ok(Admission::Repeated(prior.clone()));
        }
        self.done.insert(key, emission);
        Ok(Admission::Admitted)
    }

    /// What a key was admitted with, whenever it was admitted this session.
    ///
    /// The read side of exactly-once: a caller asks what a key already produced
    /// instead of calling [`admit`](Self::admit) to find out, so "was this wave
    /// spawned?" and "spawn it" cannot disagree.
    #[must_use]
    pub fn admitted(&self, key: &IdempotencyKey) -> Option<&Emission> {
        self.done.get(key)
    }

    /// How many keys have been admitted.
    #[must_use]
    pub fn len(&self) -> usize {
        self.done.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.done.is_empty()
    }
}
