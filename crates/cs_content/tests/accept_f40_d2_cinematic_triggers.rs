//! Acceptance scenario F40-D2: the measured trigger inventory of the ten
//! retail prerendered videos (task #661, a stage of
//! `specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`).
//!
//! `cs_content::cinematics::measure_retail_cinematics` reads decoded
//! original members — the `fmv.zrd` member of `ZBD/zrdr.zbd` and the
//! `ASSETS/LAYOUT.CSV`, `ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT`,
//! `ASSETS/SCRIPTS/FINALCINEMA.SCRIPT`, `ASSETS/SCRIPTS/PASSENGERCABIN.SCRIPT`,
//! `ASSETS/SCRIPTS/CTL.SCRIPT` and the six movie-control page members of
//! `GOSDATA/ASSETS/crimson.rof` — and fills every `Resolved` of the retail
//! records **only** where those bytes state the value. A value no named
//! source states stays an explicit `Resolved::Unknown` carrying its claim
//! id and reason (AGENTS.md rule 4).
//!
//! The non-retail tests write **synthetic source fixtures** (an authored
//! `.zrd` FMV document, an authored layout member and authored script
//! texts) that the production measurement really reads; each one pins one
//! measured decision or one measured absence, so removing the measurement
//! fails them.
//!
//! The retail test (`#[ignore = "requires CS_GAME_DIR"]`) reads the owner's
//! original installation through the production container discovery and the
//! production ROF tree reader, re-measures the inventory and pins every
//! trigger, skippability, semantic end and explicit unknown of all ten
//! videos, and validates the recorded fragment offsets against the members'
//! own bytes. Run it with `--include-ignored`; without `CS_GAME_DIR` it
//! fails loudly.

use std::fs;
use std::path::PathBuf;

use cs_content::cinematics::{
    CLAIM_RETAIL_SKIP_CAMPAIGN_INTRO, CLAIM_RETAIL_SKIP_ENGINE_FMV, CLAIM_RETAIL_SKIP_FINAL_CINEMA,
    CLAIM_RETAIL_SKIP_MENU_BACKDROP, CLAIM_RETAIL_SKIP_UNREAD, RetailCinematicSources,
    RetailSemanticEnd, RetailTrigger, UiPageSource, measure_retail_cinematics,
};
use cs_types::content::Resolved;

// --------------------------------------------------------- fixtures ------

/// A `.zrd` integer node: tag `1` and the value.
fn zrd_int(value: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

/// A `.zrd` text node: tag `3`, the byte length and the bytes.
fn zrd_text(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + text.len());
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// A `.zrd` list node: tag `4`, then **`children.len() + 1`** as the count
/// (the measured writer grammar), then the children.
fn zrd_list(children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + children.iter().map(Vec::len).sum::<usize>());
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&((children.len() as u32) + 1).to_le_bytes());
    for child in children {
        bytes.extend_from_slice(&child);
    }
    bytes
}

/// One `PLAYAVI` action naming `video`.
fn playavi(video: &str) -> Vec<u8> {
    zrd_list(vec![zrd_text("PLAYAVI"), zrd_text(video)])
}

/// The authored FMV document in the retail member's measured shape: a root
/// list holding one block of (name, body) pairs — `IMAGE_PATH`, `INTRO`,
/// `CHAP0`.
fn fmv_member(plays: Vec<(&str, Vec<Vec<u8>>)>) -> Vec<u8> {
    let mut block = vec![zrd_text("IMAGE_PATH"), zrd_list(vec![zrd_text("video")])];
    for (sequence, actions) in plays {
        block.push(zrd_text(sequence));
        block.push(zrd_list(actions));
    }
    zrd_list(vec![zrd_list(block)])
}

/// The authored full fixture's FMV member: `INTRO` plays MSopen1 then
/// zipper, `CHAP0` plays Chap0.
fn full_fmv() -> Vec<u8> {
    fmv_member(vec![
        (
            "INTRO",
            vec![
                zrd_list(vec![zrd_text("SHOWIMAGE"), zrd_text("MM_splashbackground")]),
                playavi("MSopen1.mpg"),
                playavi("zipper.mpg"),
            ],
        ),
        ("CHAP0", vec![playavi("Chap0.mpg")]),
    ])
}

/// The authored layout member: the retail record set (the six `M` records).
fn full_layout() -> String {
    [
        "[@MainMenu@]",
        ";Multimedia",
        "MOVIE=M,CrimFlag.MPG,0,0,0,250,250,0,0",
        "",
        "[@CampaignIntro@]",
        "CM_MOVIE=M,CrimFlag.MPG,0,0,0,250,250,1,0",
        "",
        "[@FinalCinema@]",
        "CF_MOVIE=M,Final.MPG,0,0,0,250,250,1,0",
        "",
        "[@Save@]",
        "MOVIE=M,CrimFlag.MPG,0,0,0,250,250,0,0",
        "",
        "[@Load@]",
        "MOVIE=M,CrimFlag.MPG,0,0,0,250,250,0,0",
        "",
        "[@Preferences@]",
        "MOVIE=M,CrimFlag.MPG,0,0,0,250,250,0,0",
        "",
    ]
    .join("\n")
}

