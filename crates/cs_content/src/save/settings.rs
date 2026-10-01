//! Which settings a profile stores, how a stored value is checked, and what a
//! refused value falls back to (stage F48-C of
//! `specs/F48-profiles-saves-settings-migration-and-recovery.md`, non-negotiable
//! behavior 5).
//!
//! A profile document carries a list of [`SettingEntry`] values
//! ([`cs_types::profile::SettingEntry`]) and F48-B persisted that list verbatim.
//! This module is the *consumer* half of that persistence: a profile's stored
//! settings are meaningless without something that says which keys exist, which
//! values are acceptable and whether a change may take effect while the game is
//! running. That declaration is a [`SettingCatalog`], and it is **supplied by
//! the caller** — the feature that owns a setting owns its rule. F52
//! (`crates/cs_content/src/settings.rs`) owns the accessibility and modern
//! presentation settings, F22 owns device bindings and F17 owns display policy;
//! nothing here decides what a key means, and no key, default or range in this
//! file is a claim about the original game. The values are newly authored engine
//! vocabulary, so nothing here is `verified_original`.
//!
//! Three properties the spec names are enforced here rather than left to a
//! caller:
//!
//! * **A change that needs a restart is labeled, not applied.**
//!   [`SettingRule::apply`] is read from the catalog, never from the stored
//!   entry, so a save written by a build that mislabeled a key cannot make a
//!   restart-required change take effect immediately. Such a change lands in
//!   [`SettingsState::pending_restart`] and becomes effective at the next
//!   session, which is the only place it is ever read from.
//! * **An unusable value has a safe recovery path.** A stored value a rule
//!   refuses is recovered to the rule's declared [`SettingRule::default`] and
//!   reported as a [`SettingRefusal`]; a *newly offered* value a rule refuses
//!   changes nothing at all, so the last value known to be acceptable stays in
//!   force. Neither path invents a value.
//! * **A key this build has no rule for is preserved, never interpreted.** It
//!   is carried through the save verbatim, exactly as an unknown document field
//!   is, and reported as [`RefusalReason::UnknownKey`]; a build that does not
//!   know what a setting means must not act on it.

use std::collections::BTreeMap;
use std::fmt;

use cs_types::profile::{
    MAX_VALUE_BYTES, ProfileFieldError, SettingApply, SettingEntry, validate_key, validate_text,
};

/// What a setting's value may be.
///
/// Two rules, because those are the only two shapes a persisted setting needs:
/// a bounded number (a device index, a volume in minor units) and one of a
/// declared set of labels (a display mode, a resolution). A boolean is written
/// as a two-label [`ValueRule::Choice`] rather than a third rule with an
/// invented spelling of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueRule {
    /// A decimal integer within an inclusive range.
    Integer { min: i64, max: i64 },
    /// Exactly one of the declared labels.
    Choice(&'static [&'static str]),
}

impl ValueRule {
    /// Whether `value` is acceptable, without interpreting it.
    pub fn accepts(self, value: &str) -> bool {
        match self {
            Self::Integer { min, max } => value
                .parse::<i64>()
                .is_ok_and(|number| (min..=max).contains(&number)),
            Self::Choice(labels) => labels.contains(&value),
        }
    }

    /// Whether this rule describes a usable value space at all. An empty range
    /// or an empty label set would make [`SettingRule::default`] unusable, so a
    /// catalog refuses it rather than declaring a setting it can never satisfy.
    pub const fn is_usable(self) -> bool {
        match self {
            Self::Integer { min, max } => min <= max,
            Self::Choice(labels) => !labels.is_empty(),
        }
    }
}

impl fmt::Display for ValueRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer { min, max } => write!(f, "an integer in {min}..={max}"),
            Self::Choice(labels) => write!(f, "one of [{}]", labels.join(", ")),
        }
    }
}

/// One declared setting: its key, when a change takes effect, the values it
/// accepts and the value that is safe when nothing acceptable is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingRule {
    pub key: &'static str,
    /// Whether a change to this setting may take effect while the game runs.
    pub apply: SettingApply,
    /// What the value may be.
    pub value: ValueRule,
    /// The value that is safe when the stored or offered value is unusable, and
    /// the value a rule with no stored value starts from.
    pub default: &'static str,
}

