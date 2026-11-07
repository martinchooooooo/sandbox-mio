//! Orchestration spanning more than one subsystem, and the only layer permitted to compose them —
//! subsystem modules stay independent of each other so each remains mockable on its own
//! (`docs/features/lib-decomposition/02_dev.md`). A service never depends on `commands/`.

pub mod panes;
pub mod routing;
pub mod sandbox;
pub mod start_env;
pub mod toolchain_secrets;