/// The authored main-menu page: creates the backdrop movie control, states
/// no skip and no done handler.
fn backdrop_page() -> &'static str {
    "main\r\n{\r\ngui_create\r\n{\r\nobject GC = @ctl@AL\r\nGC.YC = \"movie\"\r\ninitialize(GC)\r\n}\r\n}\r\n"
}

/// The authored load/preferences page: the movie control's creation is
/// wrapped in the `if ($$KW$$)` (freturntogame) conditional.
fn guarded_backdrop_page() -> &'static str {
    "main\r\n{\r\ngui_create\r\n{\r\nif ($$KW$$)\r\n{\r\nobject GC = @ctl@AL\r\nGC.YC = \"movie\"\r\ninitialize(GC)\r\n}\r\n}\r\n}\r\n"
}

/// The authored save page: an `if ($$KW$$)` conditional that does **not**
/// wrap the creation (it follows it) — the brace-scope negative case.
fn unguarded_with_trailing_kw_page() -> &'static str {
    "main\r\n{\r\ngui_create\r\n{\r\nobject GC = @ctl@AL\r\nGC.YC = \"movie\"\r\ninitialize(GC)\r\n}\r\n}\r\ngui_mailbox\r\n{\r\nif ($$KW$$)\r\n{\r\nmail (20001, this)\r\n}\r\n}\r\n"
}

/// The authored campaign-intro page: constructs the chapter name, binds
/// `cm_movie`, skips on escape/space/return or click, and continues the
/// cabin in its done handler.
fn campaign_intro_page() -> &'static str {
    concat!(
        "main\r\n{\r\n",
        "gui_create\r\n{\r\nint EC = 0\r\nint FC\r\ncallback($$E$$, 2151, FC)\r\nobject GC = @ctl@AL\r\ninitialize(this)\r\n}\r\n",
        "gui_init\r\n{\r\nGC.BL = \"chap\" conv$(FC) \".mpg\"\r\nGC.YC = \"cm_movie\"\r\ninitialize(GC)\r\n}\r\n",
        "gui_char\r\n{\r\nswitch (getchar())\r\n{\r\ncase key_escape:\r\ncase key_space:\r\ncase key_return:\r\nmail (11006, this)\r\n}\r\n}\r\n",
        "lbutton_update\r\n{\r\nif (mouse.left == button_pressed)\r\n{\r\nmail (11006, this)\r\n}\r\n}\r\n",
        "gui_mailbox\r\n{\r\nif (11006 == getmessage())\r\n{\r\nif (!EC)\r\n{\r\nEC = 1\r\nscript_continue @passengercabin@\r\nscript_end @campaignintro@\r\n}\r\n}\r\n}\r\n",
        "}\r\n",
    )
}

/// The authored final-cinema page: binds `cf_movie`, skips on escape or
/// click, and runs the scrapbook in its done handler.
fn final_cinema_page() -> &'static str {
    concat!(
        "main\r\n{\r\n",
        "gui_create\r\n{\r\nint EC = 0\r\nobject GC = @ctl@AL\r\ninitialize(this)\r\n}\r\n",
        "gui_init\r\n{\r\nGC.YC = \"cf_movie\"\r\ninitialize(GC)\r\n}\r\n",
        "gui_char\r\n{\r\nif (key_escape == getchar())\r\n{\r\nmail (11006, this)\r\n}\r\n}\r\n",
        "lbutton_update\r\n{\r\nif (mouse.left == button_pressed)\r\n{\r\nmail (11006, this)\r\n}\r\n}\r\n",
        "gui_mailbox\r\n{\r\nif (11006 == getmessage())\r\n{\r\nif (!EC)\r\n{\r\nEC = 1\r\nscript_run \"assets\\\\scripts\\\\\" \"scrapbook.script\", 0x1000\r\nscript_end @finalcinema@\r\n}\r\n}\r\n}\r\n",
        "}\r\n",
    )
}

/// The authored passenger-cabin trigger chain.
fn passenger_cabin() -> &'static str {
    concat!(
        "gui_create\r\n{\r\n",
        "if (@globals@XQ)\r\n{\r\nint FC\r\ncallback($$E$$, 2151, FC)\r\n",
        "if (FC != 0)\r\n{\r\nscript_run \"assets\\\\scripts\\\\\" \"campaignintro.script\", 0x1999\r\nscript_pause @passengercabin@, 2\r\n}\r\n}\r\n",
        "}\r\n",
    )
}

