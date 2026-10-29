//! Acceptance task F52-W2-wire-menu-audio (#2199): the front end's music and
//! UI cues take the live accessibility session's bus levels.
//!
//! The `accept_f52_w2_menu_` tests call the production [`MenuAudio`] over a
//! recording device and change the shared session between calls: a copied
//! snapshot would not follow the change.

use std::sync::{Arc, Mutex};

use cs_app::accessibility::session::SettingsSession;
use cs_app::ui::front_end::{MenuAudio, MenuCue, MenuMix, MenuSoundTable};
use cs_content::audio::{AudioBus as SettingsBus, AudioLevel};
use cs_content::settings::Settings;
use cs_sim::audio_events::{DeviceCommand, RecordingAudioDevice};

const SCRIPT: &str = "\
main
{
gui_create
{
sound DR = \"assets\\\\sounds\\\\\" \"mouseclick.wav\"
sound ER = \"assets\\\\sounds\\\\\" \"mouseover.wav\"
sound HR = \"assets\\\\sounds\\\\\" \"entertext.wav\"
sound IR = \"assets\\\\sounds\\\\\" \"entertext_error.wav\"
object RR = @ctl@SK
RR.UK = \"music_splash.wav\"
initialize(RR)
}
}
";

fn setup(name: &str) -> (MenuAudio, RecordingAudioDevice, Arc<Mutex<SettingsSession>>) {
    let path = std::env::temp_dir().join(format!(
        "cs-f52-w2-menu-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&path);
    let session = Arc::new(Mutex::new(SettingsSession::open(path, false)));
    let device = RecordingAudioDevice::new();
    let handle = device.handle();
    let table = MenuSoundTable::parse(SCRIPT).expect("fixture parses");
    let audio = MenuAudio::new(Box::new(device), &table)
        .expect("opens")
        .with_settings(Arc::clone(&session));
    (audio, handle, session)
}

fn set_level(session: &Arc<Mutex<SettingsSession>>, bus: SettingsBus, level: f64) {
    session
        .lock()
        .unwrap()
        .apply(|s: &mut Settings| {
            s.presentation
                .bus_levels
                .insert(bus, AudioLevel::try_new(level).expect("level"));
        })
        .expect("apply");
}

fn started_gains(device: &RecordingAudioDevice) -> Vec<f64> {
    device
        .commands()
        .into_iter()
        .filter_map(|c| match c {
            DeviceCommand::Started { start, .. } | DeviceCommand::OneShotStarted { start, .. } => {
                Some(start.gain)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn accept_f52_w2_menu_music_and_cues_take_the_live_bus_levels() {
    let (mut audio, device, session) = setup("levels");
    set_level(&session, SettingsBus::Music, 0.5);
    set_level(&session, SettingsBus::Ui, 0.25);
    audio.start_music();
    audio.cue(MenuCue::Press);
    assert_eq!(started_gains(&device), vec![0.5, 0.25]);
    // Changed after the voices started: the next cue follows at once.
    set_level(&session, SettingsBus::Ui, 0.75);
    audio.cue(MenuCue::Rollover);
    assert_eq!(started_gains(&device).last(), Some(&0.75));
}

#[test]
fn accept_f52_w2_menu_playing_music_follows_a_level_change() {
    let (mut audio, device, session) = setup("refresh");
    audio.start_music();
    set_level(&session, SettingsBus::Music, 0.4);
    audio.refresh_bus_levels();
    audio.refresh_bus_levels();
    let updates: Vec<f64> = device
        .commands()
        .into_iter()
        .filter_map(|c| match c {
            DeviceCommand::Updated { update, .. } => Some(update.gain),
            _ => None,
        })
        .collect();
    assert_eq!(updates, vec![0.4], "one update, none when unchanged");
}

#[test]
fn accept_f52_w2_menu_defaults_leave_the_mix_unchanged() {
    let (mut audio, device, _session) = setup("default");
    audio.set_mix(MenuMix {
        music_on: true,
        music_gain: 0.6,
        effects_gain: 0.3,
    });
    audio.start_music();
    audio.cue(MenuCue::Press);
    assert_eq!(started_gains(&device), vec![0.6, 0.3]);
}
