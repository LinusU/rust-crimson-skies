//! `cs-inspect` — the command-line inspection binary.
//!
//! `--help` and `--version` exit 0 without a GPU or a retail installation and
//! without starting any asset discovery (F00 non-negotiable behavior 3). The
//! `inventory` command (F02-C) runs production discovery over the selected
//! installation and writes the inventory and dependency-impact report, and
//! the `audit` command (F02-D) classifies every inventoried file and runs
//! the full-content readiness check, and the `resolve` command (F04-C)
//! resolves one asset key in a mounted content session and reports its
//! trace, and the `rof` command (F05-C) mounts one ROF container into a
//! content session, reports every member it holds, optionally reads
//! and exports one of them and — with `--audit` (F05-D) — reads every
//! member to report both length words of each record and how the
//! container's bytes divide, the `zbd-audit` command (F06-D) lists every
//! ZBD container and member with a strict status, and the `interp`
//! command (F07-B) decodes and validates one INTERP loading-script
//! container and reports its lossless tokens, and with `--plan` reads the
//! same container as a loading plan (F07-C), resolving the registered
//! commands through a content session and reporting every failure with its
//! source offset, the `texture-audit` command (F08-D) compares every
//! ZBD texture's decode with a pinned reference extraction and can write a
//! private contact sheet, and the `config` command (F12-C) routes one
//! configuration member or PE resource image by its observed rule — a file
//! inside the installation selected by `--cs-path`/`CS_GAME_DIR` by its
//! installation-relative spelling — and resolves declared tuning fields and
//! localized string ids through the
//! `cs_content::config` consumers, and the `scripts` command (F13-B) routes
//! every ZBD container and locates and classifies its loading, mission and
//! animation programs, and — F13-C — probes each located program in isolated
//! runs for its instruction/native signature while reporting, with structural
//! evidence, which inventory record every located program reaches. The
//! `catalog` and `closure` commands (F14-C) inspect the canonical content
//! catalog and the transitive dependency closure of a launchable mission,
//! and — F14-D — read the complete private baseline inventory of an
//! installation selected by `--cs-path`/`CS_GAME_DIR`, so the coverage
//! denominator comes from the original data instead of a filtered list.
//! The `handling` command (F26-A) renders the declared handling probe and
//! reference-envelope schema of `cs_sim::probes` from its synthetic fixture;
//! it reads no installation and makes no original-fidelity claim, and retail
//! handling probing arrives with F26-C/D.
//! Until then the binary refuses invalid input with a nonzero exit code and
//! a diagnostic naming the missing command — a failure is never returned as success.

use std::process::ExitCode;

/// Exit code for invalid input or unsupported content (CLI-EVIDENCE contract).
const EXIT_INVALID_INPUT: u8 = 2;

const HELP_TEXT: &str = "\
cs-inspect — inspect a Crimson Skies installation

USAGE
    cs-inspect [OPTIONS] <COMMAND>

OPTIONS
    -h, --help       Print this help text and exit 0
    -V, --version    Print the version and exit 0

