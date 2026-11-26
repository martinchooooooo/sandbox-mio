//! The repository build tool, run as `cargo xtask <subcommand>` from the repository root (the alias
//! lives in `.cargo/config.toml`). Dependencies are kept to the few that replace hand-rolled
//! parsing and hashing (`xtask/Cargo.toml`); there is no CLI framework.

mod binaries;
mod ci;
mod dev;
mod docs_check;
mod e2e;
mod exec;
mod package;
mod paths;
mod worktree;
#[cfg(windows)]
mod win_job;

/// A fully parsed command line. Arity is strict: [`parse`] yields `None` for any argument list that
/// is not exactly one of these shapes, so a stray flag is never forwarded or silently dropped.
enum Cli<'a> {
    Help,
    DevSetup,
    DevSetupWorktree(Option<&'a str>),
    Dev,
    E2eSetup,
    E2e(&'a str),
    Docs { fix: bool },
    CiAll,
    CiChecks,
    CiE2eLight,
    CiDiagnostics,
    Package,
}

fn parse<'a>(args: &[&'a str]) -> Option<Cli<'a>> {
    Some(match args {
        ["--help" | "-h" | "help"] => Cli::Help,
        ["dev"] => Cli::Dev,
        ["dev", "--setup"] => Cli::DevSetup,
        ["dev", "--setup-worktree"] => Cli::DevSetupWorktree(None),
        ["dev", "--setup-worktree", path] => Cli::DevSetupWorktree(Some(path)),
        ["e2e"] => Cli::E2e("standard"),
        ["e2e", "--setup"] => Cli::E2eSetup,
        // The tier is forwarded straight into `pnpm run test:{tier}`, so a flag must never reach it.
        ["e2e", tier] if !tier.starts_with('-') => Cli::E2e(tier),
        ["docs"] | ["docs", "check"] => Cli::Docs { fix: false },
        ["docs", "--fix"] => Cli::Docs { fix: true },
        ["ci"] => Cli::CiAll,
        ["ci", "checks"] => Cli::CiChecks,
        ["ci", "e2e-light"] => Cli::CiE2eLight,
        ["ci", "diagnostics"] => Cli::CiDiagnostics,
        ["package"] => Cli::Package,
        _ => return None,
    })
}

const USAGE: &[&str] = &[
    "Usage:",
    "  cargo xtask dev --setup-worktree [<path>]",
    "                     - first command to run in a fresh worktree: stage tools/ +",
    "                       src-tauri/binaries/ (hard-linked from a sibling worktree when",
    "                       possible) and run `pnpm install` into <path>, defaulting to",
    "                       the invoking worktree",
    "  cargo xtask dev    - start the Vite dev server, build, and launch the app directly",
    "                       (not via `tauri dev`/`cargo run` — see `run_dev_directly`'s doc",
    "                       comment. Rerun after a Rust change; no auto-relaunch)",
    "  cargo xtask dev --setup",
    "                     - install app dependencies and stage the Mutagen sidecar + llama.cpp engine + Podman",
    "  cargo xtask e2e [<tier>]",
    "                     - run `pnpm test:<tier>` (default `standard`) in the e2e folder, first",
    "                       re-staging tools/msedgedriver.exe if it no longer matches WebView2 and",
    "                       appending ssh-keygen's folder to PATH if it is missing",
    "  cargo xtask e2e --setup",
    "                     - install e2e test dependencies; on CI, first installs the",
    "                       WebView2 runtime if it is missing",
    "  cargo xtask ci     - run the CI sequence locally: `ci checks`, then `ci e2e-light`",
    "  cargo xtask ci checks",
    "                     - docs check, pnpm install/build/check, paseo-plugins typecheck, then",
    "                       cargo check + cargo test with the bundled-binary paths dropped",
    "  cargo xtask ci e2e-light",
    "                     - stage the real bundled binaries, `e2e --setup`, then the light tier",
    "  cargo xtask ci diagnostics",
    "                     - collect the light run's failure log, page dumps and mio.log files",
    "                       into e2e-diagnostics/ (CI uploads it when e2e-light fails)",
    "  cargo xtask package",
    "                     - stage bundled binaries and build the release NSIS installer",
    "  cargo xtask docs   - check docs/ against the documentation rules in xtask/src/docs_check.rs",
    "  cargo xtask docs --fix",
    "                     - regenerate every docs/ index.md from frontmatter, then check",
    "  cargo xtask --help - print this text",
];

fn main() {
    #[cfg(windows)]
    win_job::setup();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    let Some(cli) = parse(&args) else {
        for line in USAGE {
            eprintln!("{line}");
        }
        std::process::exit(1);
    };

    match cli {
        Cli::Help => {
            for line in USAGE {
                println!("{line}");
            }
        }
        Cli::DevSetup => {
            exec::run(paths::root_dir(), "pnpm", &["install"]);
            binaries::fetch_bundled_binaries();
        }
        // The entry point for a fresh worktree: nothing else — `dev`, `e2e`, `package` — can run
        // until this has staged the gitignored assets.
        Cli::DevSetupWorktree(path) => worktree::setup_worktree(path.map(String::from)),
        Cli::Dev => dev::dev(),
        Cli::E2eSetup => e2e::setup_e2e(),
        Cli::E2e(tier) => e2e::run_e2e_suite(tier),
        Cli::Docs { fix } => std::process::exit(docs_check::run(&paths::root_dir(), fix)),
        Cli::CiAll => {
            ci::ci_checks();
            ci::ci_e2e_light();
        }
        Cli::CiChecks => ci::ci_checks(),
        Cli::CiE2eLight => ci::ci_e2e_light(),
        Cli::CiDiagnostics => ci::collect_e2e_diagnostics(),
        Cli::Package => package::package(),
    }
}
