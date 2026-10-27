//! `cs` — the Crimson Skies application binary.
//!
//! The request comes from [`cs_app::cli::parse`]: `--help`/`--version` exit 0
//! on stdout without touching the environment or the retail installation
//! (F00-B, acceptance case AC02), `--synthetic --headless --ticks <n>` runs
//! the fixed-tick synthetic smoke through [`cs_app::run`]
//! (F00-C, acceptance case AC03), and `--synthetic --headless --mission <id>
//! --tick <n> --pose <x,y,z>` runs the deterministic capture of F21-E and
//! prints its applied report. Whatever this binary cannot do, it reports on
//! stderr with a nonzero exit code — per `docs/contracts/CLI-EVIDENCE.md` a
//! failure is never returned as success.

use std::process::ExitCode;

use cs_app::cli::{self, CliRequest};
use cs_app::playtest;
use cs_app::run;

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    match cs_app::hd_textures::take_flag(&mut args) {
        Ok(None) => {}
        Ok(Some(dir)) => match cs_app::hd_textures::install(&dir) {
            Ok(pack) => eprintln!(
                "cs: HD pack {}: {} world and {} UI replacements",
                pack.dir().display(),
                pack.world_entries(),
                pack.ui_entries()
            ),
            Err(error) => {
                eprintln!("cs: {error}");
                return ExitCode::from(cli::EXIT_INVALID_INPUT);
            }
        },
        Err(reason) => {
            eprintln!("{}", cli::invalid_message(&reason));
            return ExitCode::from(cli::EXIT_INVALID_INPUT);
        }
    }
    let profile = match cs_app::user_data::take_flag(&mut args) {
        Ok(None) => None,
        Ok(Some(value)) => match cs_app::user_data::open_profile_dir(&value) {
            Ok(session) => Some(session),
            Err(error @ cs_app::user_data::ProfileDirError::Refused(_)) => {
                eprintln!("cs: {error}");
                return ExitCode::from(cli::EXIT_INVALID_INPUT);
            }
            Err(error) => {
                eprintln!("cs: {error}");
                return ExitCode::from(cli::EXIT_RUNTIME_FAILURE);
            }
        },
        Err(reason) => {
            eprintln!("{}", cli::invalid_message(&reason));
            return ExitCode::from(cli::EXIT_INVALID_INPUT);
        }
    };
    let code = dispatch(args);
    if let Some(session) = profile {
        // The session held the private population for the whole run; its
        // teardown report says where that was and whether anything was lost.
        match session.finish() {
            Ok(report) => eprintln!("cs: profile population {}", report.population.display()),
            Err(error) => {
                eprintln!("cs: {error}");
                return ExitCode::from(cli::EXIT_RUNTIME_FAILURE);
            }
        }
    }
    code
}

fn dispatch(args: Vec<String>) -> ExitCode {
    match cli::parse(args) {
        CliRequest::Help => {
            print!("{}", cli::HELP_TEXT);
            ExitCode::SUCCESS
        }
        CliRequest::Version => {
            println!("{}", cli::version_text());
            ExitCode::SUCCESS
        }
        CliRequest::Synthetic(request) => match run::run_synthetic(&request) {
            Ok(report) => {
                print_run_report(&report);
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("cs: {error}");
                ExitCode::from(error.exit_code())
            }
        },
        CliRequest::Capture(request) => match run::run_capture(&request) {
            Ok(report) => {
                print_capture_report(&report);
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("cs: {error}");
                ExitCode::from(error.exit_code())
            }
        },
        CliRequest::Playtest(request) => match playtest::run_playtest(&request) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("cs: {error}");
                ExitCode::from(cli::EXIT_RUNTIME_FAILURE)
            }
        },
        CliRequest::Mission(request) => match cs_app::mission_launch::launch_mission(&request) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("cs: {error}");
                ExitCode::from(error.exit_code())
            }
        },
        CliRequest::FrontEnd(request) => match cs_app::ui::front_end::run_front_end(&request) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("cs: {error}");
                ExitCode::from(cli::EXIT_RUNTIME_FAILURE)
            }
        },
        CliRequest::StartFlow(request) => match cs_app::ui::front_end::run_start_flow(&request) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("cs: {error}");
                ExitCode::from(cli::EXIT_RUNTIME_FAILURE)
            }
        },
        CliRequest::JoystickProbe(request) => match cs_app::input::hid::probe::run(&request) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("cs: {error}");
                ExitCode::from(cli::EXIT_RUNTIME_FAILURE)
            }
        },
        CliRequest::MissingInput => {
            eprintln!("{}", cli::missing_input_message());
            ExitCode::from(cli::EXIT_INVALID_INPUT)
        }
        CliRequest::Unsupported { args } => {
            eprintln!("{}", cli::unsupported_message(&args));
            ExitCode::from(cli::EXIT_INVALID_INPUT)
        }
        CliRequest::Invalid { reason } => {
            eprintln!("{}", cli::invalid_message(&reason));
            ExitCode::from(cli::EXIT_INVALID_INPUT)
        }
    }
}

/// Reports a finished run on stdout: the provenance marker, how far the world
/// really got, and where the trace went when one was requested.
fn print_run_report(report: &run::SyntheticRunReport) {
    println!(
        "synthetic headless run: {} {}/{} ticks at {} Hz, ended at tick {}",
        report.provenance.label(),
        report.ticks_completed,
        report.requested_ticks,
        cs_app::synthetic::TICK_HZ,
        report.final_sample.tick.0
    );
    if let Some(path) = &report.trace {
        println!("trace: {}", path.display());
    }
}

/// Reports an applied capture on stdout: the mission and tick it is of, every
/// override the request pinned (F21 non-negotiable behavior 5, "report all
/// overrides"), then each field's `f64 -> f32` narrowing in the pinned
/// projection, and where the trace went when one was requested.
fn print_capture_report(report: &run::CaptureRunReport) {
    let capture = &report.capture;
    println!(
        "synthetic capture: {} {} at tick {} ({} ticks at {} Hz)",
        report.provenance.label(),
        capture.mission(),
        capture.tick().0,
        report.ticks_completed,
        cs_app::synthetic::TICK_HZ,
    );
    println!(
        "frame: view {} aspect {} pose {:?}",
        capture
            .rig()
            .map_or("session's".to_string(), |rig| rig.label().to_string()),
        capture.aspect(),
        capture.pose(),
    );
    for entry in capture.overrides() {
        println!("override {}: {}", entry.field(), entry);
    }
    for narrowing in capture.pinned().narrowings() {
        println!(
            "narrowed {}: {} -> {} (drift {})",
            narrowing.field,
            narrowing.exact,
            narrowing.pinned,
            narrowing.drift()
        );
    }
    if let Some(path) = &report.trace {
        println!("trace: {}", path.display());
    }
}