COMMANDS
    inventory [--cs-path <dir>] [--out <file>]
        Inventory the selected installation (--cs-path wins over
        CS_GAME_DIR) and write the JSON inventory and dependency-impact
        report to --out, or stdout without it.

    audit [--cs-path <dir>] [--scope all] [--strict] [--out <file>]
        Audit the selected installation: classify every inventoried file
        and run the full-content readiness check. Exits 0 when the
        installation passes, 3 when it does not.

    resolve [--cs-path <dir>] --asset <namespace>:<path> [--variant <v>]
            [--world <group>] [--locale <l>] [--mission <m>] [--out <file>]
            [--export-dir <dir>]
        Mount the installation into one content session and resolve the key
        (namespaces: install, world). The JSON report holds the span, every
        ordered attempt and the precedence status. --export-dir writes the
        resolved member into a private directory outside the installation.
        Exits 0 when resolved, 3 when not found or ambiguous.

    rof [--cs-path <dir>] --container <spelling> [--member <spelling>]
        [--max-decoded-bytes <n>] [--audit] [--out <file>] [--export-dir <dir>]
        Mount one ROF container of the installation into a content
        session and report every member it holds: spelling, id, stored
        extent, compression bit and digest. --member reads one member
        through the bounded decoder and --export-dir writes its decoded
        bytes into a private directory outside the installation. --audit
        reads every member instead of one and reports both length words of
        each record, the bytes the read produced, the trailing bytes inside
        each stored extent, the stored and decoded digests and how the
        container's bytes divide between its directory blocks and its
        members' stored extents. Exits 0 when mounted, 3 when the
        container, the member or the audit is refused.

    zbd-audit [--cs-path <dir>] [--strict] [--out <file>]
        Audit every ZBD container of the installation family by family:
        route it, read its own member index and give every member a row
        (decoded, readable or failed). Exits 3 when a container or member
        is corrupt, and with --strict also when any content is left
        uninterpreted. Never claims playability.

    interp --file <path> [--out <file>] [--raw] [--plan] [--commands <file>]
           [--cs-path <dir>] [--world <group>]
        Decode and validate one INTERP loading-script container (F07-B), and
        with --plan read it as a loading plan (F07-C): classify every line
        against a command table, resolve the registered commands through a
        mounted content session, and report every failure with its source
        offset and the world it affects. The JSON report holds the header,
        every script's origin and extent, and each line's arguments as
        offsets and byte strings; names and arguments have no established
        encoding, so they are reported as length and hex, never as text.
        --raw adds the unvalidated raw records next to the decoded tokens.
        --commands supplies the command table the workspace does not ship:
        which commands load resources is unmeasured (F07-D). Exits 0 when the
        container decodes with no findings and, with --plan, when the plan is
        complete; 3 when it is refused, has findings or the plan is
        incomplete.

    texture-audit [--cs-path <dir>] --reference <dir> [--sheet-dir <dir>]
                  [--out <file>]
        Decode every ZBD texture through the catalog's upload boundary and
        compare dimensions, coverage, orientation and every texel with the
        pinned reference extraction in --reference (<dir>/<archive>.zip,
        written by `unzbd cs textures`, mech3ax v0.6.0). --sheet-dir writes
        one private contact sheet (TGA) per archive. Exits 0 when no
        unexplained difference remains, 3 otherwise.

    config --file <path> [--cs-path <dir>] [--container <spelling>]
           [--member <member>] [--install-sha256 <hex>]
           [--string <id>[:<language>]]...
           [--field <consumer>=<section>:<key>:<index>:<width>:<signed>]...
           [--out <file>]
        Read one configuration member or PE resource image (F12-C), routed by
        its observed member rule (--container/--member override the routing
        for a loose export). A `strings.dll`, `language.dll` or `langui.dll`
        image is read as inert PE data into the string catalog and --string
        resolves a stable id to its text, language, code page and provenance;
        a keyed list (LAYOUT.CSV) is read into a lossless document and
        --field resolves a declared value to a checked tuning constant
        (width 8|16|32|64|f32|f64; signed|unsigned for a whole number). The
        inventory spells a loose member relative to the installation root, so
        a file inside the installation selected by --cs-path (which wins over
        CS_GAME_DIR) is routed by its installation-relative spelling and a
        loose export by its file name. Exits 0 when the file reads and every
        request resolves, 2 on invalid input, 3 when the member is unrouted
        or refused, or a requested lookup or field does not resolve, and 1 on
        a runtime failure.

    scripts [--cs-path <dir>] [--coverage] [--signatures <file>]
            [--word-bytes <n>] [--budget <n>] [--out <file>]
        Route every ZBD container of the installation (F13-B) and locate and
        classify its loading, mission and animation programs: one documented
        loading program per INTERP script body, one program per reader-archive
        member (named by member name and mission path) and one program per
        animation container, each reported as a container/member byte range
        and the role its name or path supports. Program bytes are never
        copied into the report. --coverage requires every container to route
        and every script container to locate a program without a finding;
        exits 3 when it does not. F13-C adds an isolated signature probe:
        every inventory record reports whether a located program reaches it,
        with the structural evidence for that verdict, and every located
        program is walked in isolation against the signature table.
        --signatures supplies caller-measured claims, one per line as
        `<opcode> <spelling> <program> <arity> <signature> <effects> <timing>
        <errors> <citation> [note]`; the workspace ships no such file because
        the mission opcode table is unmeasured, so without it nothing is
        resolved and probe.complete is false. With --signatures, coverage
        also requires every program to be resolved. --word-bytes and --budget
        state the assumed instruction unit and the per-program budget; their
        defaults are reported as probe.assumed. Exits 0 when the command runs
        and the requested coverage holds, 3 when it does not, 2 for invalid
        input (including a malformed signature file), 4 when no installation
        is selected.

    catalog [--cs-path <dir>] [--out <file>]
        Inspect the canonical content catalog (F14-C/F14-D): report every row
        with its parse, normalize and readiness state and the declared
        launchable baseline, and write the deterministic JSON report. With
        --cs-path (which wins over CS_GAME_DIR) the rows are the complete
        private baseline inventory read from that installation: one row per
        inventoried file, one row per campaign mission program, one declared
        launchable row per campaign mission, plus the reachable/unreachable
        coverage accounting and the reader-archive directories no campaign
        mission claims. Without an installation the rows are the validated
        synthetic catalog fixture, whose report names its source and is never
        retail-ready, so a synthetic row is never presented as a retail
        catalog entry. Exits 0 when the report is written, 2 on invalid
        input, 3 when the installation declares no campaign mission or a
        declared mission has no program archive, and 1 on a runtime failure.

    closure [--cs-path <dir>] --mission <catalog-id> [--strict] [--out <file>]
        Inspect the transitive dependency closure (F14-C/F14-D) of one
        declared launchable mission/scenario over the same catalog: report
        every reached node with its predecessor chain from the mission, its
        readiness and the orphaned references, and write the deterministic
        JSON closure report. With --cs-path the closure runs over that
        installation's baseline inventory; without one it runs over the
        synthetic fixture. --strict exits 3 when the closure is not complete
        (a node is unavailable or a reference is orphaned). Exits 0 when the
        closure computes and, with --strict, is complete; 2 on invalid input
        or an unknown/non-launchable mission; 3 on failed validation.

    campaign [--cs-path <dir>] [--out <file>]
        Read the installation's retail campaign directory layout (F14-E) and
        write the deterministic JSON report: every
        ZBD/<chapter><variant>/<mission> directory with its chapter, mission
        number, world group, program archive path and presence, and the
        SHA-256 of each present archive (paths and hashes only, no original
        bytes). --cs-path wins over CS_GAME_DIR. Exits 0 when the report is
        produced, 2 on invalid input, 3 when the installation declares no
        campaign mission directory, 4 when no installation is selected and 1
        on a runtime failure. The walk is the same production derivation the
        per-mission binding stages and F14-D use; the report makes no
        readiness or playability claim.

    handling [--out <file>]
        Render the declared handling probe and reference-envelope schema
        (F26-A) as a JSON report: the ten probe maneuvers with the quantity and
        unit each bounds, and the declared synthetic envelope's recorded input,
        initial state, difficulty, loadout, timing uncertainty, tolerance and
        held-out entry, plus the two comparison outcomes (a covering candidate
        passes; a tuned acceleration curve whose held-out turn radius is
        outside the envelope does not). It reads no installation, flies no
        probe and makes no original-fidelity claim, so it is never retail-
        ready. Retail handling probing arrives with F26-C/D. Exits 0 when the
        report is produced, 2 on invalid input, 1 on a runtime failure.

    routes [--follow] [--out <file>]
    routes --coverage [--cs-path <dir>] [--out <file>]
        Render the declared route-graph contract (F31-A) as a JSON report:
        the authored route id, origin, reference frame, termination, clearance
        and every node with its stable id, authored sequence, mandatory flag,
        resolved position, arrival radius and resolved trigger volume, plus the
        declared edges. The inspected record is the authored synthetic arch
        fixture, which names its synthetic source and is never retail-ready;
        unknown positions, arrival radii, trigger volumes and clearances are
        reported as explicit unknowns, never as a silent zero. With --follow
        (F31-C) the record is resolved and projected into the runtime
        cs_sim route graph and the production follower is driven end to end: a
        displaced actor rejoins before its next mandatory marker, the roster is
        torn down and a fresh session generation retries from scratch, while an
        unbound moving anchor or a loop termination is refused by name. With
        --coverage (F31-D) the command audits the retail installation's every
        ZBD/<group>/<mission> directory, classifies each by mission type
        (campaign M##, Instant Action IA#, multiplayer MP#) and checks that its
        reader archive locates the observed AI-navigation control carrier
        member aiv.zrd. It is a carrier coverage, not a route decode: the
        original route encoding stays unmeasured and the report says so. Exits
        0 when the report is produced (and coverage holds), 2 on invalid input,
        3 when coverage fails, 4 when --coverage has no installation, and 1 on
        a runtime failure building a report or writing --out.

`--help` and `--version` read no environment variable and open no
installation.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    for arg in &args {
        match arg.as_str() {
            "--help" | "-h" => {
                print!("{HELP_TEXT}");
                return ExitCode::SUCCESS;
            }
            "--version" | "-V" => {
                println!("cs-inspect {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            _ => {}
        }
    }

    match args.first().map(String::as_str) {
        None => {
            eprintln!(
                "cs-inspect: missing command; expected one of inventory, audit, resolve, rof, \
                 zbd-audit, interp, texture-audit, config, scripts, catalog, closure, campaign, \
                 routes, handling (see docs/contracts/CLI-EVIDENCE.md)"
            );
            ExitCode::from(EXIT_INVALID_INPUT)
        }
        Some("inventory") => cs_inspect::install::inventory_command(&args[1..]),
        Some("audit") => cs_inspect::install::audit_command(&args[1..]),
        Some("resolve") => cs_inspect::resolve::resolve_command(&args[1..]),
        Some("rof") => cs_inspect::rof::rof_command(&args[1..]),
        Some("zbd-audit") => cs_inspect::zbd::zbd_audit_command(&args[1..]),
        Some("interp") => cs_inspect::interp::interp_command(&args[1..]),
        Some("texture-audit") => cs_inspect::textures::texture_audit_command(&args[1..]),
        Some("config") => cs_inspect::config::config_command(&args[1..]),
        Some("scripts") => cs_inspect::script_discovery::scripts_command(&args[1..]),
        Some("catalog") => cs_inspect::catalog::catalog_command(&args[1..]),
        Some("closure") => cs_inspect::catalog::closure_command(&args[1..]),
        Some("campaign") => cs_inspect::campaign::campaign_command(&args[1..]),
        Some("routes") => cs_inspect::routes::routes_command(&args[1..]),
        Some("handling") => cs_inspect::handling::handling_command(&args[1..]),
        Some(command) => {
            eprintln!(
                "cs-inspect: unsupported command {command:?}; this workspace stage implements \
                 only `inventory`, `audit`, `resolve`, `rof`, `zbd-audit`, `interp`, \
                 `texture-audit`, `config`, `scripts`, `catalog`, `closure`, `campaign`, \
                 `routes` and `handling`"
            );
            ExitCode::from(EXIT_INVALID_INPUT)
        }
    }
}
