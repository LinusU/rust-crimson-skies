//! The mission's music state requests: the owner-ruled track identity and the
//! executable's measured selection rule (Rally #1463, `M01-PLAY-MUSIC`).
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stages
//! `### F41-B`/`### F41-C`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! # What a music state request is
//!
//! A `WAKEUP_SOUND_GROUP` naming one of the executable's seven built-in
//! `music_*_sg` groups is a **music state request**, not an ordinary cue
//! (`docs/findings/2026-10-06-m01-lc-directive-d-sound-help-timer-directives.md`).
//! `sounds.zrd` spells each state as the `MUSIC` leaf, a `DYNAMIC_WEIGHTS`
//! factor and its track list; the executable's dispatcher `0x5954f0` indexes a
//! built-in track table by the selected slot
//! (`docs/findings/2026-10-16-m01-play-music-state-requests.md`).
//!
//! # The identity rule (owner decision, designed)
//!
//! The owner ruled (task #1463 note of 2026-10-16, "rule (b), track-level"):
//!
//! * one [`ContentKind::Music`] id **per track**, over F14-D.7's
//!   [`install_file_key`](cs_content::catalog::baseline::install_file_key)
//!   spelling of `"<container>/<member>"` — the #1465 dialogue shape: one
//!   derivation, no new spelling. [`music_member_id`] is the [`voice_member_id`]
//!   sibling the ruling names, and the cue's id and any published id go
//!   through this one function;
//! * the container is #1438's **labelled choice**: `ZBD/soundsh.zbd` preferred
//!   when the member is present there, `ZBD/soundsl.zbd` otherwise. Which
//!   container the original opens is unmeasured (F14-D.7), so this is a
//!   labelled choice, never a measurement.
//!
//! # The selection rule (measured from the executable)
//!
//! `docs/findings/2026-10-16-m01-play-music-state-requests.md` §3 measured the
//! dispatcher's selection, and the owner ruled that it is reproduced as
//! measured:
//!
//! * **the four six-slot states** (`music_battle_sg`, `music_prebattle_sg`,
//!   `music_missionsuccess_sg`, `music_battlesuccess_sg`) draw a slot with a
//!   `rand()`-weighted pick over the group's child weights (`fcn.0059a440`):
//!   the draw is `rand()/32768 × Σw` walked against the accumulated weights —
//!   a child is picked when `accumulated + 0.001·Σw` reaches the draw, a draw
//!   that passes no child falls back to slot 0 — then the chosen weight is
//!   multiplied by the group's `DYNAMIC_WEIGHTS` factor, every weight below
//!   `0.001` is clamped up to it, and all weights are renormalized to sum
//!   `100.0`;
//! * **the three objective states** read the played handle's index off the
//!   shared counter `[0x9ad6c0]`: `music_secondaryobj_sg` reads `% 2` and
//!   advances it, `music_primaryobj_sg` reads `% 2` without advancing, and
//!   `music_tertiaryobj_sg` reads it raw (the measured oddity — bounded
//!   here by `% tracks.len()` so a counter past the table cannot index out
//!   of range, labelled designed where the original's bound is untraced).
//!   Each state's own counter `[0x9ad6c4]`/`[0x9ad6c8]` indexes only the
//!   name table its branch returns — a name the caller drops — so the
//!   shared counter alone decides the played track;
//! * the **track tables** are the image's built-in ones
//!   ([`IMAGE_MUSIC_STATES`]): where the image and `sounds.zrd` disagree —
//!   `music_prebattle_sg`, three distinct names filling six slots versus six
//!   distinct retail tracks — the image's table is followed, as ruled.
//!
//! # What is designed, by ruling
//!
//! * The RNG: the original draws `MSVCRT!rand` on the process stream — a seed
//!   and sequence no shipped file states. The selector's stream is
//!   [`SplitMix64::for_domain`] under [`MUSIC_DRAW_DOMAIN`], a documented
//!   designed constant, so an injected mission seed makes every draw
//!   deterministic;
//! * the initial per-child weights: only the post-draw arithmetic is
//!   measured. The draw is scale-free — `rand()·Σw` against weights whose
//!   ratios are what matter — so each armed weight starts at `1.0` and the
//!   first draw's distribution is unchanged whatever the original seeded;
//! * the `music_tertiaryobj_sg` bound noted above;
//! * the container choice (above);
//! * `music_member_id` itself (the ruling's designed convention — F14-D.7
//!   records that no container family states a cue class).
//!
//! Nothing here is `verified_original`: no original executable ran, and
//! whether a device plays the requested cue stays the owner's F41-D listening
//! gate, unclaimed.

