//! Loudness meter 6.7.0, macOS x86_64.

use crate::{Ident, Kind, Os, Profile, Site};

pub static PROFILE: Profile = Profile {
    tool: "dee_measure_loudness",
    os: Os::Macos,
    version: "6.7.0",
    ident: Ident::MachoUuid("19E96BBC-08AB-3501-85E0-666E553761A1"),
    sites: SITES,
};

static SITES: &[Site] = &[
    Site {
        name: "loudness.input.counts",
        va: 0x1000D7219,
        kind: Kind::Patch {
            new: "c7 40 08 08 00 00 00",
        },
        expect: "c7 40 08 06 00 00 00",
        note: "Builds the input check's list of channel counts 1, 2, 6: the store of the 6.",
    },
    Site {
        name: "loudness.meter.counts",
        va: 0x1000DBB52,
        kind: Kind::Patch {
            new: "ba bf 8a 00 00",
        },
        expect: "ba a3 8a 00 00",
        note: "Meter setup: mask of the channel counts, bit count-1.",
    },
    Site {
        name: "loudness.meter.configs",
        va: 0x1008C38B8,
        kind: Kind::Data,
        expect: "00 00 00 00 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 07 00 00 00",
        note: "Meter setup: channel configuration by channel count from 1, 16 ints.",
    },
    Site {
        name: "loudness.result.counts",
        va: 0x10007BC87,
        kind: Kind::Patch {
            new: "b9 bf 8a 00 00",
        },
        expect: "b9 a3 8a 00 00",
        note: "Result type: mask of the channel counts, bit count-1.",
    },
    Site {
        name: "loudness.result.types",
        va: 0x1008C1588,
        kind: Kind::Data,
        expect: "00 00 00 00 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 02 00 00 00",
        note: "Result type by channel count from 1, 16 ints.",
    },
];
