//! DDPDEC_LAYOUT: write every channel the stream codes.
//!
//! The tool's front end takes 5.1, stereo or mono, but the decoder library behind it decodes
//! eight channels and the frame loop and the writer take any count. Run as 5.1, two places
//! hold the output to six channels: the decoder's output mode, which the setup reads from a map
//! by layout, and the frame loop, which copies the first six channels of a longer frame. The
//! map's entry becomes "as coded", and the frame loop's test never matches.

use super::super::common::require_all;
use crate::site::Resolver;
use crate::write_code;
use dshim_profiles::{hex, Kind};
use std::io::Write;

pub fn install(res: &Resolver, log: &mut dyn Write) {
    if crate::levers().ddpdec.layout.is_none() {
        return;
    }
    let names = ["ddpdec.layout.outmode", "ddpdec.layout.copy"];
    let sites = require_all(res, "ddpdec", &names, log);
    let written = sites.iter().all(|site| match site.kind {
        Kind::Patch { new } => write_code(site.addr, &hex(new)),
        _ => false,
    });
    if !written {
        lerr!(log, "ddpdec", "could not open the layout, stopping");
        let _ = log.flush();
        std::process::exit(3);
    }
    lok!(
        log,
        "ddpdec",
        "layout native: every coded channel is written"
    );
}
