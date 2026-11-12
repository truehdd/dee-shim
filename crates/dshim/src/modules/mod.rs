//! Modules: the controls, grouped by the tool they target. Each tool is a submodule (`ddp`,
//! `ddpjoc`, `dthd`); within it, one file per capability. A module is `fn install(res, log)`,
//! gated on its levers (`crate::levers()`), that asks the resolver for sites by name. A site
//! missing from the matched profile makes that capability skip with a logged note, never a
//! crash, so running every tool's modules against whatever binary is hosted is safe.
//!
//! Helpers shared across modules live in `common`.

#[cfg(any(feature = "dthd", feature = "ddpjoc"))]
mod cluster;
pub mod common;
#[cfg(any(feature = "dthd", feature = "ddpjoc"))]
mod timing;

// Each tool's module compiles only when its cargo feature is on (all on by default). A build can
// drop a tool with `--no-default-features --features <tools>`.
#[cfg(feature = "ddp")]
pub mod ddp;
#[cfg(feature = "ddpjoc")]
pub mod ddpjoc;
#[cfg(feature = "dthd")]
pub mod dthd;

use crate::site::Resolver;
use std::io::Write;

/// Run every enabled tool's modules. The matched profile only has one tool's sites, so the other
/// tools' modules resolve nothing and are inert; this keeps the engine one binary for all tools.
pub fn run_all(res: &Resolver, log: &mut dyn Write) {
    #[cfg(feature = "ddp")]
    ddp::run_all(res, log);
    #[cfg(feature = "ddpjoc")]
    ddpjoc::run_all(res, log);
    #[cfg(feature = "dthd")]
    dthd::run_all(res, log);
    // Silence unused-param warnings when no tool feature is enabled.
    #[cfg(not(any(feature = "ddp", feature = "ddpjoc", feature = "dthd")))]
    {
        let _ = (res, log);
    }
}