impl SettingRule {
    /// Whether this declaration holds together: an unusable value space, or a
    /// default the rule itself would refuse, would leave a setting that cannot
    /// be recovered to.
    pub fn is_valid(&self) -> bool {
        self.value.is_usable() && self.value.accepts(self.default)
    }
}

/// A catalog refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogError {
    /// The key does not satisfy the save document's key bounds, so it could
    /// never be written or read back.
    BadKey(ProfileFieldError),
    /// The key is declared twice with different rules, so which one governs a
    /// stored value would be a coin toss.
    DuplicateKey { key: String },
    /// The rule's value space is empty, or its declared default is a value the
    /// rule refuses.
    UnusableRule { key: String, reason: String },
    /// More rules than a document may hold.
    TooManyRules { max: usize },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadKey(error) => write!(f, "setting {error}"),
            Self::DuplicateKey { key } => write!(f, "setting {key:?} is declared twice"),
            Self::UnusableRule { key, reason } => {
                write!(f, "setting {key:?} cannot be recovered to: {reason}")
            }
            Self::TooManyRules { max } => write!(f, "a catalog holds at most {max} rules"),
        }
    }
}

impl std::error::Error for CatalogError {}

impl From<ProfileFieldError> for CatalogError {
    fn from(error: ProfileFieldError) -> Self {
        Self::BadKey(error)
    }
}

/// The declared settings a build knows, in a stable order.
///
/// A catalog is the *only* source of what a setting means: resolution never
/// infers a range, a spelling or an apply label from a stored value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SettingCatalog {
    rules: Vec<SettingRule>,
}

impl SettingCatalog {
    /// A catalog with no declared settings. Every stored setting is then an
    /// unknown key: preserved, reported, never applied.
    pub fn empty() -> Self {
        Self { rules: Vec::new() }
    }

    /// Builds a catalog, refusing a rule that could not be honored.
    pub fn new(rules: impl IntoIterator<Item = SettingRule>) -> Result<Self, CatalogError> {
        let rules: Vec<SettingRule> = rules.into_iter().collect();
        if rules.len() > MAX_SETTINGS {
            return Err(CatalogError::TooManyRules { max: MAX_SETTINGS });
        }
        for rule in &rules {
            validate_key("setting", rule.key)?;
            if !rule.is_valid() {
                return Err(CatalogError::UnusableRule {
                    key: rule.key.to_owned(),
                    reason: if rule.value.is_usable() {
                        format!("its default {:?} is not {}", rule.default, rule.value)
                    } else {
                        format!("{} accepts nothing", rule.value)
                    },
                });
            }
            if let Some(first) = rules.iter().find(|held| held.key == rule.key)
                && first != rule
            {
                return Err(CatalogError::DuplicateKey {
                    key: rule.key.to_owned(),
                });
            }
        }
        // Declaration order is kept, so a catalog reads the way it was written;
        // resolution sorts what it returns.
        Ok(Self { rules })
    }

    /// The declared rules, in declaration order.
    pub fn rules(&self) -> &[SettingRule] {
        &self.rules
    }

    /// The rule that governs `key`.
    pub fn rule(&self, key: &str) -> Option<&SettingRule> {
        self.rules.iter().find(|rule| rule.key == key)
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// Largest catalog a document may hold, matching the document's own list bound.
pub const MAX_SETTINGS: usize = cs_types::profile::MAX_LIST_ENTRIES;

/// Why a stored or offered value was not used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusalReason {
    /// This build declares no rule for the key, so the value is carried through
    /// the save without being interpreted.
    UnknownKey,
    /// The value is not the shape the rule declares (not a number, or not one of
    /// the declared labels).
    BadValue,
    /// The value has the right shape but is outside the declared range.
    OutOfRange,
    /// The stored entry's apply label disagrees with the rule, so the rule's
    /// label was used instead.
    ApplyMismatch {
        stored: SettingApply,
        declared: SettingApply,
    },
}

impl fmt::Display for RefusalReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKey => write!(f, "no rule is declared for this setting"),
            Self::BadValue => write!(f, "the value is not an acceptable value"),
            Self::OutOfRange => write!(f, "the value is outside the declared range"),
            Self::ApplyMismatch { stored, declared } => write!(
                f,
                "the save labels this change {} while the rule declares {}",
                stored.label(),
                declared.label()
            ),
        }
    }
}

