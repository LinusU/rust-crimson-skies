//! The mission's terminal outcome, and the **declared** precedence that
//! resolves two terminal requests made on one tick.
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`
//! (F39 owns trigger semantics), stage `### F39-B`, acceptance case AC02:
//! *"Destroy a protected actor on the same tick as completing an objective;
//! use declared terminal precedence."* Shared contract:
//! `docs/contracts/SCRIPT-MISSION.md`, "Objective event ordering":
//!
//! > Terminal success/failure precedence is a compatibility rule that must be
//! > measured for conflicting events. A designed conservative policy can be
//! > used for synthetic tests only until verified.
//! >
//! > A protected actor destroyed after a success latch may or may not change
//! > the outcome; measure rather than assume.
//!
//! # What is decided here and what is still unknown
//!
//! Two rules are made structural rather than left to a caller:
//!
//! * **Distinct endings.** [`TerminalOutcome`] keeps success, failure and
//!   extraction apart. They are not a boolean, and an *optional reward* is not
//!   any of them: a reward is an intent the host applies (F39 non-negotiable
//!   behavior 5), so it has no place in this enum.
//! * **A latch, not a vote.** [`TerminalLatch`] answers exactly one question
//!   — has the mission's outcome been decided, when, and which one — and never
//!   changes its answer. A later request is refused and *named*, so a consumer
//!   can see the request that lost rather than infer it.
//!
//! What is **not** decided here is which outcome the original game prefers when
//! two requests collide. [`TerminalPrecedence::SyntheticConservative`] is a
//! designed policy: it breaks a tie towards the outcome that withholds a
//! success the player may not have earned, and it is labelled
//! `synthetic` precisely so a measured original rule replaces it as a named
//! variant instead of editing this one. No claim in this module is an
//! original-fidelity claim.

use std::collections::BTreeSet;
use std::fmt;

use cs_types::Tick;

/// How a mission ends, as the objective layer sees it.
///
/// The three are **distinct endings** (F39 non-negotiable behavior 5), not a
/// success flag with a cause:
///
/// * `Success` — the declared objective was met;
/// * `Extraction` — the mission ended by leaving the theatre with what it was
///   for, which is a different ending the player sees differently and which
///   must never be reported as a plain success;
/// * `Failure` — the declared failure condition was met.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TerminalOutcome {
    /// The declared success.
    Success,
    /// The mission ended by extraction rather than by completing in place.
    Extraction,
    /// The declared failure.
    Failure,
}

impl TerminalOutcome {
    /// Stable label for diagnostics, evidence records and event text.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Extraction => "extraction",
            Self::Failure => "failure",
        }
    }
}

impl fmt::Display for TerminalOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Which outcome wins when one tick requests more than one.
///
/// The declaration order of [`TerminalOutcome`]'s variants is deliberately *not*
/// the precedence: `Success` sorts first (so a set of requests is stable and
/// readable) while [`TerminalPrecedence::rank`] is the separate, explicit
/// statement of what wins. Sorting and precedence are different questions and
/// the code answers them in different places.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TerminalPrecedence {
    /// The designed conservative policy: `Failure` beats `Extraction` beats
    /// `Success`.
    ///
    /// It resolves a same-tick collision towards the outcome that withholds a
    /// success, because on the same tick the player has not been shown either
    /// reading and the design question is which mistake is worse. **The
    /// original game's precedence is unmeasured.** This variant is the
    /// synthetic policy the contract permits for tests only; a measured rule
    /// becomes a new named variant so every existing caller keeps its meaning.
    #[default]
    SyntheticConservative,
}

impl TerminalPrecedence {
    /// The precedence position of `outcome`; lower wins.
    #[must_use]
    pub const fn rank(self, outcome: TerminalOutcome) -> u8 {
        match self {
            Self::SyntheticConservative => match outcome {
                TerminalOutcome::Failure => 0,
                TerminalOutcome::Extraction => 1,
                TerminalOutcome::Success => 2,
            },
        }
    }

    /// The winning outcome of a set of requests, or `None` when nothing was
    /// requested.
    ///
    /// Total and order-independent: the winner depends on the *set* of
    /// requests, not on the order they arrived in, so two producers that
    /// request in opposite orders on the same tick agree.
    #[must_use]
    pub fn resolve(self, requested: &BTreeSet<TerminalOutcome>) -> Option<TerminalOutcome> {
        requested
            .iter()
            .copied()
            .min_by_key(|outcome| self.rank(*outcome))
    }

    /// Stable label for diagnostics and evidence records.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::SyntheticConservative => "synthetic_conservative",
        }
    }
}

impl fmt::Display for TerminalPrecedence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The mission's one settled outcome.
///
/// The latch is the whole of AC02's "declared terminal precedence": a request
/// is only ever *resolved* at the end of the tick that made it, and once the
/// latch holds an outcome the mission's objective work is over. A request that
/// arrives after that is refused by name ([`Resolution::AlreadySettled`])
/// instead of quietly changing a result the player has already been shown —
/// which is the contract's "may or may not change the outcome" turned into a
/// stated rule instead of an assumption: *on this engine it cannot*.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalLatch {
    settled: Option<(Tick, TerminalOutcome)>,
}

impl TerminalLatch {
    /// A latch with nothing settled.
    #[must_use]
    pub const fn new() -> Self {
        Self { settled: None }
    }

    /// The settled outcome and the tick it settled on.
    #[must_use]
    pub const fn settled(&self) -> Option<(Tick, TerminalOutcome)> {
        self.settled
    }

    /// The settled outcome alone.
    #[must_use]
    pub const fn outcome(&self) -> Option<TerminalOutcome> {
        match self.settled {
            Some((_, outcome)) => Some(outcome),
            None => None,
        }
    }

    /// Whether the mission's outcome is already decided.
    #[must_use]
    pub const fn is_settled(&self) -> bool {
        self.settled.is_some()
    }

    /// Settles the latch from every request made on `tick`.
    ///
    /// One decision per tick: the whole set is resolved by `precedence` at
    /// once, so a success request and a failure request on the same tick cannot
    /// be applied in whichever order they happened to arrive.
    pub fn resolve(
        &mut self,
        tick: Tick,
        requested: &BTreeSet<TerminalOutcome>,
        precedence: TerminalPrecedence,
    ) -> Resolution {
        if let Some((settled_at, outcome)) = self.settled {
            return Resolution::AlreadySettled {
                settled_at,
                outcome,
            };
        }
        let Some(winner) = precedence.resolve(requested) else {
            return Resolution::NoneRequested;
        };
        self.settled = Some((tick, winner));
        Resolution::Settled {
            outcome: winner,
            superseded: requested.iter().copied().filter(|o| *o != winner).collect(),
        }
    }
}

/// What one [`TerminalLatch::resolve`] decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// No terminal request was made this tick; the latch is untouched.
    NoneRequested,
    /// The latch settled on `outcome`, and these same-tick requests lost.
    Settled {
        outcome: TerminalOutcome,
        /// The requests that were made on the same tick and did not win, in
        /// [`TerminalOutcome`] order.
        superseded: BTreeSet<TerminalOutcome>,
    },
    /// The outcome was already settled; nothing changed and the request is
    /// refused.
    AlreadySettled {
        settled_at: Tick,
        outcome: TerminalOutcome,
    },
}

impl Resolution {
    /// Whether this resolution refused every request.
    #[must_use]
    pub const fn is_refused(&self) -> bool {
        matches!(self, Self::AlreadySettled { .. })
    }
}
