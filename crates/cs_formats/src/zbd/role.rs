//! Installation role: which family an archive belongs to because of where
//! it lives.
//!
//! Dispatch's second key (spec F06: "Dispatch by validated header/version
//! **and installation role**"). The rules below are the observed naming
//! conventions of the retail installation, already recorded in committed
//! evidence — `docs/findings/2026-09-24-f02-c-cs-inspect-inventory-dependency-impact.md`
//! and `docs/findings/2026-09-24-f02-d-installation-audit.md` enumerate
//! `interp.zbd` and `planes.zbd` at the `zbd` level, the four archives every
//! world group carries (`cam_anim.zbd`, `gamez.zbd`, `texture.zbd`,
//! `zrdr.zbd`), the varying `rtexture*.zbd` names and the two archives every
//! mission directory carries (`mis_anim.zbd`, `zrdr.zbd`).
//!
//! This module performs no I/O: it matches a `&RelativePath` the caller has
//! already discovered against a static table, so parsing stays independent
//! of asset-directory enumeration (`docs/01-ARCHITECTURE.md`) while the
//! mapping still lives inside this task's owner paths.

use std::fmt;

use cs_types::evidence::ClaimStatus;
use cs_types::install::RelativePath;

use super::family::{ZBD_FAMILY_INVENTORY, ZbdFamily};

/// The observed content root every `.zbd` archive in the retail
/// installation sits under (`ZBD/`, 184 archives — F02-D audit).
///
/// A path outside it matches no role rule: the observed naming conventions
/// were only ever established inside this root, so extending them to other
/// directories would be a guess.
pub const CONTENT_ROOT: &str = "zbd/";

/// Why a path matched no role rule: it lives outside [`CONTENT_ROOT`].
pub const OUTSIDE_CONTENT_ROOT: &str = "the path is outside the observed `zbd/` content root";

/// Why a path matched no role rule: it is inside [`CONTENT_ROOT`] but its
/// basename matches no archive name observed at its directory level.
pub const UNOBSERVED_NAME: &str =
    "the archive basename matches no role rule observed at its directory level";

/// The directory level under [`CONTENT_ROOT`] an archive name was observed
/// at (F02-C/F02-D findings: `zbd/<name>`, `zbd/<group>/<name>`,
/// `zbd/<group>/<mission>/<name>`).
///
/// A name observed at one level says nothing about the same name at
/// another, so every [`RoleRule`] lists the levels it was observed at and
/// matches nowhere else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RoleLevel {
    /// Directly under `zbd/` (`interp.zbd`, `planes.zbd`).
    ContentRoot,
    /// Directly inside a world group directory (`zbd/c1/gamez.zbd`).
    WorldGroup,
    /// Directly inside a mission directory of a world group
    /// (`zbd/c1/m02/mis_anim.zbd`).
    Mission,
}

impl RoleLevel {
    /// The level of a path given its components below [`CONTENT_ROOT`]
    /// (including the basename), or `None` when it is deeper than any
    /// observed level.
    pub const fn from_depth(components: usize) -> Option<Self> {
        match components {
            1 => Some(Self::ContentRoot),
            2 => Some(Self::WorldGroup),
            3 => Some(Self::Mission),
            _ => None,
        }
    }
}

/// How a role rule recognizes a logical basename.
///
/// Basenames are compared against [`RelativePath::logical_key`], so every
/// pattern is ASCII case-insensitive by construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RolePattern {
    /// The basename equals this value exactly.
    Exact(&'static str),
    /// The basename starts with `prefix` and ends with `suffix`
    /// (`rtexture*.zbd`: the observed names vary per world group).
    Prefixed {
        /// Leading part of the basename.
        prefix: &'static str,
        /// Trailing part of the basename, including the extension.
        suffix: &'static str,
    },
}

impl RolePattern {
    /// Whether a logical (lowercased) basename matches this pattern.
    ///
    /// Both sides come from [`RelativePath::logical_key`], so the comparison
    /// is byte-wise over already-lowercased ASCII.
    pub fn matches(self, basename: &str) -> bool {
        match self {
            Self::Exact(name) => basename == name,
            Self::Prefixed { prefix, suffix } => {
                basename.starts_with(prefix) && basename.ends_with(suffix)
            }
        }
    }

    /// Human-readable form for diagnostics and findings, e.g.
    /// `basename == "zrdr.zbd"`.
    pub fn describe(self) -> String {
        match self {
            Self::Exact(name) => format!("basename == {name:?}"),
            Self::Prefixed { prefix, suffix } => {
                format!("basename starts with {prefix:?} and ends with {suffix:?}")
            }
        }
    }
}

impl fmt::Display for RolePattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.describe())
    }
}