/// The authored control member's movie-control fragments.
fn control_member() -> &'static str {
    concat!(
        "AL\r\n{\r\ngui_create\r\n{\r\nmovie CL\r\nstring BL\r\n}\r\n",
        "gui_init\r\n{\r\nif (equal$(BL,\"\"))\r\n{\r\nBL = $$NI$$\r\n}\r\n",
        "CL = \"assets\\\\graphics\\\\mpg\\\\\" BL\r\n}\r\n",
        "gui_execute\r\n{\r\nif (status_stopped == getstatus(CL))\r\n{\r\nmail (11006, parent)\r\n}\r\n}\r\n}\r\n",
    )
}

/// The ten retail file names, as the installation spells them.
fn retail_mpg_files() -> Vec<&'static str> {
    vec![
        "MSopen1.mpg",
        "zipper.mpg",
        "Chap0.mpg",
        "chap1.mpg",
        "chap2.mpg",
        "chap3.mpg",
        "chap4.mpg",
        "chap5.mpg",
        "CrimFlag.MPG",
        "Final.MPG",
    ]
}

/// The authored full fixture wired for the production measurement.
struct FullFixture {
    fmv: Vec<u8>,
    layout: String,
    mainmenu: &'static str,
    save: &'static str,
    load: &'static str,
    preferences: &'static str,
    campaignintro: &'static str,
    finalcinema: &'static str,
    cabin: &'static str,
    control: &'static str,
    mpg: Vec<&'static str>,
}

impl FullFixture {
    fn new() -> Self {
        Self {
            fmv: full_fmv(),
            layout: full_layout(),
            mainmenu: backdrop_page(),
            save: unguarded_with_trailing_kw_page(),
            load: guarded_backdrop_page(),
            preferences: guarded_backdrop_page(),
            campaignintro: campaign_intro_page(),
            finalcinema: final_cinema_page(),
            cabin: passenger_cabin(),
            control: control_member(),
            mpg: retail_mpg_files(),
        }
    }

    fn pages(&self) -> [UiPageSource<'_>; 6] {
        [
            UiPageSource {
                page: "mainmenu",
                script: self.mainmenu.as_bytes(),
            },
            UiPageSource {
                page: "save",
                script: self.save.as_bytes(),
            },
            UiPageSource {
                page: "load",
                script: self.load.as_bytes(),
            },
            UiPageSource {
                page: "preferences",
                script: self.preferences.as_bytes(),
            },
            UiPageSource {
                page: "campaignintro",
                script: self.campaignintro.as_bytes(),
            },
            UiPageSource {
                page: "finalcinema",
                script: self.finalcinema.as_bytes(),
            },
        ]
    }

    fn measure(&self) -> Vec<cs_content::cinematics::RetailCinematic> {
        let pages = self.pages();
        let sources = RetailCinematicSources {
            fmv_member: &self.fmv,
            layout_member: self.layout.as_bytes(),
            ui_pages: &pages,
            passenger_cabin: self.cabin.as_bytes(),
            control: self.control.as_bytes(),
            mpg_files: &self.mpg,
        };
        measure_retail_cinematics(&sources)
            .expect("the authored fixture measures")
            .cinematics()
            .to_vec()
    }
}

/// The claim id an unknown carries, for assertions.
fn unknown_claim(resolved: &Resolved<bool>) -> &str {
    let Resolved::Unknown { claim_id, .. } = resolved else {
        panic!("the value must stay an explicit unknown");
    };
    claim_id.as_str()
}

// ------------------------------------------------------- synthetic -------

#[test]
fn accept_f40_d2_full_chain_measures_every_video_and_trigger() {
    let cinematics = FullFixture::new().measure();
    let names: Vec<&str> = cinematics
        .iter()
        .map(|cinematic| cinematic.video())
        .collect();
    assert_eq!(
        names,
        [
            "MSopen1.mpg",
            "zipper.mpg",
            "Chap0.mpg",
            "CrimFlag.MPG",
            "Final.MPG",
            "chap1.mpg",
            "chap2.mpg",
            "chap3.mpg",
            "chap4.mpg",
            "chap5.mpg",
        ],
        "one record per named video, FMV names first, then layout names, then chapter files"
    );

    for (video, sequence) in [("MSopen1.mpg", "INTRO"), ("zipper.mpg", "INTRO")] {
        let cinematic = cinematics
            .iter()
            .find(|cinematic| cinematic.video() == video)
            .expect(video);
        assert!(
            matches!(
                cinematic.triggers(),
                [RetailTrigger::FmvSequence {
                    sequence: found,
                    ..
                }] if found == sequence
            ),
            "{video} is triggered by the engine FMV sequence {sequence}"
        );
        assert_eq!(
            unknown_claim(cinematic.skippable()),
            CLAIM_RETAIL_SKIP_ENGINE_FMV,
            "{video}'s skippability stays the engine-FMV unknown"
        );
        assert_eq!(
            *cinematic.semantic_end(),
            RetailSemanticEnd::EngineUnread,
            "{video}'s semantic end is engine-unread"
        );
    }

    let chap0 = cinematics
        .iter()
        .find(|cinematic| cinematic.video() == "Chap0.mpg")
        .expect("chap0 is named by the CHAP0 sequence");
    assert!(matches!(
        chap0.triggers(),
        [RetailTrigger::FmvSequence { sequence, .. }] if sequence == "CHAP0"
    ));

    for file in [
        "chap1.mpg",
        "chap2.mpg",
        "chap3.mpg",
        "chap4.mpg",
        "chap5.mpg",
    ] {
        let cinematic = cinematics
            .iter()
            .find(|cinematic| cinematic.video() == file)
            .unwrap_or_else(|| panic!("{file} is a construction product"));
        assert!(
            matches!(
                cinematic.triggers(),
                [RetailTrigger::ChapterConstruction {
                    callback_id: 2151,
                    ..
                }]
            ),
            "{file} is triggered by the measured chapter construction"
        );
        assert!(
            matches!(
                cinematic.skippable(),
                Resolved::Known(known) if known.value
            ),
            "{file}'s skippability is measured true from the intro page's skip handlers"
        );
        assert_eq!(
            *cinematic.semantic_end(),
            RetailSemanticEnd::UiScriptResume {
                page: "passengercabin".to_owned()
            },
            "{file}'s semantic end resumes the paused cabin page"
        );
    }
}

