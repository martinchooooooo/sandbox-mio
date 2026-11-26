//! Child-process helpers: every external command the tool runs is spawned here, so it joins
//! `win_job`'s kill-on-close job.

use std::{ffi::OsStr, path::PathBuf, process::Command};

/// The command every helper spawns. On Windows it goes through `cmd /C` so `.cmd` shims such as
/// `pnpm` resolve the way they do in a terminal.
fn base_command(cmd: &str, args: &[&str]) -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(cmd).args(args);
        c
    } else {
        let mut c = Command::new(cmd);
        c.args(args);
        c
    }
}

/// `run` without the panic: returns whether the command both launched and exited successfully, for
/// callers that have a printable fallback.
pub(crate) fn try_run(dir: PathBuf, cmd: &str, args: &[&str]) -> bool {
    let mut command = base_command(cmd, args);
    let Ok(mut child) = command.current_dir(dir).spawn() else {
        return false;
    };
    #[cfg(windows)]
    crate::win_job::assign(&child);
    child.wait().map(|s| s.success()).unwrap_or(false)
}

pub(crate) fn run(dir: PathBuf, cmd: &str, args: &[&str]) {
    run_env(dir, cmd, args, &[]);
}

/// [`run`] with extra environment variables set on the child.
pub(crate) fn run_env(dir: PathBuf, cmd: &str, args: &[&str], envs: &[(&str, &OsStr)]) {
    let mut command = base_command(cmd, args);
    let mut child = command
        .current_dir(dir)
        .envs(envs.iter().copied())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to run `{}`: {}", cmd, e));
    #[cfg(windows)]
    crate::win_job::assign(&child);
    let status = child
        .wait()
        .unwrap_or_else(|e| panic!("failed to wait for `{}`: {}", cmd, e));
    assert!(status.success(), "`{}` exited with {}", cmd, status);
}

/// `run`'s non-blocking counterpart: spawns `cmd`, joins it to `win_job`'s job the same way, but
/// returns the [`std::process::Child`] immediately instead of waiting — for a long-running sidecar
/// like the Vite dev server that has to keep running alongside whatever `dev` does next.
pub(crate) fn spawn_background(dir: PathBuf, cmd: &str, args: &[&str]) -> std::process::Child {
    let mut command = base_command(cmd, args);
    let child = command
        .current_dir(dir)
        .spawn()
        .unwrap_or_else(|e| panic!("failed to run `{}`: {}", cmd, e));
    #[cfg(windows)]
    crate::win_job::assign(&child);
    child
}
