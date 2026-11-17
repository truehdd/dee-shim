//! Loudness meter 6.7.0, Windows x86_64.

use crate::{Ident, Kind, Os, Profile, Site};

pub static PROFILE: Profile = Profile {
    tool: "dee_measure_loudness",
    os: Os::Windows,
    version: "6.7.0",
    ident: Ident::PeModuleId("6AE0E86C-00D75000"),
    sites: SITES,
};

static SITES: &[Site] = &[
    Site {
        name: "loudness.input.counts",
        va: 0x140238E8B,
        kind: Kind::Patch {
            new: "c7 44 24 48 08 00 00 00",
        },
        expect: "c7 44 24 48 06 00 00 00",
        note: "Builds the input check's list of channel counts 1, 2, 6: the store of the 6.",
    },
    Site {
        name: "loudness.meter.cases",
        va: 0x140243B6C,
        kind: Kind::Data,
        expect: "4d 3a 24 00 67 3a 24 00 15 3b 24 00 15 3b 24 00",
        note: "Meter setup: jump table by channel count from 1, RVAs.",
    },
    Site {
        name: "loudness.meter.config",
        va: 0x140243AF8,
        kind: Kind::Patch {
            new: "b8 04 00 00 00",
        },
        expect: "b8 0b 00 00 00",
        note: "Meter setup: the case for 16 channels, a mov of its channel configuration.",
    },
    Site {
        name: "loudness.result.cases",
        va: 0x14021450C,
        kind: Kind::Data,
        expect: "1a 44 21 00 31 44 21 00 cd 44 21 00 cd 44 21 00",
        note: "Result type: jump table by channel count from 1, RVAs.",
    },
];
