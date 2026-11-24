//! Orchestration spanning more than one subsystem, and the only layer permitted to compose them —
//! subsystem modules stay independent of each other so each remains mockable on its own
//! (`docs/features/lib-decomposition/02_dev.md`). A service never depends on `commands/`.

pub mod github_access;
pub mod inference_reach;
pub mod panes;
pub mod proxy;
pub mod resources;
pub mod routing;
pub mod sandbox;
pub mod ssh_access;
pub mod toolchain_secrets;