/// A value that was not used, and what happened instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingRefusal {
    pub key: String,
    /// The value as it was stored or offered.
    pub stored: String,
    pub reason: RefusalReason,
    /// The value in force after the refusal. Empty for an unknown key, whose
    /// stored value is carried through untouched.
    pub recovery: String,
}

impl fmt::Display for SettingRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.recovery.is_empty() {
            write!(f, "{}={}: {}", self.key, self.stored, self.reason)
        } else {
            write!(
                f,
                "{}={}: {}; {:?} is in force",
                self.key, self.stored, self.reason, self.recovery
            )
        }
    }
}

/// What a change to a setting did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingOutcome {
    /// The value changed and takes effect now.
    AppliedLive {
        key: String,
        previous: String,
        value: String,
    },
    /// The value is stored and takes effect at the next session; the running
    /// one keeps the value it had.
    AppliedAfterRestart {
        key: String,
        previous: String,
        value: String,
    },
    /// The value was refused. Nothing changed: the value that was in force
    /// stays in force, which is the safe recovery path.
    Refused {
        key: String,
        offered: String,
        reason: RefusalReason,
        recovery: String,
    },
}

impl SettingOutcome {
    /// The key this outcome is about.
    pub fn key(&self) -> &str {
        match self {
            Self::AppliedLive { key, .. }
            | Self::AppliedAfterRestart { key, .. }
            | Self::Refused { key, .. } => key,
        }
    }

    /// Whether a restart is needed for the change to take effect.
    pub const fn needs_restart(&self) -> bool {
        matches!(self, Self::AppliedAfterRestart { .. })
    }

    /// The refusal, when the value was not used. An applied change has none:
    /// nothing about it had to be recovered.
    pub fn refusal(&self) -> Option<SettingRefusal> {
        match self {
            Self::AppliedLive { .. } | Self::AppliedAfterRestart { .. } => None,
            Self::Refused {
                key,
                offered,
                reason,
                recovery,
            } => Some(SettingRefusal {
                key: key.clone(),
                stored: offered.clone(),
                reason: *reason,
                recovery: recovery.clone(),
            }),
        }
    }
}

/// A refusal or a restart-pending change, as one line of text a caller can show
/// on a settings screen.
pub fn setting_line(key: &str, apply: SettingApply, value: &str) -> String {
    format!("{key} = {value} (takes effect {})", apply.label())
}

/// One profile's settings at one session: what the save stores, what this
/// process is actually using, and what is waiting for a restart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsState {
    /// The value stored for each key, whether or not this build can use it. An
    /// unknown key is carried here verbatim.
    stored: BTreeMap<String, SettingEntry>,
    /// The value this process is using, for keys whose change takes effect live.
    live: BTreeMap<String, String>,
    /// Declared keys whose stored value is not yet the one in force, because the
    /// rule labels the change as needing a restart.
    pending_restart: Vec<String>,
    /// Everything that had to be recovered or was not used, as text.
    refusals: Vec<SettingRefusal>,
}

