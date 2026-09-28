//! The observed ZBD family inventory: families, their reader slots and the
//! evidence behind each dispatch rule.
//!
//! Spec F06's deliverable is "dispatch by validated header/version and
//! installation role into distinct sound, reader, texture, interp, GameZ and
//! animation readers", so this module is the typed half of the F06-A
//! inventory: one row per family, each row carrying the reader it routes to,
//! the header rule that may validate its bytes and the observed archive
//! names that identify it. The prose inventory, the citations and the
//! recorded unknowns live in
//! `docs/findings/2026-09-28-f06-a-zbd-family-inventory-and-dispatch.md`.
//!
//! Every rule is evidence-labelled (`cs_types::evidence::ClaimStatus`):
//! `documented` where a cited source states it, `observed_tool` where a value
//! was read from the retail installation but no source states it, `inferred`
//! where the family comes from reading an observed basename, `unknown` where
//! nothing is known yet. No rule here was written from an uninspected file
//! layout; the task #340 rules cite the pinned mech3ax v0.6.0 source (commit
//! `d3521a9721be731d365504568ddcd78e3f9846bb`, `docs/research/SOURCES.md`
//! S02/S06) and
//! `docs/findings/2026-09-28-t340-zbd-family-headers-and-archive-names.md`.

use cs_types::evidence::ClaimStatus;
use cs_types::install::FileFamily;

use super::header::{
    ANIMATION_SIGNATURE, ANIMATION_SIGNATURE_OFFSET, ANIMATION_VERSION, ANIMATION_VERSION_OFFSET,
    GAMEZ_SIGNATURE, GAMEZ_SIGNATURE_OFFSET, GAMEZ_VERSION, GAMEZ_VERSION_OFFSET, HeaderRule,
    INTERP_SIGNATURE, INTERP_SIGNATURE_OFFSET, INTERP_VERSION, INTERP_VERSION_OFFSET,
    SignatureRule,
};
use super::role::{RoleLevel, RoleRule};

/// One ZBD family: a `spec F06` reader target, not a file layout.
///
/// "ZBD" is only the extension families share, so the family is what
/// dispatch decides; the bytes behind it are validated per family by that
/// family's own reader (F06-B), never by a universal header.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ZbdFamily {
    /// Sound containers (sample format, channels, rate, loop metadata —
    /// spec F06 non-negotiable #2).
    Sound,
    /// Reader archives (byte content plus encoding evidence).
    Reader,
    /// Texture archives.
    Texture,
    /// The `INTERP.ZBD` loading-script container (F07).
    Interp,
    /// GameZ scene/mesh data: world geometry and `PLANES.ZBD` (F10).
    GameZ,
    /// Animation archives (`cam_anim`, `mis_anim`).
    Animation,
}

impl ZbdFamily {
    /// Every family, in the order the F06 deliverable names them.
    pub const ALL: [ZbdFamily; 6] = [
        Self::Sound,
        Self::Reader,
        Self::Texture,
        Self::Interp,
        Self::GameZ,
        Self::Animation,
    ];

    /// Stable lowercase family name (diagnostics, error messages).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sound => "sound",
            Self::Reader => "reader",
            Self::Texture => "texture",
            Self::Interp => "interp",
            Self::GameZ => "gamez",
            Self::Animation => "animation",
        }
    }

    /// The F02 open family-vocabulary label (`docs/contracts` +
    /// `cs_types::install::FileFamily`, whose doc comment names `zbd.sound`
    /// as exactly this kind of dispatch label).
    pub const fn file_family_label(self) -> &'static str {
        match self {
            Self::Sound => "zbd.sound",
            Self::Reader => "zbd.reader",
            Self::Texture => "zbd.texture",
            Self::Interp => "zbd.interp",
            Self::GameZ => "zbd.gamez",
            Self::Animation => "zbd.animation",
        }
    }

    /// The validated F02 family label for this family.
    ///
    /// The labels are static and ASCII, so validation cannot fail; the test
    /// suite still runs them through [`FileFamily::new`] rather than trusting
    /// that claim.
    pub fn file_family(self) -> FileFamily {
        FileFamily::new(self.file_family_label())
            .expect("ZbdFamily labels are valid FileFamily labels")
    }

    /// The reader slot dispatch routes this family to.
    pub const fn reader(self) -> ZbdReaderId {
        match self {
            Self::Sound => ZbdReaderId::Sound,
            Self::Reader => ZbdReaderId::Reader,
            Self::Texture => ZbdReaderId::Texture,
            Self::Interp => ZbdReaderId::Interp,
            Self::GameZ => ZbdReaderId::GameZ,
            Self::Animation => ZbdReaderId::Animation,
        }
    }
}

