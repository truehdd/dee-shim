//! Modules targeting dee_ddpjoc_encoder (DD+ with JOC: spatial coder, backbone render, OAMD).

mod features;

use crate::site::Resolver;
use dshim_profiles::{hex, Kind};
use std::io::Write;

pub fn run_all(res: &Resolver, log: &mut dyn Write) {
    // DDPJOC_RATE_GATE_OFF=1 removes the Atmos data-rate policy gate. On an Online-Media build
    // this does not unlock rates above 1024: the rate-config map rejects them first, and
    // streaming is a single substream capped at 1024 by frmsiz. It is kept for builds with a
    // Blu-ray mode, so the gate is one lever rather than a special case.
    if crate::levers().ddpjoc.rate_gate_off {
        ldebug!(
            log,
            "ddpjoc",
            "removing Atmos data-rate policy gate (no effect on an Online-only build)"
        );
        apply_patch(res, "ddpjoc.rate.online_max", log);
    }
    features::install(res, log);
    super::timing::install_ddpjoc(res, log);
}

/// Write a Patch site's `new` bytes at its resolved, byte-checked address.
fn apply_patch(res: &Resolver, name: &str, log: &mut dyn Write) {
    let Some(r) = res.site(name, log) else {
        return;
    };
    if let Kind::Patch { new } = r.kind {
        if crate::write_code(r.addr, &hex(new)) {
            lok!(log, "ddpjoc", "patch {name} @ {:#x} ({new})", r.addr);
        } else {
            lerr!(log, "ddpjoc", "patch {name} write failed");
        }
    }
}
