//! Modules targeting dee_measure_loudness.

pub mod layout;

use crate::site::Resolver;
use std::io::Write;

pub fn run_all(res: &Resolver, log: &mut dyn Write) {
    layout::install(res, log);
}