use std::collections::BTreeMap;
use std::fmt;

use cs_content::catalog::baseline::install_file_key;
use cs_types::content::{ContentId, ContentKind};
use cs_types::random::SplitMix64;

/// The archive the labelled container choice prefers
/// (`docs/findings/2026-10-16-m01-play-music-state-requests.md` §9.2).
pub const PREFERRED_MUSIC_CONTAINER: &str = "ZBD/soundsh.zbd";

/// The archive the labelled container choice falls back to.
pub const FALLBACK_MUSIC_CONTAINER: &str = "ZBD/soundsl.zbd";

/// The domain constant of the music selector's draw stream.
///
/// Chosen once and documented here, exactly as `SYNTHETIC_BODY_DOMAIN` is for
/// its stream: the big-endian ASCII bytes `"MUSIC_DR"`, an arbitrary but
/// fixed `u64` — **designed**, because the original's `rand()` seed and
/// sequence are not the original's and no file states them.
pub const MUSIC_DRAW_DOMAIN: u64 = 0x4D55_5349_435F_4452;

/// The exact content id one music track member of `container` is known by.
///
/// The identity is [`ContentKind::Music`] over F14-D.7's
/// [`install_file_key`] spelling of `"<container>/<member>"` — the owner-ruled
/// track-level derivation (task #1463 note of 2026-10-16), the music sibling
/// of [`voice_member_id`](super::voice_member_id). The cue a
/// [`cs_sim::audio_events::MusicDirector`] request carries and any published
/// sample go through this one function, so both name the same id.
///
/// # Errors
///
/// [`MusicMemberError::Identity`] when the id grammar refuses the derived key.
pub fn music_member_id(container: &str, member: &str) -> Result<ContentId, MusicMemberError> {
    let spelling = format!("{container}/{member}");
    ContentId::from_source(ContentKind::Music, &install_file_key(&spelling)).map_err(|error| {
        MusicMemberError::Identity {
            container: container.to_owned(),
            member: member.to_owned(),
            reason: error.to_string(),
        }
    })
}

/// Why a music track member could not be identified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MusicMemberError {
    /// The member's own spelling derives no id: the F14-D.7 key grammar
    /// refuses it, so no cue can ever name it.
    Identity {
        /// The install-relative container the member was read from.
        container: String,
        /// The member's own name.
        member: String,
        /// The id grammar's own reason, unchanged.
        reason: String,
    },
}

impl fmt::Display for MusicMemberError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity {
                container,
                member,
                reason,
            } => write!(
                f,
                "music member {member:?} of {container:?} derives no content id: {reason}"
            ),
        }
    }
}

impl std::error::Error for MusicMemberError {}

/// Which measured mechanism picks one of a state's table slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MusicStateRule {
    /// The weighted `rand()` draw of `fcn.0059a440` over the group's child
    /// weights, with the `DYNAMIC_WEIGHTS` post-draw rescale — the four
    /// six-slot states.
    WeightedDraw,
    /// The shared counter `[0x9ad6c0]` read `% 2`, then advanced — the
    /// `music_secondaryobj_sg` branch.
    CounterAdvance,
    /// The shared counter `[0x9ad6c0]` read `% 2`, not advanced — the
    /// `music_primaryobj_sg` branch.
    CounterPeek,
    /// The shared counter `[0x9ad6c0]` read **raw** — the measured oddity of
    /// the `music_tertiaryobj_sg` branch (the finding's unknown #6: whether
    /// anything else keeps it in range was not traced). The selector bounds
    /// the read by the table's length so a counter past the slots cannot
    /// index out of range; that bound is labelled designed.
    CounterRaw,
}

