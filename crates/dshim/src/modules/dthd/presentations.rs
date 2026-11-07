//! Independent presentation count for dee_dthd_encoder.
//!
//! The CLI refuses a job with more than two independent presentations before it composes the
//! .mll ("Only up to 2 independent presentations are allowed"). That is the front-end's own
//! policy: the engine has no such cap, and a stream with an independent two-, six- and
//! eight-channel tier encodes and verifies. The refusal is lifted whenever the shim runs dthd, so
//! the engine judges the job.
//!
//! Site: dthd.cli.independent_cap, the count check's branch, made a fall-through.

use crate::site::Resolver;
use crate::write_code;
use dshim_profiles::{hex, Kind};
use std::io::Write;

pub fn install(res: &Resolver, log: &mut dyn Write) {
    let Some(r) = res.site("dthd.cli.independent_cap", log) else {
        return;
    };
    let Kind::Patch { new } = r.kind else {
        return;
    };
    if write_code(r.addr, &hex(new)) {
        lok!(
            log,
            "presentations",
            "independent presentation cap: patch @ {:#x}",
            r.addr
        );
    } else {
        lerr!(
            log,
            "presentations",
            "independent presentation cap: write failed"
        );
    }
}