#[test]
fn accept_f40_d2_layout_pages_measure_backdrop_supersede_and_guard() {
    let cinematics = FullFixture::new().measure();

    let crimflag = cinematics
        .iter()
        .find(|cinematic| cinematic.video() == "CrimFlag.MPG")
        .expect("crimflag is named by five layout records");
    assert_eq!(crimflag.triggers().len(), 5, "one trigger per record");
    let pages: Vec<(&str, bool, bool)> = crimflag
        .triggers()
        .iter()
        .map(|trigger| {
            let RetailTrigger::UiMoviePage {
                page,
                freturntogame_guarded,
                file_superseded_by_constructed_name,
                ..
            } = trigger
            else {
                panic!("every crimflag trigger is a UI movie page");
            };
            (
                page.as_str(),
                *freturntogame_guarded,
                *file_superseded_by_constructed_name,
            )
        })
        .collect();
    assert_eq!(
        pages,
        [
            ("MainMenu", false, false),
            ("CampaignIntro", false, true),
            ("Save", false, false),
            ("Load", true, false),
            ("Preferences", true, false),
        ],
        "the CM record's file is superseded by the constructed name; the freturntogame guard \
         is brace-scoped (Save's trailing `if ($$KW$$)` does not guard its creation)"
    );
    assert_eq!(
        unknown_claim(crimflag.skippable()),
        CLAIM_RETAIL_SKIP_MENU_BACKDROP,
        "the backdrop's skippability stays the menu-backdrop unknown"
    );
    assert_eq!(
        *crimflag.semantic_end(),
        RetailSemanticEnd::LoopForever,
        "the # Loops 0 backdrop loops forever"
    );

    let final_video = cinematics
        .iter()
        .find(|cinematic| cinematic.video() == "Final.MPG")
        .expect("final is named by the CF record");
    assert!(matches!(
        final_video.triggers(),
        [RetailTrigger::UiMoviePage {
            page,
            loops: Some(1),
            file_superseded_by_constructed_name: false,
            ..
        }] if page == "FinalCinema"
    ));
    assert!(
        matches!(
            final_video.skippable(),
            Resolved::Known(known) if known.value
        ),
        "final's skippability is measured true from the cinema page's skip handlers"
    );
    assert_eq!(
        *final_video.semantic_end(),
        RetailSemanticEnd::UiScriptRun {
            script: "scrapbook.script".to_owned()
        },
        "final's semantic end runs the scrapbook page"
    );

    for cinematic in &cinematics {
        assert!(
            matches!(cinematic.pause(), Resolved::Unknown { .. }),
            "the pause policy stays an explicit unknown on {}",
            cinematic.video()
        );
        assert!(
            matches!(cinematic.clock(), Resolved::Unknown { .. }),
            "the master clock stays an explicit unknown on {}",
            cinematic.video()
        );
        assert!(
            matches!(cinematic.format(), Resolved::Known(known) if known.value == "mpg"),
            "{} carries the measured .mpg label",
            cinematic.video()
        );
        assert!(
            cinematic.actions().is_empty(),
            "no named source states an objective-event or aircraft-control action for {}",
            cinematic.video()
        );
    }
}

