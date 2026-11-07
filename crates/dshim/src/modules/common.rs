//! Helpers shared by modules.

use crate::site::{Resolved, Resolver};
use std::io::Write;

/// Resolve and byte-check every site of a coupled-lever feature before anything is written. If
/// any site is missing or its bytes differ, log which and end the process: a requested feature is
/// applied in full or the run does not happen.
pub fn require_all(
    res: &Resolver,
    feature: &str,
    names: &[&str],
    log: &mut dyn Write,
) -> Vec<Resolved> {
    let mut out = Vec::with_capacity(names.len());
    for n in names {
        match res.site(n, log) {
            Some(r) => out.push(r),
            None => {
                lerr!(
                    log,
                    feature,
                    "site {n} unavailable, nothing applied, stopping"
                );
                let _ = log.flush();
                std::process::exit(3);
            }
        }
    }
    out
}
