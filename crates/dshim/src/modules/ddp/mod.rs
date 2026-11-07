//! Modules targeting dee_ddp_encoder (DD+ core, no JOC).

pub mod core_type;
pub mod p4;
pub mod phase90;
pub mod rates;

use crate::site::Resolver;
use std::io::Write;

pub fn run_all(res: &Resolver, log: &mut dyn Write) {
    p4::install(res, log);
    rates::install(res, log);
    phase90::install(res, log);
    core_type::install(res, log);
}
