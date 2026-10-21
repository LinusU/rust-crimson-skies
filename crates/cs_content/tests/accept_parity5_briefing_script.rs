//! Task #1750 `PARITY5-FRONTEND-BRIEFING-MAP`: the original `Briefing.zrd`
//! record read into typed commands, and the voice-over's cue points read into
//! marker times. Task test prefix: `accept_parity5_briefing_`.
//!
//! The documents here are built in the member's own binary form (`.zrd` tags
//! `1` int, `2` float, `3` text, `4` list) with invented names and numbers, so
//! nothing is hard-coded to one mission: another script gives other commands.
//! The retail member is read by the `cs_app` suite
//! (`accept_parity5_briefing_retail_m01_...`).

use cs_content::briefing::{
    BriefingCommand, BriefingDocument, DialogButtonKind, Point, sound_name_for, voice_member_in,
    wave_cue_points,
};
use cs_content::stunts::{ZrdValue, decode_zrd};

/// A `.zrd` node to encode.
enum N {
    I(u32),
    F(f32),
    T(&'static str),
    L(Vec<N>),
}

fn encode(node: &N, out: &mut Vec<u8>) {
    match node {
        N::I(value) => {
            out.extend(1_u32.to_le_bytes());
            out.extend(value.to_le_bytes());
        }
        N::F(value) => {
            out.extend(2_u32.to_le_bytes());
            out.extend(value.to_le_bytes());
        }
        N::T(text) => {
            out.extend(3_u32.to_le_bytes());
            out.extend((text.len() as u32).to_le_bytes());
            out.extend(text.as_bytes());
        }
        N::L(children) => {
            out.extend(4_u32.to_le_bytes());
            out.extend((children.len() as u32 + 1).to_le_bytes());
            for child in children {
                encode(child, out);
            }
        }
    }
}

fn bytes(node: &N) -> Vec<u8> {
    let mut out = Vec::new();
    encode(node, &mut out);
    out
}

fn pt(x: u32, y: u32) -> N {
    N::L(vec![N::I(x), N::I(y)])
}

fn named(name: &'static str, value: N) -> [N; 2] {
    [N::T(name), value]
}

fn flat(pairs: Vec<[N; 2]>) -> N {
    N::L(pairs.into_iter().flatten().collect())
}

fn label_record(string: &'static str, font: &'static str) -> N {
    N::L(vec![
        N::T(string),
        N::T("offset"),
        pt(98, 4),
        N::T("font"),
        N::L(vec![N::T(font)]),
    ])
}

fn button(x: u32, string: &'static str) -> N {
    flat(vec![
        named("POSITION", pt(x, 560)),
        named("BITMAP", N::L(vec![N::T("brief_button1")])),
        named("LABEL", label_record(string, "BtnLabelNormal")),
        named(
            "ROLLOVER",
            N::L(vec![
                N::T("LABEL"),
                label_record(string, "BtnLabelRollover"),
            ]),
        ),
        named(
            "ACTIVATE",
            N::L(vec![
                N::T("LABEL"),
                label_record(string, "BtnLabelActivate"),
            ]),
        ),
    ])
}

fn dialog() -> N {
    flat(vec![
        named(
            "PRIMITIVES",
            flat(vec![named(
                "OBJECTIVESLIST",
                flat(vec![
                    named(
                        "BACKGROUND",
                        flat(vec![
                            named("POSITION", pt(0, 295)),
                            named("BITMAP", N::L(vec![N::T("parchment")])),
                        ]),
                    ),
                    named(
                        "TITLE",
                        flat(vec![
                            named("TEXT", N::L(vec![N::T("MSG_DLG_OBJECTIVES")])),
                            named("FONT", N::L(vec![N::T("ObjListTitle")])),
                            named("POSITION", pt(35, 315)),
                        ]),
                    ),
                    named(
                        "LIST",
                        flat(vec![
                            named("FONT", N::L(vec![N::T("ObjList")])),
                            named("POSITION", pt(35, 335)),
                            named("WORDWRAP", pt(185, 240)),
                            named("SPACING", N::L(vec![N::I(5)])),
                        ]),
                    ),
                ]),
            )]),
        ),
        named(
            "BUTTONS",
            flat(vec![
                named("REPLAY", button(197, "MSG_BTN_REPLAY")),
                named("RETURNTOCABIN", button(397, "MSG_BTN_CABIN")),
                named("FLIGHTCHECK", button(597, "MSG_BTN_CHECK")),
            ]),
        ),
    ])
}

fn command(name: &'static str, args: Vec<N>) -> [N; 2] {
    [N::T(name), N::L(args)]
}

fn script(sound: &'static str, extra: Vec<[N; 2]>) -> N {
    let mut commands = vec![
        command(
            "PlaySound",
            vec![
                N::T("sound"),
                N::L(vec![N::T(sound)]),
                N::T("vol"),
                N::L(vec![N::F(0.5)]),
                N::T("markers"),
                N::L(vec![N::T("true")]),
                N::T("startmarker"),
                N::L(vec![N::I(0)]),
            ],
        ),
        command("WaitForMarker", vec![N::I(1)]),
        command(
            "Pict",
            vec![
                N::T("PIC"),
                N::T("bitmap"),
                N::L(vec![N::T("Photo")]),
                N::T("at"),
                pt(3, 229),
                N::T("center"),
                N::L(vec![N::T("true")]),
            ],
        ),
        command(
            "Fade",
            vec![
                N::T("PIC"),
                N::T("start"),
                N::L(vec![N::F(0.0)]),
                N::T("end"),
                N::L(vec![N::F(1.0)]),
                N::T("duration"),
                N::L(vec![N::F(0.25)]),
            ],
        ),
        command(
            "Spin",
            vec![
                N::T("PIC"),
                N::T("startrevs"),
                N::L(vec![N::F(-0.05)]),
                N::T("revs"),
                N::L(vec![N::F(0.05)]),
                N::T("duration"),
                N::L(vec![N::F(0.5)]),
            ],
        ),
        command(
            "Move",
            vec![
                N::T("PIC"),
                N::T("path"),
                N::L(vec![pt(3, 229), pt(150, 130)]),
                N::T("duration"),
                N::L(vec![N::F(0.5)]),
            ],
        ),
        command("On", vec![N::T("PIC")]),
        command("Wait", vec![N::F(2.5)]),
        command(
            "Objective",
            vec![N::T("TEXT1"), N::T("index"), N::L(vec![N::I(3)])],
        ),
    ];
    commands.extend(extra);
    flat(vec![
        named("IMAGE_PATH", N::L(vec![N::T("..\\img;..\\more")])),
        named(
            "BACKGROUND_IMAGES",
            N::L(vec![N::L(vec![N::T("TheMap"), N::I(0), N::I(0)])]),
        ),
        named(
            "CURSOR",
            N::L(vec![N::T("BITMAP"), N::L(vec![N::T("glove")])]),
        ),
        named("SCRIPT", N::L(commands.into_iter().flatten().collect())),
    ])
}

fn document(scripts: Vec<(&'static str, N)>) -> Vec<u8> {
    let mut pairs = vec![
        named("SHARED_IMAGE_PATH", N::L(vec![N::T("..\\shared")])),
        named("BRIEFINGDIALOG", dialog()),
    ];
    for (key, record) in scripts {
        pairs.push(named(key, record));
    }
    bytes(&N::L(vec![flat(pairs)]))
}

#[test]
fn accept_parity5_briefing_decodes_the_dialog_and_every_command_with_its_numbers() {
    let document = BriefingDocument::decode(&document(vec![
        ("brief_c91", script("briefing_c9m1", vec![])),
        ("brief_c92", script("briefing_c9m2", vec![])),
    ]))
    .expect("the member decodes");

    let dialog = &document.dialog;
    assert_eq!(dialog.objectives.background_bitmap, "parchment");
    assert_eq!(dialog.objectives.background_at, Point { x: 0, y: 295 });
    assert_eq!(dialog.objectives.title_text, "MSG_DLG_OBJECTIVES");
    assert_eq!(dialog.objectives.list_at, Point { x: 35, y: 335 });
    assert_eq!(dialog.objectives.wordwrap, (185, 240));
    assert_eq!(dialog.objectives.spacing, 5);
    assert_eq!(dialog.buttons.len(), 3);
    assert_eq!(
        dialog
            .buttons
            .iter()
            .map(|button| (button.kind, button.at.x, button.label_text.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (DialogButtonKind::Replay, 197, "MSG_BTN_REPLAY"),
            (DialogButtonKind::ReturnToCabin, 397, "MSG_BTN_CABIN"),
            (DialogButtonKind::FlightCheck, 597, "MSG_BTN_CHECK"),
        ]
    );
    assert_eq!(dialog.buttons[0].rollover_font, "BtnLabelRollover");
    assert_eq!(dialog.buttons[0].activate_font, "BtnLabelActivate");
    assert_eq!(dialog.buttons[0].label_offset, Point { x: 98, y: 4 });

    assert_eq!(document.scripts.len(), 2);
    let one = document
        .script_for_sound("BRIEFING_C9M1")
        .expect("the script that plays briefing_c9m1");
    assert_eq!(one.key, "brief_c91");
    assert_eq!(one.background[0].bitmap, "TheMap");
    let cue = one.sound.as_ref().expect("the script plays a sound");
    assert_eq!(cue.name, "briefing_c9m1");
    assert!((cue.volume - 0.5).abs() < 1e-6);
    assert!(cue.markers);
    assert_eq!(cue.start_marker, 0);
    assert_eq!(
        document
            .script_for_sound("briefing_c9m2")
            .map(|s| s.key.as_str()),
        Some("brief_c92")
    );
    assert!(document.script_for_sound("briefing_c9m3").is_none());

    assert_eq!(one.commands[1], BriefingCommand::WaitForMarker(1));
    assert_eq!(
        one.commands[2],
        BriefingCommand::Pict {
            id: "PIC".into(),
            bitmap: "Photo".into(),
            at: Point { x: 3, y: 229 },
            center: true
        }
    );
    assert_eq!(
        one.commands[3],
        BriefingCommand::Fade {
            id: "PIC".into(),
            start: 0.0,
            end: 1.0,
            duration: 0.25
        }
    );
    assert_eq!(
        one.commands[4],
        BriefingCommand::Spin {
            id: "PIC".into(),
            start_revs: Some(-0.05),
            revs: 0.05,
            duration: 0.5
        }
    );
    assert_eq!(
        one.commands[5],
        BriefingCommand::Move {
            id: "PIC".into(),
            path: [Point { x: 3, y: 229 }, Point { x: 150, y: 130 }],
            duration: 0.5
        }
    );
    assert_eq!(one.commands[6], BriefingCommand::On("PIC".into()));
    assert_eq!(one.commands[7], BriefingCommand::Wait(2.5));
    assert_eq!(
        one.commands[8],
        BriefingCommand::Objective {
            id: "TEXT1".into(),
            index: 3
        }
    );
    assert_eq!(one.highest_marker(), Some(1));
    assert_eq!(one.bitmaps(), vec!["TheMap", "Photo"]);
    assert!(one.unknowns().is_empty());
}

#[test]
fn accept_parity5_briefing_a_command_outside_the_known_set_is_recorded_not_skipped() {
    let document = BriefingDocument::decode(&document(vec![(
        "brief_c91",
        script(
            "briefing_c9m1",
            vec![command(
                "Shake",
                vec![N::T("PIC"), N::T("amount"), N::L(vec![N::F(1.0)])],
            )],
        ),
    )]))
    .expect("an unknown command is data, not a decode failure");
    let script = &document.scripts[0];
    assert_eq!(script.unknowns(), vec!["Shake"]);
    assert!(matches!(
        script.commands.last(),
        Some(BriefingCommand::Unknown { name }) if name == "Shake"
    ));
}

#[test]
fn accept_parity5_briefing_a_malformed_command_names_its_script_and_command() {
    let bad_move = command(
        "Move",
        vec![
            N::T("PIC"),
            N::T("path"),
            N::L(vec![pt(1, 1), pt(2, 2), pt(3, 3)]),
            N::T("duration"),
            N::L(vec![N::F(1.0)]),
        ],
    );
    let error = BriefingDocument::decode(&document(vec![(
        "brief_c91",
        script("briefing_c9m1", vec![bad_move]),
    )]))
    .expect_err("a three-point path is not the retail grammar");
    assert_eq!(error.scope, "brief_c91.Move");
    assert!(error.reason.contains("two points"), "{}", error.reason);

    let no_dialog = bytes(&N::L(vec![flat(vec![named(
        "brief_c91",
        script("briefing_c9m1", vec![]),
    )])]));
    let error = BriefingDocument::decode(&no_dialog).expect_err("the dialog record is required");
    assert!(error.reason.contains("BRIEFINGDIALOG"), "{error}");

    let error = BriefingDocument::decode(&[1, 2, 3]).expect_err("not a .zrd tree");
    assert!(error.reason.contains("not a .zrd tree"), "{error}");
}

#[test]
fn accept_parity5_briefing_the_voice_cue_name_follows_the_campaign_position() {
    assert_eq!(sound_name_for(1, 1), "briefing_c1m1");
    assert_eq!(sound_name_for(3, 5), "briefing_c3m5");
}

/// A RIFF/WAVE member with a `fmt ` chunk, a `cue ` chunk and a `data` chunk.
fn wave(rate: u32, cues: &[(u32, u32)]) -> Vec<u8> {
    let mut fmt = Vec::new();
    fmt.extend(2_u16.to_le_bytes()); // format tag
    fmt.extend(1_u16.to_le_bytes()); // channels
    fmt.extend(rate.to_le_bytes());
    fmt.extend((rate / 2).to_le_bytes());
    fmt.extend(1024_u16.to_le_bytes());
    fmt.extend(4_u16.to_le_bytes());
    let mut cue = Vec::new();
    cue.extend((cues.len() as u32).to_le_bytes());
    for (id, position) in cues {
        cue.extend(id.to_le_bytes());
        cue.extend(position.to_le_bytes());
        cue.extend(*b"data");
        cue.extend([0; 12]);
    }
    let chunk = |id: &[u8; 4], body: &[u8]| {
        let mut out = id.to_vec();
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(body);
        if body.len() % 2 == 1 {
            out.push(0);
        }
        out
    };
    let mut body = b"WAVE".to_vec();
    body.extend(chunk(b"fmt ", &fmt));
    body.extend(chunk(b"cue ", &cue));
    body.extend(chunk(b"data", &[0; 8]));
    let mut out = b"RIFF".to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

#[test]
fn accept_parity5_briefing_marker_n_is_the_nth_earliest_cue_point_in_seconds() {
    // Ids out of time order, as the retail briefing's are.
    let cues = wave_cue_points(&wave(1000, &[(1, 900), (2, 1500), (3, 300), (4, 100)]))
        .expect("the member reads");
    assert_eq!(cues.sample_rate, 1000);
    assert_eq!(
        cues.points.iter().map(|point| point.id).collect::<Vec<_>>(),
        vec![1, 2, 3, 4],
        "the chunk's own order is kept"
    );
    assert_eq!(cues.marker_seconds(), vec![0.1, 0.3, 0.9, 1.5]);

    // The same cue ids at a quarter of the rate keep their times.
    let quarter = wave_cue_points(&wave(250, &[(1, 225), (2, 375), (3, 75), (4, 25)]))
        .expect("the member reads");
    assert_eq!(quarter.marker_seconds(), vec![0.1, 0.3, 0.9, 1.5]);

    assert!(
        wave_cue_points(b"RIFX....WAVE").is_err(),
        "only RIFF/WAVE members are read"
    );
    let mut cut = wave(1000, &[(1, 1)]);
    cut.truncate(cut.len() - 30);
    assert!(wave_cue_points(&cut).is_err(), "a cut chunk is refused");
}

#[test]
fn accept_parity5_briefing_the_sound_group_names_the_voice_member() {
    let sounds = N::L(vec![
        N::T("brief_c61"),
        N::L(vec![N::L(vec![
            N::T("briefing_c1m1"),
            N::T("c1-HA-m1_briefing.wav"),
            N::T("PURGEABLE"),
        ])]),
        N::T("brief_c62"),
        N::L(vec![N::L(vec![
            N::T("briefing_c1m2"),
            N::T("c1-HA-m2_briefing.wav"),
            N::T("PURGEABLE"),
        ])]),
    ]);
    let tree = decode_zrd(&bytes(&N::L(vec![sounds]))).expect("decodes");
    assert_eq!(
        voice_member_in(&tree, "brief_c61", "briefing_c1m1").as_deref(),
        Some("c1-HA-m1_briefing.wav")
    );
    assert_eq!(
        voice_member_in(&tree, "brief_c62", "briefing_c1m2").as_deref(),
        Some("c1-HA-m2_briefing.wav")
    );
    assert!(
        voice_member_in(&tree, "brief_c61", "briefing_c1m2").is_none(),
        "a group maps only its own sound"
    );
    assert!(voice_member_in(&tree, "brief_c99", "briefing_c1m1").is_none());
    let _: &ZrdValue = &tree;
}
