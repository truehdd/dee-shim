//! Loudness meter 6.7.0, Linux x86_64.

use crate::{Ident, Kind, Os, Profile, Site};

pub static PROFILE: Profile = Profile {
    tool: "dee_measure_loudness",
    os: Os::Linux,
    version: "6.7.0",
    ident: Ident::GnuBuildId("aff621c10b3420c35cd53481331a9f2f3f90d018"),
    sites: SITES,
};

static SITES: &[Site] = &[
    Site {
        name: "loudness.input.counts",
        va: 0x50B359,
        kind: Kind::Patch {
            new: "c7 44 24 08 08 00 00 00",
        },
        expect: "c7 44 24 08 06 00 00 00",
        note: "Builds the input check's list of channel counts 1, 2, 6: the store of the 6.",
    },
    Site {
        name: "loudness.meter.cases",
        va: 0x1149C08,
        kind: Kind::Data,
        expect: "8c 27 3d ff dc 27 3d ff 98 98 2d ff 98 98 2d ff",
        note: "Meter setup: jump table by channel count from 1, offsets from 4 bytes before.",
    },
    Site {
        name: "loudness.meter.config",
        va: 0x51C060,
        kind: Kind::Patch {
            new: "b8 04 00 00 00",
        },
        expect: "b8 0b 00 00 00",
        note: "Meter setup: the case for 16 channels, a mov of its channel configuration.",
    },
    Site {
        name: "loudness.result.cases",
        va: 0x11462D8,
        kind: Kind::Data,
        expect: "4c fe 38 ff 9c fe 38 ff 77 70 2d ff 77 70 2d ff",
        note: "Result type: jump table by channel count from 1, offsets from 4 bytes before.",
    },
];
