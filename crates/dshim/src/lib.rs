//! dshim-engine: the DEE shim engine. The in-process host in `dee_shim` maps the tool and calls
//! `init_hosted` with the levers the CLI checked; the engine identifies which build it holds,
//! picks the matching site profile, and runs each module whose levers ask for it. Modules name
//! sites; the profile maps names to addresses and checks the bytes. Nothing on disk is modified.
//!
//! Layers, bottom up: `platform` (per-OS code writes and the hosted image), `detour` (x86_64
//! inline detour), `site` (profile match and byte-checked resolve), `modules` (the controls).

mod detour;
#[macro_use]
mod logfmt;
pub mod modules;
mod platform;
mod site;

use dshim_levers::Levers;
use site::Resolver;
use std::io::Write;
use std::sync::OnceLock;

static LEVERS: OnceLock<Levers> = OnceLock::new();

/// The levers of this run, set once by `init_hosted` before any module runs.
pub(crate) fn levers() -> &'static Levers {
    LEVERS.get_or_init(Levers::default)
}

/// The DEE major.minor selectors compiled into this engine, e.g. `6.7`.
pub const DEE_VERSIONS: &str = env!("DSHIM_DEE_VERSIONS");

/// Run the engine against the hosted tool image, once.
fn init() {
    if let Err(error) = dshim_log::init(
        &dshim_log::filter(),
        &dshim_log::output(),
        &dshim_log::format(),
    ) {
        eprintln!("dshim: {error}");
        std::process::exit(3);
    }
    let mut log = std::io::sink();
    ldebug!(
        &mut log,
        "",
        "loaded, built {}; DEE versions: {}",
        env!("CARGO_PKG_VERSION"),
        env!("DSHIM_DEE_VERSIONS")
    );
    let res = match Resolver::new(&mut log) {
        Some(r) => r,
        None => {
            if !levers().is_empty() {
                lerr!(
                    &mut log,
                    "",
                    "a lever is set but this binary has no shim profile"
                );
                let _ = log.flush();
                std::process::exit(3);
            }
            log::debug!(target: "dshim", "no matching profile; no controls applied");
            return;
        }
    };
    modules::run_all(&res, &mut log);
    if res.failed_site() && !levers().is_empty() {
        lerr!(
            &mut log,
            "",
            "requested shim feature has a stale profile site; stopping"
        );
        let _ = log.flush();
        std::process::exit(3);
    }
    log::info!(target: "dshim", "encoder controls loaded");
    dshim_log::flush();
}

/// Run the engine for an in-process host. `image` is the tool's file on disk and `slide` the
/// difference between where the host mapped it and its link-time addresses. The host calls this
/// after the image is mapped and linked, and before its entry point runs. The engine reads the
/// identity from `image` and matches profiles against it instead of scanning the process.
/// `levers` says what to change; with none set the tool runs as shipped.
pub fn init_hosted(image: &std::path::Path, slide: i64, levers: Levers) {
    let _ = LEVERS.set(levers);
    match platform::Hosted::new(image, slide) {
        Ok(hosted) => platform::set_hosted(hosted),
        Err(error) => {
            eprintln!("dshim: hosted image {}: {error}", image.display());
            std::process::exit(3);
        }
    }
    init();
}

// Re-exports for modules. Which helpers are used depends on the enabled tool features, so allow
// some to be unused in a narrow build (e.g. ddp-only does not need the trampoline allocator).
#[allow(unused_imports)]
pub(crate) use detour::install as detour_install;
#[allow(unused_imports)]
pub(crate) use platform::{alloc_rwx, flush_icache, read as mem_read, write_code};