#[test]
fn accept_f40_d2_missing_skip_input_leaves_skippable_unknown_but_the_end_measured() {
    let mut fixture = FullFixture::new();
    // The intro page loses its skip inputs but keeps its done handler.
    fixture.campaignintro = concat!(
        "main\r\n{\r\n",
        "gui_create\r\n{\r\nint EC = 0\r\nint FC\r\ncallback($$E$$, 2151, FC)\r\nobject GC = @ctl@AL\r\ninitialize(this)\r\n}\r\n",
        "gui_init\r\n{\r\nGC.BL = \"chap\" conv$(FC) \".mpg\"\r\nGC.YC = \"cm_movie\"\r\ninitialize(GC)\r\n}\r\n",
        "gui_mailbox\r\n{\r\nif (11006 == getmessage())\r\n{\r\nscript_continue @passengercabin@\r\nscript_end @campaignintro@\r\n}\r\n}\r\n",
        "}\r\n",
    );
    // The cinema page loses its skip inputs too.
    fixture.finalcinema = concat!(
        "main\r\n{\r\n",
        "gui_create\r\n{\r\nint EC = 0\r\nobject GC = @ctl@AL\r\ninitialize(this)\r\n}\r\n",
        "gui_init\r\n{\r\nGC.YC = \"cf_movie\"\r\ninitialize(GC)\r\n}\r\n",
        "gui_mailbox\r\n{\r\nif (11006 == getmessage())\r\n{\r\nscript_run \"assets\\\\scripts\\\\\" \"scrapbook.script\", 0x1000\r\nscript_end @finalcinema@\r\n}\r\n}\r\n",
        "}\r\n",
    );
    let cinematics = fixture.measure();

    let chap1 = cinematics
        .iter()
        .find(|cinematic| cinematic.video() == "chap1.mpg")
        .expect("the chain still measures");
    assert_eq!(
        unknown_claim(chap1.skippable()),
        CLAIM_RETAIL_SKIP_UNREAD,
        "without a measured skip input the skippability stays the explicit unread unknown"
    );
    assert_eq!(
        *chap1.semantic_end(),
        RetailSemanticEnd::UiScriptResume {
            page: "passengercabin".to_owned()
        },
        "the done handler still measures the semantic end"
    );

    let final_video = cinematics
        .iter()
        .find(|cinematic| cinematic.video() == "Final.MPG")
        .expect("final is still named");
    assert_eq!(
        unknown_claim(final_video.skippable()),
        CLAIM_RETAIL_SKIP_UNREAD,
        "the cinema's skippability stays unknown without its skip handlers"
    );
    assert_eq!(
        *final_video.semantic_end(),
        RetailSemanticEnd::UiScriptRun {
            script: "scrapbook.script".to_owned()
        },
        "the cinema's semantic end is still measured"
    );
}

#[test]
fn accept_f40_d2_missing_construction_link_yields_no_chapter_records() {
    let mut fixture = FullFixture::new();
    // The cabin loses the `if (FC != 0)` gate: one chain link is absent.
    fixture.cabin = concat!(
        "gui_create\r\n{\r\n",
        "if (@globals@XQ)\r\n{\r\nint FC\r\ncallback($$E$$, 2151, FC)\r\n",
        "script_run \"assets\\\\scripts\\\\\" \"campaignintro.script\", 0x1999\r\n",
        "}\r\n}\r\n",
    );
    let cinematics = fixture.measure();
    assert_eq!(
        cinematics.len(),
        5,
        "without the whole chain no chap file becomes a record: MSopen1, zipper, Chap0, \
         CrimFlag, Final only"
    );
    assert!(
        cinematics.iter().all(|cinematic| !cinematic
            .video()
            .to_ascii_lowercase()
            .starts_with("chap")
            || cinematic.video().eq_ignore_ascii_case("Chap0.mpg")),
        "only the FMV-named Chap0 remains"
    );
}

#[test]
fn accept_f40_d2_movie_record_without_mpg_name_is_not_a_trigger() {
    let mut fixture = FullFixture::new();
    fixture.layout = format!(
        "{}\n[@Broken@]\nMOVIE=M,CrimFlag.bmp,0,0,0,250,250,0,0\nMOVIE=M\n",
        fixture.layout
    );
    let pages = fixture.pages();
    let sources = RetailCinematicSources {
        fmv_member: &fixture.fmv,
        layout_member: fixture.layout.as_bytes(),
        ui_pages: &pages,
        passenger_cabin: fixture.cabin.as_bytes(),
        control: fixture.control.as_bytes(),
        mpg_files: &fixture.mpg,
    };
    let inventory = measure_retail_cinematics(&sources).expect("the fixture still measures");
    assert_eq!(
        inventory.movie_record_without_mpg_name(),
        2,
        "a record whose file field is not an .mpg name, and a record without a file field, \
         are counted — never read as video references"
    );
    let names: Vec<&str> = inventory
        .cinematics()
        .iter()
        .map(|cinematic| cinematic.video())
        .collect();
    assert!(
        !names.iter().any(|name| name.ends_with(".bmp")),
        "the .bmp record never becomes a trigger"
    );
}

