//! Object metadata smoothing switches for dee_dthd_encoder.
//!
//! Levers:
//!   DTHD_OAMD_EXACT_POSITIONS=on
//!                     code the DAMF object positions as authored (default off: a 25 ms one-pole
//!                     per 512 samples, the same sampler as ddpjoc's).
//!   DTHD_OAMD_EXACT_SIZES=on
//!                     the same for object size (a 20 ms one-pole).
//!   DTHD_OAMD_AS_IS=on
//!                     both, and the cluster pass-through (passthrough.rs); a switch set on its
//!                     own wins.

use crate::modules::common::require_all;
use crate::site::Resolver;
use crate::write_code;
use dshim_profiles::{hex, Kind};
use std::io::Write;

pub fn install(res: &Resolver, log: &mut dyn Write) {
    let oamd = &crate::levers().dthd.oamd;
    let pos = oamd.exact_positions();
    let siz = oamd.exact_sizes();
    let mut names = Vec::new();
    if pos {
        names.push("dthd.oa.sampler_position_flag");
    }
    if siz {
        names.push("dthd.oa.sampler_size_flag");
    }
    if names.is_empty() {
        return;
    }
    let patch = |name: &str, r: &crate::site::Resolved, log: &mut dyn Write| {
        let Kind::Patch { new } = r.kind else { return };
        if !write_code(r.addr, &hex(new)) {
            lerr!(log, "dthd", "{name} write failed, stopping");
            let _ = log.flush();
            std::process::exit(3);
        }
        lok!(
            log,
            "dthd",
            "exact metadata: patch {name} @ {:#x} ({new})",
            r.addr
        );
    };
    // Resolve and byte-check every site before writing any.
    let sites = require_all(res, "dthd exact metadata", &names, log);
    for (name, r) in names.iter().zip(&sites) {
        patch(name, r, log);
    }
}
