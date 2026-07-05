//! Suppresses the console window Windows otherwise pops up whenever this (GUI-subsystem, no
//! console of its own) app spawns a console subprocess like `podman`, `wsl`, `ssh`, or `winget`.
//! Every raw `Command::new` in the crate should be piped through [`no_window`] immediately after
//! construction.

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub(crate) trait NoWindow {
    fn no_window(&mut self) -> &mut Self;
}

impl NoWindow for std::process::Command {
    fn no_window(&mut self) -> &mut Self {
        std::os::windows::process::CommandExt::creation_flags(self, CREATE_NO_WINDOW);
        self
    }
}

impl NoWindow for tokio::process::Command {
    fn no_window(&mut self) -> &mut Self {
        self.creation_flags(CREATE_NO_WINDOW);
        self
    }
}