/// One of the executable's seven built-in music states, as the image spells
/// it: the group a mission cue names, the measured rule that picks a slot,
/// and the track table the dispatcher indexes — the table's entries are
/// member names verbatim, so a slot names its track directly.
///
/// The tables were measured from the decrypted image
/// (`docs/findings/2026-10-16-m01-play-music-state-requests.md` §2) and are
/// re-read there by
/// `accept_m01_music_state_requests_match_the_executable_track_tables`; the
/// name and table virtual addresses recorded beside each entry are its
/// provenance. `music_prebattle_sg` is the measured exception where the image
/// disagrees with `sounds.zrd`: the image's table is followed, per the owner
/// ruling.
pub struct MusicStateSpec {
    /// The `music_*_sg` group name a mission cue spells.
    pub group: &'static str,
    /// The measured mechanism that picks this state's slot.
    pub rule: MusicStateRule,
    /// The state's built-in track table, in the image's spelled order: the
    /// member each slot plays.
    pub tracks: &'static [&'static str],
    /// The group's own name virtual address in the image.
    pub name_va: u32,
    /// The track table's virtual address in the image.
    pub table_va: u32,
}

/// The seven built-in music states the dispatcher `0x5954f0` names.
///
/// The order is the name table's order in the image (`0x63a4a8`…`0x63a530` by
/// `0x18`), not an ordering anything consumes.
pub const IMAGE_MUSIC_STATES: &[MusicStateSpec] = &[
    MusicStateSpec {
        group: "music_battlesuccess_sg",
        rule: MusicStateRule::WeightedDraw,
        tracks: &[
            "music_battlesuccess1.wav",
            "music_battlesuccess2.wav",
            "music_battlesuccess3.wav",
            "music_battlesuccess4.wav",
            "music_battlesuccess5.wav",
            "music_battlesuccess6.wav",
        ],
        name_va: 0x63a4a8,
        table_va: 0x63a088,
    },
    MusicStateSpec {
        group: "music_missionsuccess_sg",
        rule: MusicStateRule::WeightedDraw,
        tracks: &[
            "music_missionsuccess1.wav",
            "music_missionsuccess2.wav",
            "music_missionsuccess3.wav",
            "music_missionsuccess4.wav",
            "music_missionsuccess5.wav",
            "music_missionsuccess6.wav",
        ],
        name_va: 0x63a4c0,
        table_va: 0x63a058,
    },
    MusicStateSpec {
        group: "music_tertiaryobj_sg",
        rule: MusicStateRule::CounterRaw,
        tracks: &["music_tertiaryobj1.wav", "music_tertiaryobj2.wav"],
        name_va: 0x63a4d8,
        table_va: 0x63a038,
    },
    MusicStateSpec {
        group: "music_primaryobj_sg",
        rule: MusicStateRule::CounterPeek,
        tracks: &["music_primaryobj1.wav", "music_primaryobj2.wav"],
        name_va: 0x63a4f0,
        table_va: 0x63a028,
    },
    MusicStateSpec {
        group: "music_secondaryobj_sg",
        rule: MusicStateRule::CounterAdvance,
        tracks: &["music_secondaryobj1.wav", "music_secondaryobj2.wav"],
        name_va: 0x63a504,
        table_va: 0x63a030,
    },
    // Measured exception: the image's prebattle table repeats its three
    // distinct names to fill the six slots — `sounds.zrd` names six distinct
    // tracks, and the owner ruling follows the image's table.
    MusicStateSpec {
        group: "music_prebattle_sg",
        rule: MusicStateRule::WeightedDraw,
        tracks: &[
            "music_prebattle1.wav",
            "music_prebattle2.wav",
            "music_prebattle3.wav",
            "music_prebattle1.wav",
            "music_prebattle2.wav",
            "music_prebattle3.wav",
        ],
        name_va: 0x63a51c,
        table_va: 0x63a070,
    },
    MusicStateSpec {
        group: "music_battle_sg",
        rule: MusicStateRule::WeightedDraw,
        tracks: &[
            "music_battle1.wav",
            "music_battle2.wav",
            "music_battle3.wav",
            "music_battle4.wav",
            "music_battle5.wav",
            "music_battle6.wav",
        ],
        name_va: 0x63a530,
        table_va: 0x63a040,
    },
];