#[test]
fn accept_f40_d2_non_text_playavi_is_counted_not_named() {
    let mut fixture = FullFixture::new();
    fixture.fmv = fmv_member(vec![
        (
            "INTRO",
            vec![
                playavi("MSopen1.mpg"),
                zrd_list(vec![zrd_text("PLAYAVI"), zrd_int(7)]),
            ],
        ),
        ("CHAP0", vec![playavi("Chap0.mpg")]),
    ]);
    let pages = fixture.pages();
    let sources = RetailCinematicSources {
        fmv_member: &fixture.fmv,
        layout_member: fixture.layout.as_bytes(),
        ui_pages: &pages,
        passenger_cabin: fixture.cabin.as_bytes(),
        control: fixture.control.as_bytes(),
        mpg_files: &fixture.mpg,
    };
    let inventory = measure_retail_cinematics(&sources).expect("the fixture still measures");
    assert_eq!(
        inventory.playavi_not_text(),
        1,
        "a PLAYAVI action with a non-text argument is counted, never named"
    );
    assert!(
        inventory.by_video("7").is_none(),
        "the non-text argument never becomes a record"
    );
}

#[test]
fn accept_f40_d2_undecodable_fmv_member_is_an_error() {
    let fixture = FullFixture::new();
    let pages = fixture.pages();
    let sources = RetailCinematicSources {
        fmv_member: &[9, 0, 0, 0],
        layout_member: fixture.layout.as_bytes(),
        ui_pages: &pages,
        passenger_cabin: fixture.cabin.as_bytes(),
        control: fixture.control.as_bytes(),
        mpg_files: &fixture.mpg,
    };
    let error = measure_retail_cinematics(&sources).expect_err("an unknown .zrd tag must refuse");
    assert!(
        error.to_string().contains("fmv.zrd"),
        "the error names the member it could not decode: {error}"
    );
}

// ----------------------------------------------------------- retail ------

/// Reads one member of the retail `crimson.rof` through the production tree
/// walk and member decode.
fn retail_rof_member(root: &std::path::Path, member: &str) -> Vec<u8> {
    let bytes = fs::read(root.join("GOSDATA/ASSETS/crimson.rof")).expect("crimson.rof reads");
    let mut context = cs_formats::ParseContext::with_defaults("crimson.rof");
    let tree = cs_formats::read_tree(&mut context, &bytes).expect("the container tree walks");
    let wanted: Vec<&str> = member.split('/').collect();
    let entry = tree
        .members()
        .iter()
        .find(|entry| {
            entry.path.len() == wanted.len()
                && entry
                    .path
                    .iter()
                    .zip(&wanted)
                    .all(|(segment, name)| segment.eq_ignore_ascii_case(name.as_bytes()))
        })
        .unwrap_or_else(|| panic!("{member} is a member of crimson.rof"));
    cs_formats::read_member(&context, &bytes, entry, &cs_formats::RofLimits::default())
        .unwrap_or_else(|error| panic!("{member} must read: {error}"))
        .data
}

/// The retail `fmv.zrd` member through the production container discovery.
fn retail_fmv_member(root: &std::path::Path) -> Vec<u8> {
    let bytes = fs::read(root.join("ZBD/zrdr.zbd")).expect("zrdr.zbd reads");
    let path =
        cs_types::install::RelativePath::new("ZBD/zrdr.zbd").expect("the spelling is relative");
    let discovery = cs_formats::script_raw::discover_container("ZBD/zrdr.zbd", &path, &bytes);
    let fmv = discovery
        .programs()
        .iter()
        .find(|program| program.locator().member() == Some("fmv.zrd"))
        .expect("zrdr.zbd holds fmv.zrd");
    fmv.bytes().to_vec()
}