/// A distinct reader slot: the F06-B implementation target a probe is
/// routed to.
///
/// F06-A defines the slots and the routing only — no reader in this crate
/// parses container bytes yet (stage `### F06-A`: "define typed
/// inputs/outputs … do not jump ahead to a whole runtime"). Dispatch names
/// the slot explicitly so a caller routes without matching on the family and
/// AC01 ("route … to different readers") is observable directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ZbdReaderId {
    /// Reads sound containers.
    Sound,
    /// Reads reader archives.
    Reader,
    /// Reads texture archives.
    Texture,
    /// Reads `INTERP.ZBD` loading-script containers.
    Interp,
    /// Reads GameZ scene/mesh containers.
    GameZ,
    /// Reads animation archives.
    Animation,
}

impl ZbdReaderId {
    /// Stable lowercase slot name (spec F06 vocabulary: the sound, reader,
    /// texture, interp, GameZ and animation readers).
    pub const fn label(self) -> &'static str {
        self.family().as_str()
    }

    /// The family this slot reads.
    pub const fn family(self) -> ZbdFamily {
        match self {
            Self::Sound => ZbdFamily::Sound,
            Self::Reader => ZbdFamily::Reader,
            Self::Texture => ZbdFamily::Texture,
            Self::Interp => ZbdFamily::Interp,
            Self::GameZ => ZbdFamily::GameZ,
            Self::Animation => ZbdFamily::Animation,
        }
    }
}

/// One inventory row: a family, its reader slot, its dispatch rules and the
/// citation that put them there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZbdFamilyRecord {
    family: ZbdFamily,
    reader: ZbdReaderId,
    header_rule: HeaderRule,
    role_rules: &'static [RoleRule],
    source: &'static str,
}

impl ZbdFamilyRecord {
    /// The family this row describes.
    pub const fn family(self) -> ZbdFamily {
        self.family
    }

    /// The reader slot this family routes to.
    pub const fn reader(self) -> ZbdReaderId {
        self.reader
    }

    /// How this family's header is recognized, if it is.
    pub const fn header_rule(self) -> HeaderRule {
        self.header_rule
    }

    /// The observed archive names that identify this family under `zbd/`.
    ///
    /// Empty means no name in committed evidence has been tied to this
    /// family — an explicit unknown, never a default guess.
    pub fn role_rules(&self) -> &'static [RoleRule] {
        self.role_rules
    }

    /// Where this row comes from (a `docs/` or `specs/` citation, plus the
    /// recorded unknown when there is one).
    pub const fn source(self) -> &'static str {
        self.source
    }
}

/// Why the sound and reader families have no header rule: their archives
/// keep their index at the end of the file, not in a leading header.
const INDEXED_AT_END: &str = "no leading header: the pinned mech3ax v0.6.0 source reads sound and reader archives \
     (`crates/mech3ax-archive/src/archive.rs`, Crimson Skies = version one) from a trailer — \
     u32 version 1 and u32 member count in the last 8 bytes, preceded by the 148-byte member \
     table — and the retail archives start with member data (task #340 findings)";

/// Why the texture family has no header rule: its documented leading words
/// are constants, not a signature.
const TEXTURE_HEADER_NOT_A_SIGNATURE: &str = "the documented texture header (`crates/mech3ax-image/src/textures.rs` in mech3ax v0.6.0: \
     u32 0 at offset 0, u32 1 at offset 4) holds no signature — two constant words that other \
     data can carry too, so it cannot identify the family; the reference CLI selects it by \
     subcommand (task #340 findings)";