/// The built-in state a `music_*_sg` group names, when it is one.
#[must_use]
pub fn music_state_spec(group: &str) -> Option<&'static MusicStateSpec> {
    IMAGE_MUSIC_STATES.iter().find(|state| state.group == group)
}

/// What one resolved music group arms the selector with: the child count its
/// weights span (the `sounds.zrd` expansion's member count — the domain the
/// draw runs over) and the `DYNAMIC_WEIGHTS` factor it spells, if it spells
/// one.
///
/// `factor` is `None` for a group that spells no `DYNAMIC_WEIGHTS`: the
/// measured `[group+8]` flag then gates the whole post-draw block — no
/// factor multiply, no floor, no renormalization — so an unspelled group's
/// weights never move.
#[derive(Clone, Debug, PartialEq)]
pub struct MusicArm {
    /// The music group a cue can name.
    pub group: String,
    /// The child count the draw walks — the resolved member count.
    pub children: usize,
    /// The `DYNAMIC_WEIGHTS` factor the group spells, if any.
    pub factor: Option<f32>,
}

/// The track one music state request selected: the slot the rule picked and
/// the member that slot names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MusicPick {
    /// The slot the state's rule picked.
    pub slot: usize,
    /// The member that slot names in the state's built-in table.
    pub member: String,
}

/// The selector behind `0x59a440` and the three objective counters,
/// `docs/findings/2026-10-16-m01-play-music-state-requests.md` §3 reproduced
/// per the owner ruling: weighted draws for the four six-slot states, the
/// shared `[0x9ad6c0]` counter for the three objective states.
///
/// The draw stream is [`SplitMix64::for_domain`] over the mission seed under
/// [`MUSIC_DRAW_DOMAIN`] — the ruling's injected designed RNG — so one seed
/// replays one request sequence exactly.
#[derive(Clone, Debug)]
pub struct MusicSelector {
    rng: SplitMix64,
    /// The live child weights of each armed weighted state (the group's own
    /// dynamic weights — a `0x1c`-byte child record's `+8` float each).
    weights: BTreeMap<String, Vec<f32>>,
    /// The `DYNAMIC_WEIGHTS` factor each armed weighted state spells —
    /// absent for a group that spells none, the measured `[group+8]` flag
    /// that skips the whole post-draw block.
    factors: BTreeMap<String, f32>,
    /// The shared objective counter `[0x9ad6c0]`.
    counter: u32,
}

impl MusicSelector {
    /// A selector with no armed states: every `select` answers `None`, the
    /// counterpart of `MissionRadio::empty` for synthetic stages.
    #[must_use]
    pub fn empty(mission_seed: u64) -> Self {
        Self::launch(mission_seed, Vec::new())
    }

    /// Arms `armed` over a stream derived from `mission_seed`.
    ///
    /// Only weighted states arm per-group state — the objective states share
    /// the one counter and need no weights. A weighted group armed with zero
    /// children keeps no weights and its `select` answers `None`.
    #[must_use]
    pub fn launch(mission_seed: u64, armed: impl IntoIterator<Item = MusicArm>) -> Self {
        let mut selector = Self {
            rng: SplitMix64::for_domain(mission_seed, MUSIC_DRAW_DOMAIN),
            weights: BTreeMap::new(),
            factors: BTreeMap::new(),
            counter: 0,
        };
        for arm in armed {
            if music_state_spec(&arm.group)
                .is_some_and(|spec| spec.rule == MusicStateRule::WeightedDraw)
                && arm.children > 0
            {
                // Designed initial: uniform weights — the draw is scale-free,
                // so only the post-draw ratios the measured rule maintains
                // matter, never the absolute scale.
                selector
                    .weights
                    .insert(arm.group.clone(), vec![1.0; arm.children]);
                if let Some(factor) = arm.factor {
                    selector.factors.insert(arm.group, factor);
                }
            }
        }
        selector
    }