/// One observed archive name and the family it points at.
///
/// `evidence` is honest about *how* the pattern was tied to its family:
/// `documented` for a name a cited source states (an `interp` container, the
/// `gamez`/`texture` CLI examples), `inferred` for a name whose family comes
/// from reading the observed basename (`zrdr`, `cam_anim`, `mis_anim`,
/// `rtexture`). An inferred rule may route a container to a reader that then
/// rejects its bytes — which is the visible failure spec F06 AC02 wants,
/// never a silent fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoleRule {
    pattern: RolePattern,
    levels: &'static [RoleLevel],
    evidence: ClaimStatus,
    source: &'static str,
}

impl RoleRule {
    /// A rule for an exact observed basename at the observed `levels`.
    pub const fn exact(
        basename: &'static str,
        levels: &'static [RoleLevel],
        evidence: ClaimStatus,
        source: &'static str,
    ) -> Self {
        Self {
            pattern: RolePattern::Exact(basename),
            levels,
            evidence,
            source,
        }
    }

    /// A rule for the observed `rtexture*.zbd` shape at the observed
    /// `levels`.
    pub const fn prefixed(
        prefix: &'static str,
        suffix: &'static str,
        levels: &'static [RoleLevel],
        evidence: ClaimStatus,
        source: &'static str,
    ) -> Self {
        Self {
            pattern: RolePattern::Prefixed { prefix, suffix },
            levels,
            evidence,
            source,
        }
    }

    /// The basename pattern.
    pub const fn pattern(self) -> RolePattern {
        self.pattern
    }

    /// The directory levels this name was observed at.
    pub const fn levels(self) -> &'static [RoleLevel] {
        self.levels
    }

    /// Whether this rule matches `basename` at `level`.
    pub fn matches(self, level: RoleLevel, basename: &str) -> bool {
        self.levels.contains(&level) && self.pattern.matches(basename)
    }

    /// Evidence class of this rule.
    pub const fn evidence(self) -> ClaimStatus {
        self.evidence
    }

    /// Where this rule was observed (a `docs/` citation).
    pub const fn source(self) -> &'static str {
        self.source
    }
}

/// What the observed installation role says about one container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZbdRole {
    /// An observed archive-name rule matched.
    Observed {
        /// The family that rule points at.
        family: ZbdFamily,
        /// The rule that matched.
        rule: RoleRule,
    },
    /// Nothing matched: dispatch has no role evidence for this path.
    Unrecognized {
        /// Why no rule matched ([`OUTSIDE_CONTENT_ROOT`] /
        /// [`UNOBSERVED_NAME`]).
        reason: &'static str,
    },
}

impl ZbdRole {
    /// The family the role names, when it names one.
    pub const fn family(self) -> Option<ZbdFamily> {
        match self {
            Self::Observed { family, .. } => Some(family),
            Self::Unrecognized { .. } => None,
        }
    }

    /// Why the role is unrecognized, when it is.
    pub const fn unrecognized_reason(self) -> Option<&'static str> {
        match self {
            Self::Observed { .. } => None,
            Self::Unrecognized { reason } => Some(reason),
        }
    }
}

/// Resolves the observed installation role of `path`.
///
/// The logical key (case-folded, `/`-separated) must start with
/// [`CONTENT_ROOT`]; its basename and [`RoleLevel`] are then matched against
/// every role rule in [`ZBD_FAMILY_INVENTORY`], in inventory order. A path
/// deeper than the mission level matches nothing. The patterns are authored
/// disjoint (`Exact("texture.zbd")` never matches `rtexture2.zbd`), so the
/// first match wins and no rule can shadow another.
pub fn role_for_path(path: &RelativePath) -> ZbdRole {
    let key = path.logical_key();
    let Some(within_root) = key.strip_prefix(CONTENT_ROOT) else {
        return ZbdRole::Unrecognized {
            reason: OUTSIDE_CONTENT_ROOT,
        };
    };
    // `RelativePath` rejects empty components, so `within_root` is non-empty
    // and carries at least its final component.
    let basename = within_root.rsplit('/').next().unwrap_or(within_root);
    let Some(level) = RoleLevel::from_depth(within_root.split('/').count()) else {
        return ZbdRole::Unrecognized {
            reason: UNOBSERVED_NAME,
        };
    };
    for record in ZBD_FAMILY_INVENTORY.iter() {
        for rule in record.role_rules() {
            if rule.matches(level, basename) {
                return ZbdRole::Observed {
                    family: record.family(),
                    rule: *rule,
                };
            }
        }
    }
    ZbdRole::Unrecognized {
        reason: UNOBSERVED_NAME,
    }
}
