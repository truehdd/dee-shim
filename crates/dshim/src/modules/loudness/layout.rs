//! LOUDNESS_LAYOUT: measure a channel layout the meter library has but the tool refuses.
//!
//! The tool's front end stands before the library in three places: the input check, which takes
//! 1, 2 or 6 channels; the meter setup, which turns the count into the library's channel
//! configuration; and the result, which turns it into a type. 7.1 and the layouts above it stop
//! only at the first. The layouts without LFE have no count in the other two, so the setup gets
//! the count with its configuration, and the result reports them as 5.1.
//!
//! The setup and the result hold their counts as a mask with a table by count (macOS) or as a
//! jump table by count (Linux, Windows).

use super::super::common::require_all;
use crate::site::{Resolved, Resolver};
use crate::{mem_read, write_code};
use dshim_profiles::{hex, Kind, Os};
use std::io::Write;

/// The counts both masks hold: 1, 2, 6, 8, 10, 12 and 16, bit count-1.
const COUNTS: u32 = 0x8AA3;
/// The result type of 5.1.
const TYPE_5_1: u32 = 2;

/// Write the imm32 that ends a patch site's instruction.
fn imm(site: &Resolved, value: u32) -> bool {
    let Kind::Patch { new } = site.kind else {
        return false;
    };
    write_code(site.addr + hex(new).len() - 4, &value.to_le_bytes())
}

/// The address of a table's entry for a channel count; the tables start at one channel.
fn entry(table: &Resolved, count: usize) -> usize {
    table.addr + 4 * (count - 1)
}

/// Send a jump table's `count` where `like` goes.
fn same_case(table: &Resolved, count: usize, like: usize) -> bool {
    write_code(entry(table, count), &mem_read(entry(table, like), 4))
}

pub fn install(res: &Resolver, log: &mut dyn Write) {
    let Some(layout) = crate::levers().loudness.layout else {
        return;
    };
    let count = layout.channels() as usize;
    let masks = res.profile.os == Os::Macos;

    let mut names = vec!["loudness.input.counts"];
    if layout.config().is_some() {
        names.extend(if masks {
            &[
                "loudness.meter.counts",
                "loudness.meter.configs",
                "loudness.result.counts",
                "loudness.result.types",
            ][..]
        } else {
            &[
                "loudness.meter.cases",
                "loudness.meter.config",
                "loudness.result.cases",
            ][..]
        });
    }
    let sites = require_all(res, "loudness", &names, log);

    // The input list 1, 2, 6 becomes 1, 2 and this count.
    let mut written = imm(&sites[0], count as u32);
    if let Some(config) = layout.config() {
        let config = config as u32;
        written &= if masks {
            let counts = COUNTS | 1 << (count - 1);
            imm(&sites[1], counts)
                && write_code(entry(&sites[2], count), &config.to_le_bytes())
                && imm(&sites[3], counts)
                && write_code(entry(&sites[4], count), &TYPE_5_1.to_le_bytes())
        } else {
            // The case for 16 channels is a mov of its configuration: this count goes there,
            // and that case gets this configuration.
            same_case(&sites[1], count, 16)
                && imm(&sites[2], config)
                && same_case(&sites[3], count, 6)
        };
    }
    if !written {
        lerr!(log, "loudness", "could not open the layout, stopping");
        let _ = log.flush();
        std::process::exit(3);
    }
    lok!(
        log,
        "loudness",
        "layout {} ({count} channels)",
        layout.name()
    );
}
