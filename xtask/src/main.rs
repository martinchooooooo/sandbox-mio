//! The repository build tool, run as `cargo xtask <subcommand>` from the repository root (the alias
//! lives in `.cargo/config.toml`). Std-only on purpose: no dependency to fetch before the first run.

use std::env;

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

fn main() {
    #[cfg(windows)]
    win_job::setup();

    let args: Vec<String> = env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("dev") if args.get(2).map(String::as_str) == Some("--setup") => {
            exec::run(paths::root_dir(), "pnpm", &["install"]);
            binaries::fetch_bundled_binaries();
        }
        // The entry point for a fresh worktree, ahead of every other arm: nothing else — `dev`,
        // `e2e`, `package` — can run until this has staged the gitignored assets.
        Some("dev") if args.get(2).map(String::as_str) == Some("--setup-worktree") => {
            worktree::setup_worktree(args.get(3).cloned());
        }
        Some("dev") => dev::dev(),
        Some("e2e") if args.get(2).map(String::as_str) == Some("--setup") => {
            e2e::setup_e2e();
        }
        // Any further `e2e --<flag>` arm belongs above this one, which forwards its second argument
        // straight into `pnpm run test:{suite}` — an arm added below would run `pnpm run test:--<flag>`.
        Some("e2e") => {
            e2e::run_e2e_suite(args.get(2).map(String::as_str).unwrap_or("standard"));
        }
        Some("docs") if matches!(args.get(2).map(String::as_str), None | Some("check")) => {
            std::process::exit(docs_check::run(&paths::root_dir(), false));
        }
        Some("docs") if args.get(2).map(String::as_str) == Some("--fix") => {
            std::process::exit(docs_check::run(&paths::root_dir(), true));
        }
        Some("ci")
            if matches!(
                args.get(2).map(String::as_str),
                None | Some("checks") | Some("e2e-light") | Some("diagnostics")
            ) =>
        {
            match args.get(2).map(String::as_str) {
                Some("checks") => ci::ci_checks(),
                Some("e2e-light") => ci::ci_e2e_light(),
                Some("diagnostics") => ci::collect_e2e_diagnostics(),
                _ => {
                    ci::ci_checks();
                    ci::ci_e2e_light();
                }
            }
        }
        Some("package") => package::package(),
        _ => {
            eprintln!("Usage:");
            eprintln!("  cargo xtask dev --setup-worktree [<path>]");
            eprintln!("                     - first command to run in a fresh worktree: stage tools/ +");
            eprintln!("                       src-tauri/binaries/ (hard-linked from a sibling worktree when");
            eprintln!("                       possible) and run `pnpm install` into <path>, defaulting to");
            eprintln!("                       the invoking worktree");
            eprintln!("  cargo xtask dev    - start the Vite dev server, build, and launch the app directly");
            eprintln!("                       (not via `tauri dev`/`cargo run` — see `run_dev_directly`'s doc");
            eprintln!("                       comment. Rerun after a Rust change; no auto-relaunch)");
            eprintln!("  cargo xtask dev --setup");
            eprintln!("                     - install app dependencies and stage the Mutagen sidecar + llama.cpp engine + Podman");
            eprintln!("  cargo xtask e2e    - run `pnpm test` in the e2e folder, first re-staging");
            eprintln!("                       tools/msedgedriver.exe if it no longer matches WebView2 and");
            eprintln!("                       appending ssh-keygen's folder to PATH if it is missing");
            eprintln!("  cargo xtask e2e --setup");
            eprintln!("                     - install e2e test dependencies; on CI, first installs the");
            eprintln!("                       WebView2 runtime if it is missing");
            eprintln!("  cargo xtask ci     - run the CI sequence locally: `ci checks`, then `ci e2e-light`");
            eprintln!("  cargo xtask ci checks");
            eprintln!("                     - docs check, pnpm install/build/check, paseo-plugins typecheck, then");
            eprintln!("                       cargo check + cargo test with the bundled-binary paths dropped");
            eprintln!("  cargo xtask ci e2e-light");
            eprintln!("                     - stage the real bundled binaries, `e2e --setup`, then the light tier");
            eprintln!("  cargo xtask ci diagnostics");
            eprintln!("                     - collect the light run's failure log, page dumps and mio.log files");
            eprintln!("                       into e2e-diagnostics/ (CI uploads it when e2e-light fails)");
            eprintln!("  cargo xtask package");
            eprintln!("                     - stage bundled binaries and build the release NSIS installer");
            eprintln!("  cargo xtask docs   - check docs/ against the documentation rules in xtask/src/docs_check.rs");
            eprintln!("  cargo xtask docs --fix");
            eprintln!("                     - regenerate every docs/ index.md from frontmatter, then check");
            std::process::exit(1);
        }
    }
}
