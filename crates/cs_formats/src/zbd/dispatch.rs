//! Two-key ZBD dispatch: a validated header **and** an installation role.
//!
//! Spec F06's deliverable: "Dispatch by validated header/version and
//! installation role into distinct sound, reader, texture, interp, GameZ and
//! animation readers." [`dispatch`] is that decision: it takes a
//! [`ZbdProbe`] (where the container lives, plus its first bytes) and
//! returns the [`ZbdDispatch`] the family's reader consumes — or a
//! [`ZbdDispatchError`] that names exactly which key disagreed.
//!
//! The rules the decision combines are evidence-labelled data from
//! [`crate::zbd::family`] and [`crate::zbd::role`], so nothing here decides a
//! family from the file extension alone, and nothing silently falls back to
//! another parser: when the observed role names a family whose header is
//! documented, the bytes must validate against it; when the bytes claim a
//! different documented family than the role, dispatch fails with
//! [`ZbdDispatchError::HeaderRoleConflict`] (spec F06 AC02); when neither key
//! says anything, dispatch fails with [`ZbdDispatchError::UnknownFamily`].

use std::fmt;

use cs_types::install::{FileFamily, RelativePath};

use super::family::{ZBD_FAMILY_INVENTORY, ZbdFamily, ZbdFamilyRecord, ZbdReaderId, family_record};
use super::header::{HeaderProbe, HeaderRule, SignatureRule};
use super::role::{RoleRule, ZbdRole, role_for_path};

/// Everything dispatch decides from: where the container came from, where it
/// lives in the installation and its first bytes.
///
/// The header is a *probe*, not the file: callers hand the documented
/// prefix (the rules in [`crate::zbd::header`] need at most
/// [`SignatureRule::required_bytes`] bytes) or the whole member when they
/// already hold it. Dispatch borrows it and never copies probe bytes into a
/// result or an error (F03: errors carry metadata only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZbdProbe<'a> {
    container: &'a str,
    path: &'a RelativePath,
    header: &'a [u8],
}

impl<'a> ZbdProbe<'a> {
    /// Records the three inputs of a dispatch decision.
    ///
    /// `container` is the provenance label errors are stamped with (an
    /// installation-relative path, a VFS key, a mount id — opaque to this
    /// crate, never joined to a filesystem path).
    pub const fn new(container: &'a str, path: &'a RelativePath, header: &'a [u8]) -> Self {
        Self {
            container,
            path,
            header,
        }
    }

    /// Provenance label carried by this probe's errors.
    pub const fn container(&self) -> &'a str {
        self.container
    }

    /// The installation-relative path the role rules are matched against.
    pub const fn path(&self) -> &'a RelativePath {
        self.path
    }

    /// The probe bytes (the container's header prefix).
    pub const fn header_bytes(&self) -> &'a [u8] {
        self.header
    }
}

/// Which key identified the family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchBasis {
    /// The observed role and a documented header signature agree.
    HeaderAndRole,
    /// Only the documented header signature identified the family; the path
    /// matched no observed role rule.
    HeaderOnly,
    /// Only the observed role identified the family. The bytes were checked
    /// against every documented signature (a contradicting match would have
    /// failed) but cannot be validated for this family, because no header
    /// layout for it is documented.
    RoleOnly,
}

impl DispatchBasis {
    /// Stable label for reports and diagnostics.
    pub const fn label(self) -> &'static str {
        match self {
            Self::HeaderAndRole => "header+role",
            Self::HeaderOnly => "header",
            Self::RoleOnly => "role",
        }
    }
}

/// What dispatch could establish about the probe's bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderStatus {
    /// A documented signature rule matched the probe: these are the
    /// observed signature and version words that validated.
    Validated {
        /// The documented signature word that matched.
        signature: u32,
        /// The observed (and documented) version word.
        version: u32,
    },
    /// The bytes were **not** validated: no header layout for this family is
    /// documented, so dispatch can only record what it does not know. The
    /// reason is the family's own [`HeaderRule::Undocumented`] reason.
    Unvalidated {
        /// Why this family's bytes cannot be validated (spec F06 research
        /// boundary; unknown stays unknown).
        reason: &'static str,
    },
}

/// What the observed installation role contributed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoleStatus {
    /// An observed archive-name rule matched the path.
    Observed {
        /// The rule that matched, with its evidence class and citation.
        rule: RoleRule,
    },
    /// The path matched no observed rule.
    Unrecognized {
        /// Why (`crate::zbd::role::OUTSIDE_CONTENT_ROOT` /
        /// `crate::zbd::role::UNOBSERVED_NAME`).
        reason: &'static str,
    },
}