    /// The live child weights of an armed weighted state, in child order —
    /// the dynamic weights the measured post-draw rescale maintains. `None`
    /// for a counter state or a group the launch never armed.
    ///
    /// Exposed so the acceptance suite pins the measured rescale itself, not
    /// just its picks.
    #[must_use]
    pub fn weights(&self, group: &str) -> Option<&[f32]> {
        self.weights.get(group).map(Vec::as_slice)
    }

    /// The shared objective counter `[0x9ad6c0]`'s current value — the raw
    /// word the objective branches read and the secondary branch advances.
    #[must_use]
    pub const fn objective_counter(&self) -> u32 {
        self.counter
    }

    /// Picks one track of `group` by the state's measured rule.
    ///
    /// `None` when `group` is not one of the seven built-in states, or is a
    /// weighted state the launch never armed (a group the radio never
    /// resolved, or one whose expansion holds no children): the request is
    /// refused by name rather than answered with a guessed track.
    pub fn select(&mut self, group: &str) -> Option<MusicPick> {
        let spec = music_state_spec(group)?;
        let slot = match spec.rule {
            MusicStateRule::WeightedDraw => self.weighted_slot(group)?,
            // The measured branches mask the shared counter to the table's
            // `% 2` index: secondary advances the unmasked word, primary
            // only reads.
            MusicStateRule::CounterAdvance => {
                let slot = self.counter as usize % 2;
                self.counter = self.counter.wrapping_add(1);
                slot
            }
            MusicStateRule::CounterPeek => self.counter as usize % 2,
            // Tertiary reads the counter raw — the measured oddity; the
            // member lookup below bounds it by `% tracks.len()`.
            MusicStateRule::CounterRaw => self.counter as usize,
        };
        Some(MusicPick {
            slot,
            member: spec.tracks[slot % spec.tracks.len()].to_owned(),
        })
    }

    /// The measured `rand()`-weighted pick of `fcn.0059a440`, post-draw
    /// `DYNAMIC_WEIGHTS` rescale included.
    ///
    /// A one-child group returns slot 0 untouched — the measured early return
    /// leaves `[0x9ad730]` at its entry value zero. An unarmed group answers
    /// `None`.
    fn weighted_slot(&mut self, group: &str) -> Option<usize> {
        let weights = self.weights.get_mut(group)?;
        if weights.len() == 1 {
            return Some(0);
        }
        let total: f32 = weights.iter().sum();
        // `rand()/32768` measured at `0x609430`; the designed stream's
        // unit-interval draw takes its place — same `[0, 1)` domain.
        let draw = self.rng.unit_f64() as f32 * total;
        let mut accumulated = 0.0f32;
        let mut slot = 0usize;
        for (index, weight) in weights.iter().copied().enumerate() {
            accumulated += weight;
            if accumulated + WEIGHT_EPSILON * total >= draw {
                slot = index;
                break;
            }
        }
        // The post-draw `DYNAMIC_WEIGHTS` block runs only under the
        // `[group+8]` flag — a spelled factor: the chosen weight takes it,
        // every weight below `0.001` is clamped up to it (`0x59a536`), then
        // all are rescaled by the inverse total to sum `100.0` (`0x59a574`,
        // `0x6032e4`). An unspelled group skips the whole block — its
        // weights never move.
        if let Some(factor) = self.factors.get(group).copied() {
            weights[slot] *= factor;
            for weight in weights.iter_mut() {
                if *weight < WEIGHT_EPSILON {
                    *weight = WEIGHT_EPSILON;
                }
            }
            let rescaled: f32 = weights.iter().sum();
            for weight in weights.iter_mut() {
                *weight *= WEIGHT_SUM_TARGET / rescaled;
            }
        }
        Some(slot)
    }
}

/// The draw's threshold constant `0.001` (`0x6081bc`), both the pick
/// threshold (`0.001·Σw` past the accumulated weight) and the post-draw floor
/// a weight is clamped up to.
const WEIGHT_EPSILON: f32 = 0.001;

/// The post-draw renormalization target `100.0` (`0x6032e4`).
const WEIGHT_SUM_TARGET: f32 = 100.0;