/// The inventory: one row per family, in the order the F06 deliverable
/// names them.
///
/// A `static` (not a `const`) so every borrow of a row — and of the role
/// rules behind it — is `'static` and dispatch can hand its evidence to a
/// caller without copying strings.
pub static ZBD_FAMILY_INVENTORY: [ZbdFamilyRecord; 6] = [
    // --- sound ---------------------------------------------------------
    ZbdFamilyRecord {
        family: ZbdFamily::Sound,
        reader: ZbdReaderId::Sound,
        header_rule: HeaderRule::Undocumented {
            reason: INDEXED_AT_END,
        },
        role_rules: &[RoleRule::prefixed(
            "sounds",
            ".zbd",
            &[RoleLevel::ContentRoot],
            ClaimStatus::Documented,
            "mech3ax v0.6.0 README: `sounds*.zbd` are sound archives, supported for Crimson Skies \
             [S02, S06]; observed as `ZBD/soundsl.zbd` and `ZBD/soundsh.zbd` (task #340 findings)",
        )],
        source: "mech3ax v0.6.0 README and `unzbd sounds` (\"Extract 'sounds*.zbd' archives\") \
                 [S02, S06]; the two retail archives sit directly under `ZBD/` (task #340 findings)",
    },
    // --- reader --------------------------------------------------------
    ZbdFamilyRecord {
        family: ZbdFamily::Reader,
        reader: ZbdReaderId::Reader,
        header_rule: HeaderRule::Undocumented {
            reason: INDEXED_AT_END,
        },
        role_rules: &[RoleRule::exact(
            "zrdr.zbd",
            &[
                RoleLevel::ContentRoot,
                RoleLevel::WorldGroup,
                RoleLevel::Mission,
            ],
            ClaimStatus::Documented,
            "mech3ax v0.6.0 README: `zrdr.zbd`/`reader*.zbd` are reader archives, supported for \
             Crimson Skies [S02, S06]; observed at `ZBD/`, in every world group and in every \
             mission directory (task #340 findings)",
        )],
        source: "mech3ax v0.6.0 README and `unzbd reader` (\"Extract 'reader*.zbd'/'zrdr.zbd' \
                 archives\") [S02, S06]",
    },
    // --- texture -------------------------------------------------------
    ZbdFamilyRecord {
        family: ZbdFamily::Texture,
        reader: ZbdReaderId::Texture,
        header_rule: HeaderRule::Undocumented {
            reason: TEXTURE_HEADER_NOT_A_SIGNATURE,
        },
        role_rules: &[
            RoleRule::exact(
                "texture.zbd",
                &[RoleLevel::WorldGroup],
                ClaimStatus::Documented,
                "docs/research/FORMAT-NOTES.md workflow example `unzbd cs textures <texture.zbd>` \
                 [S06]",
            ),
            RoleRule::prefixed(
                "rtexture",
                ".zbd",
                &[RoleLevel::WorldGroup],
                ClaimStatus::Documented,
                "mech3ax v0.6.0 README: `rtexture*.zbd` are image/texture packages [S02, S06]; \
                 observed in the world groups (task #340 findings)",
            ),
            RoleRule::exact(
                "rimage.zbd",
                &[RoleLevel::ContentRoot],
                ClaimStatus::Documented,
                "mech3ax v0.6.0 README: `rimage.zbd` is an image/texture package [S02, S06]; \
                 observed as `ZBD/rimage.zbd` (task #340 findings)",
            ),
        ],
        source: "docs/research/SOURCES.md S06 (`textures` subcommand), the FORMAT-NOTES workflow \
                 example and the mech3ax v0.6.0 README list of image/texture packages [S02]",
    },
    // --- interp --------------------------------------------------------
    ZbdFamilyRecord {
        family: ZbdFamily::Interp,
        reader: ZbdReaderId::Interp,
        header_rule: HeaderRule::Signature(SignatureRule::new(
            INTERP_SIGNATURE_OFFSET,
            INTERP_SIGNATURE,
            INTERP_VERSION_OFFSET,
            INTERP_VERSION,
            "docs/research/FORMAT-NOTES.md 'INTERP observed subset' [S07]; \
             specs/F07-interp-loading-script-container.md",
            ClaimStatus::Documented,
        )),
        role_rules: &[RoleRule::exact(
            "interp.zbd",
            &[RoleLevel::ContentRoot],
            ClaimStatus::Documented,
            "docs/research/FINDINGS.md: `INTERP.ZBD` is a loading-script container [S06, S07]; \
             observed at `ZBD/interp.zbd` \
             (docs/findings/2026-09-24-f02-c-cs-inspect-inventory-dependency-impact.md)",
        )],
        source: "docs/research/FORMAT-NOTES.md 'INTERP observed subset' [S07]: u32 signature \
                 0x08971119, u32 version 7, u32 script count",
    },
    // --- gamez ---------------------------------------------------------
    ZbdFamilyRecord {
        family: ZbdFamily::GameZ,
        reader: ZbdReaderId::GameZ,
        header_rule: HeaderRule::Signature(SignatureRule::new(
            GAMEZ_SIGNATURE_OFFSET,
            GAMEZ_SIGNATURE,
            GAMEZ_VERSION_OFFSET,
            GAMEZ_VERSION,
            "mech3ax v0.6.0 `crates/mech3ax-gamez/src/gamez/common.rs` (`SIGNATURE`, \
             `VERSION_CS`) and `gamez/cs/mod.rs` (`HeaderCsC`: u32 signature @0, u32 version @4) \
             [S02]; matched by all 9 retail GameZ archives (task #340 findings)",
            ClaimStatus::Documented,
        )),
        role_rules: &[
            RoleRule::exact(
                "planes.zbd",
                &[RoleLevel::ContentRoot],
                ClaimStatus::Documented,
                "specs/F10: 'GameZ data that supplies world geometry and PLANES.ZBD meshes'; \
                 docs/research/FINDINGS.md `ZBD/PLANES.ZBD`",
            ),
            RoleRule::exact(
                "gamez.zbd",
                &[RoleLevel::WorldGroup],
                ClaimStatus::Documented,
                "docs/research/FINDINGS.md: world-specific `gamez.zbd`; CLI example `unzbd cs \
                 gamez <PLANES.ZBD>` [S06]",
            ),
        ],
        source: "specs/F10 (planes and world geometry are GameZ data); docs/research/FINDINGS.md \
                 and SOURCES.md S02/S03/S06; header from mech3ax v0.6.0 [S02]",
    },
    // --- animation -----------------------------------------------------
    ZbdFamilyRecord {
        family: ZbdFamily::Animation,
        reader: ZbdReaderId::Animation,
        header_rule: HeaderRule::Signature(SignatureRule::new(
            ANIMATION_SIGNATURE_OFFSET,
            ANIMATION_SIGNATURE,
            ANIMATION_VERSION_OFFSET,
            ANIMATION_VERSION,
            "signature: mech3ax v0.6.0 `crates/mech3ax-anim/src/parse.rs` (`SIGNATURE`, u32 @0, \
             u32 version @4) [S02]; version 53: observed in all 61 retail `cam_anim.zbd`/\
             `mis_anim.zbd` archives — the pinned source documents no Crimson Skies version \
             (task #340 findings)",
            ClaimStatus::ObservedTool,
        )),
        role_rules: &[
            RoleRule::exact(
                "cam_anim.zbd",
                &[RoleLevel::WorldGroup],
                ClaimStatus::Documented,
                "mech3ax v0.6.0 README: `anim.zbd`/`cam_anim.zbd`/`mis_anim.zbd` are one \
                 animation family, not yet read for Crimson Skies [S02, S06]",
            ),
            RoleRule::exact(
                "mis_anim.zbd",
                &[RoleLevel::Mission],
                ClaimStatus::Documented,
                "mech3ax v0.6.0 README: `anim.zbd`/`cam_anim.zbd`/`mis_anim.zbd` are one \
                 animation family, not yet read for Crimson Skies [S02, S06]",
            ),
        ],
        source: "spec F06 deliverable names an animation reader; the mech3ax v0.6.0 README ties \
                 `cam_anim.zbd` (every world group) and `mis_anim.zbd` (every mission directory) \
                 to it [S02, S06]",
    },
];

/// The inventory row for `family`.
///
/// # Panics
///
/// Never, in practice: [`ZBD_FAMILY_INVENTORY`] holds exactly one row per
/// [`ZbdFamily::ALL`] variant, and a test pins that.
pub fn family_record(family: ZbdFamily) -> &'static ZbdFamilyRecord {
    ZBD_FAMILY_INVENTORY
        .iter()
        .find(|record| record.family() == family)
        .expect("ZBD_FAMILY_INVENTORY holds one row per ZbdFamily")
}