/// The typed output of [`dispatch`] and the typed input a ZBD reader is
/// handed: the container, the path, the probe bytes, the decided family and
/// reader slot, and the evidence for each key.
///
/// Built only by [`dispatch`], so every value here is an internally
/// consistent decision; F06-B's readers take it and report the ranges they
/// consume and the records they could not support (spec F06 deliverable).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZbdDispatch<'a> {
    container: &'a str,
    path: &'a RelativePath,
    header: &'a [u8],
    family: ZbdFamily,
    reader: ZbdReaderId,
    basis: DispatchBasis,
    header_status: HeaderStatus,
    role_status: RoleStatus,
}

impl<'a> ZbdDispatch<'a> {
    /// Provenance label of the dispatched container.
    pub const fn container(&self) -> &'a str {
        self.container
    }

    /// The installation-relative path this container was dispatched from.
    pub const fn path(&self) -> &'a RelativePath {
        self.path
    }

    /// The probe bytes the decision looked at.
    pub const fn header_bytes(&self) -> &'a [u8] {
        self.header
    }

    /// The family the two keys agreed on (or one key alone identified).
    pub const fn family(&self) -> ZbdFamily {
        self.family
    }

    /// The reader slot this container is routed to.
    pub const fn reader(&self) -> ZbdReaderId {
        self.reader
    }

    /// Which key identified the family.
    pub const fn basis(&self) -> DispatchBasis {
        self.basis
    }

    /// What the probe's bytes could establish.
    pub const fn header_status(&self) -> HeaderStatus {
        self.header_status
    }

    /// What the observed installation role contributed.
    pub const fn role_status(&self) -> RoleStatus {
        self.role_status
    }

    /// The validated F02 family label (`zbd.interp`, `zbd.gamez`, …) the
    /// inventory row for this dispatch carries.
    pub fn file_family(&self) -> FileFamily {
        self.family.file_family()
    }

    /// The inventory row this dispatch routes through.
    pub fn record(&self) -> &'static ZbdFamilyRecord {
        family_record(self.family)
    }
}

/// Why a probe could not be dispatched.
///
/// Every variant names the container label it came from and the
/// decision-relevant values — never probe bytes (F03: errors carry metadata
/// only, so a diagnostic cannot echo private installation data). Match on
/// [`Self::code`] instead of the [`fmt::Display`] text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZbdDispatchError {
    /// The observed role names a family whose header **is** documented, and
    /// the probe is too short to evaluate that rule: dispatch cannot claim
    /// it validated bytes it could not read.
    HeaderTooShort {
        /// Provenance label of the container.
        container: String,
        /// The family the observed role named.
        family: ZbdFamily,
        /// Bytes the documented rule needs (`SignatureRule::required_bytes`).
        needed: usize,
        /// Bytes the probe actually has.
        available: usize,
        /// The role rule that named the family.
        role_rule: RoleRule,
    },
    /// The observed role names a family whose header is documented, and the
    /// probe does not carry that family's signature. Dispatch refuses the
    /// container instead of routing it to a parser that does not fit
    /// (spec F06 AC02: never fall back to another parser silently).
    HeaderMismatch {
        /// Provenance label of the container.
        container: String,
        /// The family the observed role named.
        family: ZbdFamily,
        /// The signature word the family's rule requires.
        expected_signature: u32,
        /// Bytes the probe actually has.
        available: usize,
        /// The role rule that named the family.
        role_rule: RoleRule,
    },
    /// A documented signature matched but the version word is not the
    /// documented one: the family is recognized, this version is not.
    UnsupportedHeaderVersion {
        /// Provenance label of the container.
        container: String,
        /// The family the signature belongs to.
        family: ZbdFamily,
        /// The observed version word.
        observed: u32,
        /// The documented version word.
        supported: u32,
        /// Where the version was documented.
        source: &'static str,
    },
    /// The probe carries one family's documented signature while the
    /// observed installation role names another. A valid header with
    /// incompatible family data fails explicitly (spec F06 AC02); a family
    /// disagreement outranks a version disagreement.
    HeaderRoleConflict {
        /// Provenance label of the container.
        container: String,
        /// The family the header signature belongs to.
        header_family: ZbdFamily,
        /// The family the observed role named.
        role_family: ZbdFamily,
        /// The contradicting signature word.
        signature: u32,
        /// The role rule that named the role family.
        role_rule: RoleRule,
    },
    /// No documented signature matched and no observed role rule matched:
    /// dispatch has nothing to route this container by, so it routes nothing
    /// (unknown means unknown).
    UnknownFamily {
        /// Provenance label of the container.
        container: String,
        /// Why the role matched nothing (`crate::zbd::role::OUTSIDE_CONTENT_ROOT`
        /// / `crate::zbd::role::UNOBSERVED_NAME`).
        reason: &'static str,
    },
}