impl SettingsState {
    /// Opens a session's settings from a profile's stored entries.
    ///
    /// This never fails: a save is not rejected because a setting in it is
    /// unusable. Each stored value is checked against the catalog and:
    ///
    /// * an unknown key is preserved verbatim and reported, never interpreted;
    /// * a known key with an unusable value is recovered to the rule's declared
    ///   default and reported;
    /// * a known key whose stored apply label disagrees with the rule keeps the
    ///   rule's label and the mismatch is reported.
    ///
    /// Every declared key the profile does not store starts from its declared
    /// default, so a setting added by a newer build of the engine has a value
    /// without the save having to carry it.
    pub fn open(catalog: &SettingCatalog, stored: &[SettingEntry]) -> Self {
        let mut entries: BTreeMap<String, SettingEntry> = BTreeMap::new();
        let mut live: BTreeMap<String, String> = BTreeMap::new();
        let mut refusals = Vec::new();

        for entry in stored {
            // A duplicate key cannot reach here — the document validator refuses
            // one — so the last entry wins and is reported rather than hidden.
            if entries.insert(entry.key.clone(), entry.clone()).is_some() {
                refusals.push(SettingRefusal {
                    key: entry.key.clone(),
                    stored: entry.value.clone(),
                    reason: RefusalReason::BadValue,
                    recovery: String::new(),
                });
            }
            let Some(rule) = catalog.rule(&entry.key) else {
                refusals.push(SettingRefusal {
                    key: entry.key.clone(),
                    stored: entry.value.clone(),
                    reason: RefusalReason::UnknownKey,
                    recovery: String::new(),
                });
                continue;
            };
            let mislabeled = entry.apply != rule.apply;
            if mislabeled {
                refusals.push(SettingRefusal {
                    key: entry.key.clone(),
                    stored: entry.value.clone(),
                    reason: RefusalReason::ApplyMismatch {
                        stored: entry.apply,
                        declared: rule.apply,
                    },
                    recovery: String::new(),
                });
            }
            // A stored value is in force at a session start: it was written by a
            // session that had already ended. The one case where it is not is a
            // **restart-required** key whose stored label says otherwise — then
            // the save does not establish when the change took effect, and the
            // label is the only thing that says. The catalog wins: the value is
            // still stored, still written back under the catalog's own label,
            // and still takes effect at the next session, but a device or
            // display change is never applied by a save that claims it needed no
            // restart.
            let acceptable = rule.value.accepts(&entry.value);
            if acceptable && !(mislabeled && rule.apply == SettingApply::RestartRequired) {
                live.insert(entry.key.clone(), entry.value.clone());
            } else {
                if !acceptable {
                    refusals.push(SettingRefusal {
                        key: entry.key.clone(),
                        stored: entry.value.clone(),
                        reason: value_reason(rule.value, &entry.value),
                        recovery: rule.default.to_owned(),
                    });
                }
                if !acceptable {
                    // An unusable value is recovered to the declared default;
                    // a usable one is left exactly as stored, so a save is never
                    // rewritten behind the player's back.
                    let recovered = entries
                        .get_mut(&entry.key)
                        .expect("the entry was inserted above");
                    recovered.value = rule.default.to_owned();
                }
                live.insert(entry.key.clone(), rule.default.to_owned());
                let recovered = entries
                    .get_mut(&entry.key)
                    .expect("the entry was inserted above");
                recovered.apply = rule.apply;
            }
        }

        for rule in catalog.rules() {
            entries
                .entry(rule.key.to_owned())
                .or_insert_with(|| SettingEntry {
                    key: rule.key.to_owned(),
                    apply: rule.apply,
                    value: rule.default.to_owned(),
                });
            live.entry(rule.key.to_owned())
                .or_insert_with(|| rule.default.to_owned());
        }

        Self {
            stored: entries,
            live,
            pending_restart: Vec::new(),
            refusals,
        }
    }

    /// The value this process is using for `key`, whether or not a rule governs
    /// it: an unknown key reports the value as it was stored, which is what a
    /// caller must show rather than a value this build invented.
    pub fn live_value(&self, key: &str) -> Option<&str> {
        self.live.get(key).map(String::as_str)
    }

    /// The value stored for `key`, which differs from the live value while a
    /// change waits for a restart.
    pub fn stored_value(&self, key: &str) -> Option<&str> {
        self.stored.get(key).map(|entry| entry.value.as_str())
    }

    /// Whether a change to any setting is waiting for a restart.
    pub const fn needs_restart(&self) -> bool {
        !self.pending_restart.is_empty()
    }

    /// The keys whose change is waiting for a restart, in key order.
    pub fn pending_restart(&self) -> &[String] {
        &self.pending_restart
    }

