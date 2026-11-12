//! Modules targeting dee_dthd_encoder (Dolby TrueHD / MLP). The macOS 6.7 x86_64 code also runs
//! in the Linux ELF port of that tool; the Windows PE build has its own profile.

use crate::site::Resolver;
use std::io::Write;

pub mod audio;
pub mod compose;
pub mod drc;
pub mod highres;
pub mod layout;
pub mod mll;
pub mod overlap;
pub mod passthrough;
pub mod presentations;
pub mod rate;
pub mod smoothing;
pub mod timing;

/// In this order: keep quiet reads the element blocks the overlap module hooks. highres, mll,
/// compose and rate are independent (an authoring gate, the full job redirect, the
/// capture-and-mutate composer, and the input rates a target opens).
pub fn run_all(res: &Resolver, log: &mut dyn Write) {
    smoothing::install(res, log);
    let keep_quiet = passthrough::install(res, log);
    audio::install(res, keep_quiet, log);
    drc::install(res, log);
    highres::install(res, log);
    mll::install(res, log);
    compose::install(res, log);
    rate::install(res, log);
    presentations::install(res, log);
    timing::install(res, log);
}