impl ZbdDispatchError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::HeaderTooShort { .. } => "header_too_short",
            Self::HeaderMismatch { .. } => "header_mismatch",
            Self::UnsupportedHeaderVersion { .. } => "unsupported_header_version",
            Self::HeaderRoleConflict { .. } => "header_role_conflict",
            Self::UnknownFamily { .. } => "unknown_family",
        }
    }

    /// The container label the probe was dispatched with.
    pub fn container(&self) -> &str {
        match self {
            Self::HeaderTooShort { container, .. }
            | Self::HeaderMismatch { container, .. }
            | Self::UnsupportedHeaderVersion { container, .. }
            | Self::HeaderRoleConflict { container, .. }
            | Self::UnknownFamily { container, .. } => container,
        }
    }
}

impl fmt::Display for ZbdDispatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HeaderTooShort {
                container,
                family,
                needed,
                available,
                ..
            } => write!(
                f,
                "{container}: the observed role names the `{}` family, whose documented header \
                 needs {needed} bytes, but the probe has {available}",
                family.as_str()
            ),
            Self::HeaderMismatch {
                container,
                family,
                expected_signature,
                available,
                role_rule,
            } => write!(
                f,
                "{container}: the observed role rule ({}) names the `{}` family, but the probe \
                 does not carry its documented signature 0x{expected_signature:08X} in its \
                 {available} bytes",
                role_rule.pattern(),
                family.as_str()
            ),
            Self::UnsupportedHeaderVersion {
                container,
                family,
                observed,
                supported,
                source,
            } => write!(
                f,
                "{container}: the `{}` family signature matched with version {observed}, \
                 documented version is {supported} ({source})",
                family.as_str()
            ),
            Self::HeaderRoleConflict {
                container,
                header_family,
                role_family,
                signature,
                role_rule,
            } => write!(
                f,
                "{container}: header signature 0x{signature:08X} belongs to the `{}` family but \
                 the observed role rule ({}) names the `{}` family",
                header_family.as_str(),
                role_rule.pattern(),
                role_family.as_str()
            ),
            Self::UnknownFamily { container, reason } => {
                write!(
                    f,
                    "{container}: no documented header signature matched, no observed role rule matched ({reason})"
                )
            }
        }
    }
}

impl std::error::Error for ZbdDispatchError {}

/// Dispatches one probe to the reader of the family its two keys identify.
///
/// The decision, in order:
///
/// 1. **The observed role names a family.** If that family's header rule is
///    documented, the probe must validate: [`HeaderProbe::TooShort`] becomes
///    [`ZbdDispatchError::HeaderTooShort`], [`HeaderProbe::Mismatch`] becomes
///    [`ZbdDispatchError::HeaderMismatch`], a wrong version becomes
///    [`ZbdDispatchError::UnsupportedHeaderVersion`], and a match dispatches
///    on [`DispatchBasis::HeaderAndRole`]. If the family's header rule is
///    undocumented, the probe is still checked against *every* documented
///    signature — a contradicting match is
///    [`ZbdDispatchError::HeaderRoleConflict`] — and the dispatch records
///    [`HeaderStatus::Unvalidated`] on [`DispatchBasis::RoleOnly`].
/// 2. **The role is unrecognized.** The documented signatures decide alone:
///    a match dispatches on [`DispatchBasis::HeaderOnly`] (after the version
///    check), no match fails with [`ZbdDispatchError::UnknownFamily`].
///
/// # Errors
///
/// All five [`ZbdDispatchError`] variants; each carries the probe's
/// container label and the values the decision disagreed on.
pub fn dispatch<'a>(probe: ZbdProbe<'a>) -> Result<ZbdDispatch<'a>, ZbdDispatchError> {
    match role_for_path(probe.path) {
        ZbdRole::Observed { family, rule } => dispatch_with_role(probe, family, rule),
        ZbdRole::Unrecognized { reason } => dispatch_by_header_only(probe, reason),
    }
}

