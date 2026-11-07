//! TrueHD cluster pass-through switches.

use crate::modules::cluster::{self, Options};
use crate::site::Resolver;
use std::io::Write;

pub fn install(res: &Resolver, log: &mut dyn Write) -> bool {
    let oamd = &crate::levers().dthd.oamd;
    let options = Options {
        own_clusters: oamd.own_clusters(),
        exact_timing: oamd.exact_timing(),
        keep_quiet: oamd.keep_quiet(),
        own_gains: oamd.own_gains(),
    };
    cluster::install(
        res,
        "dthd",
        options,
        Some(super::overlap::element_live),
        log,
    )
}