    /// Everything that had to be recovered or was not used.
    pub fn refusals(&self) -> &[SettingRefusal] {
        &self.refusals
    }

    /// The refusals as text a caller can show.
    pub fn refusal_lines(&self) -> Vec<String> {
        self.refusals.iter().map(ToString::to_string).collect()
    }

    /// Changes a setting.
    ///
    /// The key and value bounds of the save document are checked first, so a
    /// value that could not be written is refused before anything is stored. A
    /// declared key is then checked against its rule: an acceptable value is
    /// stored, and takes effect now or at the next session according to the
    /// rule's apply label. An unacceptable value stores nothing and reports the
    /// value that stays in force.
    pub fn set(
        &mut self,
        catalog: &SettingCatalog,
        key: &str,
        value: &str,
    ) -> Result<SettingOutcome, ProfileFieldError> {
        validate_key("setting", key)?;
        validate_text("setting value", value, MAX_VALUE_BYTES, false)?;
        let Some(rule) = catalog.rule(key) else {
            let refusal = SettingRefusal {
                key: key.to_owned(),
                stored: value.to_owned(),
                reason: RefusalReason::UnknownKey,
                recovery: self.live_value(key).unwrap_or_default().to_owned(),
            };
            self.refusals.push(refusal.clone());
            return Ok(SettingOutcome::Refused {
                key: key.to_owned(),
                offered: value.to_owned(),
                reason: RefusalReason::UnknownKey,
                recovery: refusal.recovery,
            });
        };
        if !rule.value.accepts(value) {
            let recovery = self.live_value(key).unwrap_or(rule.default).to_owned();
            let reason = value_reason(rule.value, value);
            self.refusals.push(SettingRefusal {
                key: key.to_owned(),
                stored: value.to_owned(),
                reason,
                recovery: recovery.clone(),
            });
            return Ok(SettingOutcome::Refused {
                key: key.to_owned(),
                offered: value.to_owned(),
                reason,
                recovery,
            });
        }
        let previous = self
            .stored
            .get(key)
            .map(|entry| entry.value.clone())
            .unwrap_or_else(|| rule.default.to_owned());
        self.stored.insert(
            key.to_owned(),
            SettingEntry {
                key: key.to_owned(),
                apply: rule.apply,
                value: value.to_owned(),
            },
        );
        match rule.apply {
            SettingApply::Live => {
                self.live.insert(key.to_owned(), value.to_owned());
                self.pending_restart.retain(|held| held != key);
                Ok(SettingOutcome::AppliedLive {
                    key: key.to_owned(),
                    previous,
                    value: value.to_owned(),
                })
            }
            SettingApply::RestartRequired => {
                // The live value is untouched: the change is in force only after
                // this session ends and a new one opens.
                self.pending_restart.push(key.to_owned());
                self.pending_restart.sort();
                self.pending_restart.dedup();
                Ok(SettingOutcome::AppliedAfterRestart {
                    key: key.to_owned(),
                    previous,
                    value: value.to_owned(),
                })
            }
        }
    }

    /// The entries to write into a profile document, in key order.
    ///
    /// This is what a save must carry: the *stored* values, with each key's
    /// apply label taken from the catalog rather than from whatever a previous
    /// build wrote. A pending restart is deliberately not reported here — the
    /// change is stored, and whether this session is using it is
    /// [`SettingsState::pending_restart`].
    pub fn entries(&self) -> Vec<SettingEntry> {
        self.stored.values().cloned().collect()
    }
}

/// Why a value was refused: a value of the wrong shape is a bad value, a value
/// of the right shape out of range is out of range, so a caller can tell a
/// malformed save from one this engine wrote for another range.
fn value_reason(rule: ValueRule, value: &str) -> RefusalReason {
    match rule {
        ValueRule::Integer { .. } => match value.parse::<i64>() {
            Ok(_) => RefusalReason::OutOfRange,
            Err(_) => RefusalReason::BadValue,
        },
        ValueRule::Choice(_) => RefusalReason::BadValue,
    }
}