/// The role-known half of [`dispatch`]: see the entrypoint's step 1.
fn dispatch_with_role<'a>(
    probe: ZbdProbe<'a>,
    family: ZbdFamily,
    rule: RoleRule,
) -> Result<ZbdDispatch<'a>, ZbdDispatchError> {
    let record = family_record(family);
    match record.header_rule() {
        HeaderRule::Undocumented { reason } => {
            // The family's bytes cannot be validated — but another family's
            // documented signature in them must still fail loudly.
            if let Some((header_family, contradicting, _version)) =
                documented_header_match(probe.header)
            {
                debug_assert_ne!(
                    header_family, family,
                    "a family without a signature rule cannot also produce one"
                );
                return Err(ZbdDispatchError::HeaderRoleConflict {
                    container: probe.container.to_owned(),
                    header_family,
                    role_family: family,
                    signature: contradicting.value(),
                    role_rule: rule,
                });
            }
            Ok(ZbdDispatch {
                container: probe.container,
                path: probe.path,
                header: probe.header,
                family,
                reader: family.reader(),
                basis: DispatchBasis::RoleOnly,
                header_status: HeaderStatus::Unvalidated { reason },
                role_status: RoleStatus::Observed { rule },
            })
        }
        HeaderRule::Signature(signature) => match signature.probe(probe.header) {
            HeaderProbe::TooShort { needed, available } => Err(ZbdDispatchError::HeaderTooShort {
                container: probe.container.to_owned(),
                family,
                needed,
                available,
                role_rule: rule,
            }),
            HeaderProbe::Mismatch => Err(ZbdDispatchError::HeaderMismatch {
                container: probe.container.to_owned(),
                family,
                expected_signature: signature.value(),
                available: probe.header.len(),
                role_rule: rule,
            }),
            HeaderProbe::Match { version } => {
                if version != signature.version() {
                    return Err(ZbdDispatchError::UnsupportedHeaderVersion {
                        container: probe.container.to_owned(),
                        family,
                        observed: version,
                        supported: signature.version(),
                        source: signature.source(),
                    });
                }
                Ok(ZbdDispatch {
                    container: probe.container,
                    path: probe.path,
                    header: probe.header,
                    family,
                    reader: family.reader(),
                    basis: DispatchBasis::HeaderAndRole,
                    header_status: HeaderStatus::Validated {
                        signature: signature.value(),
                        version,
                    },
                    role_status: RoleStatus::Observed { rule },
                })
            }
        },
    }
}

/// The role-unknown half of [`dispatch`]: see the entrypoint's step 2.
fn dispatch_by_header_only<'a>(
    probe: ZbdProbe<'a>,
    role_reason: &'static str,
) -> Result<ZbdDispatch<'a>, ZbdDispatchError> {
    let Some((family, signature, version)) = documented_header_match(probe.header) else {
        return Err(ZbdDispatchError::UnknownFamily {
            container: probe.container.to_owned(),
            reason: role_reason,
        });
    };
    if version != signature.version() {
        return Err(ZbdDispatchError::UnsupportedHeaderVersion {
            container: probe.container.to_owned(),
            family,
            observed: version,
            supported: signature.version(),
            source: signature.source(),
        });
    }
    Ok(ZbdDispatch {
        container: probe.container,
        path: probe.path,
        header: probe.header,
        family,
        reader: family.reader(),
        basis: DispatchBasis::HeaderOnly,
        header_status: HeaderStatus::Validated {
            signature: signature.value(),
            version,
        },
        role_status: RoleStatus::Unrecognized {
            reason: role_reason,
        },
    })
}

/// The first documented signature rule the probe satisfies, with the family
/// it belongs to and the observed version word.
///
/// Only the INTERP rule is documented today (spec F06 research boundary), so
/// in practice this finds at most one family; a second rule matching the
/// same bytes would be a rule-table defect and is asserted in debug builds
/// — the whole test suite runs there.
fn documented_header_match(header: &[u8]) -> Option<(ZbdFamily, SignatureRule, u32)> {
    let mut found: Option<(ZbdFamily, SignatureRule, u32)> = None;
    for record in ZBD_FAMILY_INVENTORY.iter() {
        let Some(rule) = record.header_rule().signature() else {
            continue;
        };
        if let HeaderProbe::Match { version } = rule.probe(header) {
            debug_assert!(
                found.is_none(),
                "two documented ZBD header signatures must not match the same bytes"
            );
            found = Some((record.family(), rule, version));
        }
    }
    found
}
