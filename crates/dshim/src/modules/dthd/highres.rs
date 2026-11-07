//! High-rate multichannel authoring gates for dee_dthd_encoder.
//!
//! DEE refuses to author a stream that is well formed but breaks a disc conformance rule: more
//! than 6 channels (BluRay) or 2 (HD DVD-Video) at 176.4 or 192 kHz. The three gates are exactly
//! those refusals, so an 8-channel 192 kHz FBA stream (which no disc format carries, but which a
//! conformance checker wants as a test vector) can be written. All move together: the
//! presentation checks refuse the job, and the bitstream verifier refuses the coded stream
//! (rules M265 and M284).
//!
//! Lever:
//!   DTHD_HIGHRES=on    allow more than the disc channel limit at 176.4/192 kHz (default off,
//!                      on with DTHD_TARGET=non-conformant-fba).
//!
//! Both sites are fixed byte patches in the profile, applied on the switch.

use crate::site::Resolver;
use crate::write_code;
use dshim_profiles::{hex, Kind};
use std::io::Write;

const GATES: [&str; 3] = [
    "highres.channel_limit",
    "highres.presentation_fs",
    "highres.verify_channels",
];

pub fn install(res: &Resolver, log: &mut dyn Write) {
    if !crate::levers().dthd.highres() {
        return;
    }
    for name in GATES {
        let Some(r) = res.site(name, log) else {
            lerr!(
                log,
                "highres",
                "{name} not in this profile, DTHD_HIGHRES needs every gate, stopping"
            );
            let _ = log.flush();
            std::process::exit(3);
        };
        let Kind::Patch { new } = r.kind else {
            lerr!(log, "highres", "{name} is not a patch site, stopping");
            let _ = log.flush();
            std::process::exit(3);
        };
        if !write_code(r.addr, &hex(new)) {
            lerr!(log, "highres", "{name} write failed, stopping");
            let _ = log.flush();
            std::process::exit(3);
        }
        lok!(
            log,
            "highres",
            "high-rate gate: patch {name} @ {:#x} ({new})",
            r.addr
        );
    }
}