/// The retail `.mpg` file names under the engine's own MPG directory.
fn retail_mpg_names(root: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root.join("GOSDATA/ASSETS/GRAPHICS/MPG"))
        .expect("the MPG directory lists")
        .map(|entry| {
            entry
                .expect("a directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.to_ascii_lowercase().ends_with(".mpg"))
        .collect();
    names.sort();
    names
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f40_d2_retail_triggers_remeasure_from_the_installation() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));

    let fmv = retail_fmv_member(&root);
    let layout = retail_rof_member(&root, "ASSETS/LAYOUT.CSV");
    let cabin = retail_rof_member(&root, "ASSETS/SCRIPTS/PASSENGERCABIN.SCRIPT");
    let campaignintro = retail_rof_member(&root, "ASSETS/SCRIPTS/CAMPAIGNINTRO.SCRIPT");
    let finalcinema = retail_rof_member(&root, "ASSETS/SCRIPTS/FINALCINEMA.SCRIPT");
    let control = retail_rof_member(&root, "ASSETS/SCRIPTS/CTL.SCRIPT");
    let mainmenu = retail_rof_member(&root, "ASSETS/SCRIPTS/MAINMENU.SCRIPT");
    let save = retail_rof_member(&root, "ASSETS/SCRIPTS/SAVE.SCRIPT");
    let load = retail_rof_member(&root, "ASSETS/SCRIPTS/LOAD.SCRIPT");
    let preferences = retail_rof_member(&root, "ASSETS/SCRIPTS/PREFERENCES.SCRIPT");
    let mpg = retail_mpg_names(&root);
    let mpg_refs: Vec<&str> = mpg.iter().map(String::as_str).collect();

    let pages = [
        UiPageSource {
            page: "mainmenu",
            script: &mainmenu,
        },
        UiPageSource {
            page: "save",
            script: &save,
        },
        UiPageSource {
            page: "load",
            script: &load,
        },
        UiPageSource {
            page: "preferences",
            script: &preferences,
        },
        UiPageSource {
            page: "campaignintro",
            script: &campaignintro,
        },
        UiPageSource {
            page: "finalcinema",
            script: &finalcinema,
        },
    ];
    let sources = RetailCinematicSources {
        fmv_member: &fmv,
        layout_member: &layout,
        ui_pages: &pages,
        passenger_cabin: &cabin,
        control: &control,
        mpg_files: &mpg_refs,
    };
    let inventory = measure_retail_cinematics(&sources).expect("the retail installation measures");

    // Ten videos, each named by an original source, none minted from a file
    // name: the five chap files are the construction's product shape, every
    // other record is named by a PLAYAVI action or a layout record.
    let names: Vec<&str> = inventory
        .cinematics()
        .iter()
        .map(|cinematic| cinematic.video())
        .collect();
    let expected = [
        "msopen1.mpg",
        "zipper.mpg",
        "chap0.mpg",
        "crimflag.mpg",
        "final.mpg",
        "chap1.mpg",
        "chap2.mpg",
        "chap3.mpg",
        "chap4.mpg",
        "chap5.mpg",
    ];
    assert_eq!(names.len(), 10, "one record per retail video: {names:?}");
    for name in expected {
        assert!(
            names.iter().any(|found| found.eq_ignore_ascii_case(name)),
            "{name} is inventoried by the measurement; measured names: {names:?}"
        );
    }

    // Engine FMV sequences: INTRO names MSopen1 and zipper, CHAP0 names
    // Chap0; skippability and end stay the engine-FMV unknowns.
    for (video, sequence) in [
        ("MSopen1.mpg", "INTRO"),
        ("zipper.mpg", "INTRO"),
        ("Chap0.mpg", "CHAP0"),
    ] {
        let cinematic = inventory
            .by_video(video)
            .unwrap_or_else(|| panic!("{video} is named by the {sequence} sequence"));
        assert!(
            matches!(
                    cinematic.triggers(),
                    [RetailTrigger::FmvSequence {
                        sequence: found,
                        archive: "ZBD/zrdr.zbd",
                        member: "fmv.zrd",
                    }] if found == sequence
            ),
            "{video} is triggered by the engine FMV sequence {sequence}: {:?}",
            cinematic.triggers()
        );
        assert_eq!(
            unknown_claim(cinematic.skippable()),
            CLAIM_RETAIL_SKIP_ENGINE_FMV,
            "{video}'s skippability stays the engine-FMV unknown"
        );
        assert_eq!(
            *cinematic.semantic_end(),
            RetailSemanticEnd::EngineUnread,
            "{video}'s semantic end stays engine-unread"
        );
    }

    // The chapter construction: chap1..chap5, skippable via the measured
    // intro-page skip handlers, ending by resuming the cabin page.
    for file in [
        "chap1.mpg",
        "chap2.mpg",
        "chap3.mpg",
        "chap4.mpg",
        "chap5.mpg",
    ] {
        let cinematic = inventory
            .by_video(file)
            .unwrap_or_else(|| panic!("{file} is a construction product"));
        assert!(
            matches!(
                cinematic.triggers(),
                [RetailTrigger::ChapterConstruction {
                    page: "campaignintro",
                    callback_id: 2151,
                    ..
                }]
            ),
            "{file} is triggered by the measured chapter construction"
        );
        assert!(
            matches!(
                cinematic.skippable(),
                Resolved::Known(known)
                    if known.value
                        && known.provenance.claim_id.as_str()
                            == CLAIM_RETAIL_SKIP_CAMPAIGN_INTRO
            ),
            "{file}'s skippability is measured true under the campaign-intro claim"
        );
        assert_eq!(
            *cinematic.semantic_end(),
            RetailSemanticEnd::UiScriptResume {
                page: "passengercabin".to_owned()
            },
            "{file}'s semantic end resumes the paused cabin page"
        );
    }

    // CrimFlag: five layout records — the menu backdrop loops forever on
    // four pages, and the campaign-intro record's file is superseded by the
    // constructed chapter name.
    let crimflag = inventory
        .by_video("CrimFlag.MPG")
        .expect("crimflag is named by five layout records");
    assert_eq!(
        crimflag.triggers().len(),
        5,
        "one trigger per CrimFlag record: {:?}",
        crimflag.triggers()
    );
    let superseded: Vec<&str> = crimflag
        .triggers()
        .iter()
        .filter_map(|trigger| match trigger {
            RetailTrigger::UiMoviePage {
                page,
                file_superseded_by_constructed_name: true,
                ..
            } => Some(page.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        superseded,
        ["CampaignIntro"],
        "only the campaign-intro record's file is superseded by the constructed name"
    );
    let guards: Vec<(&str, bool)> = crimflag
        .triggers()
        .iter()
        .filter_map(|trigger| match trigger {
            RetailTrigger::UiMoviePage {
                page,
                freturntogame_guarded,
                ..
            } => Some((page.as_str(), *freturntogame_guarded)),
            _ => None,
        })
        .collect();
    assert_eq!(
        guards,
        [
            ("MainMenu", false),
            ("CampaignIntro", false),
            ("Save", false),
            ("Load", true),
            ("Preferences", true),
        ],
        "the load and preferences pages create their movie control behind the freturntogame \
         conditional; the main-menu, campaign-intro and save pages do not"
    );
    assert_eq!(
        unknown_claim(crimflag.skippable()),
        CLAIM_RETAIL_SKIP_MENU_BACKDROP,
        "the backdrop's skippability stays the menu-backdrop unknown"
    );
    assert_eq!(
        *crimflag.semantic_end(),
        RetailSemanticEnd::LoopForever,
        "the # Loops 0 backdrop loops forever"
    );

    // Final: played once by the final-cinema page, skippable via its
    // measured handlers, ending by running the scrapbook.
    let final_video = inventory
        .by_video("Final.MPG")
        .expect("final is named by the CF record");
    assert!(matches!(
        final_video.triggers(),
        [RetailTrigger::UiMoviePage {
            page,
            loops: Some(1),
            file_superseded_by_constructed_name: false,
            ..
        }] if page == "FinalCinema"
    ));
    assert!(
        matches!(
            final_video.skippable(),
            Resolved::Known(known)
                if known.value
                    && known.provenance.claim_id.as_str()
                        == CLAIM_RETAIL_SKIP_FINAL_CINEMA
        ),
        "final's skippability is measured true under the final-cinema claim"
    );
    assert_eq!(
        *final_video.semantic_end(),
        RetailSemanticEnd::UiScriptRun {
            script: "scrapbook.script".to_owned()
        },
        "final's semantic end runs the scrapbook page"
    );

    // Every scene: pause and clock unknown, the .mpg label measured, no
    // declared-vocabulary semantic actions.
    for cinematic in inventory.cinematics() {
        assert!(
            matches!(cinematic.pause(), Resolved::Unknown { .. }),
            "the pause policy stays an explicit unknown on {}",
            cinematic.video()
        );
        assert!(
            matches!(cinematic.clock(), Resolved::Unknown { .. }),
            "the master clock stays an explicit unknown on {}",
            cinematic.video()
        );
        assert!(
            matches!(cinematic.format(), Resolved::Known(known) if known.value == "mpg"),
            "{} carries the measured .mpg label",
            cinematic.video()
        );
        assert!(
            cinematic.actions().is_empty(),
            "no named source states an objective-event or aircraft-control action for {}",
            cinematic.video()
        );
    }
    assert_eq!(
        inventory.playavi_not_text(),
        0,
        "every retail PLAYAVI action carries a text name"
    );
    assert_eq!(
        inventory.movie_record_without_mpg_name(),
        0,
        "every retail layout movie record states an .mpg name"
    );

    // Every fragment the measurement searched for in the construction chain
    // is present in the retail members, and every recorded offset points at
    // the fragment's own bytes in the member it names.
    let member_bytes: Vec<(&str, &[u8])> = vec![
        ("CAMPAIGNINTRO.SCRIPT", &campaignintro),
        ("FINALCINEMA.SCRIPT", &finalcinema),
        ("PASSENGERCABIN.SCRIPT", &cabin),
        ("CTL.SCRIPT", &control),
        ("MAINMENU.SCRIPT", &mainmenu),
        ("SAVE.SCRIPT", &save),
        ("LOAD.SCRIPT", &load),
        ("PREFERENCES.SCRIPT", &preferences),
    ];
    for fragment in inventory.fragments() {
        let Some(offset) = fragment.first_offset else {
            continue;
        };
        let (_, member) = member_bytes
            .iter()
            .find(|(name, _)| fragment.source.ends_with(name))
            .unwrap_or_else(|| panic!("the fragment names a measured member"));
        let at = offset..(offset + fragment.fragment.len());
        assert_eq!(
            &member[at.clone()],
            fragment.fragment.as_bytes(),
            "the recorded offset of {:?} points at its own bytes in {}",
            fragment.fragment,
            fragment.source
        );
    }
    for name in [
        "CAMPAIGNINTRO.SCRIPT",
        "PASSENGERCABIN.SCRIPT",
        "CTL.SCRIPT",
    ] {
        assert!(
            inventory
                .fragments()
                .iter()
                .filter(|fragment| fragment.source.ends_with(name))
                .all(|fragment| fragment.first_offset.is_some()),
            "every construction-chain fragment of {name} is present in the retail member"
        );
    }
}
