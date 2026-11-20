//! DD+ decoder 6.7.0, Windows x86_64.

use crate::{Ident, Kind, Os, Profile, Site};

pub static PROFILE: Profile = Profile {
    tool: "dee_ddp_decoder",
    os: Os::Windows,
    version: "6.7.0",
    ident: Ident::PeModuleId("6AE0E871-006E0000"),
    sites: SITES,
};

static SITES: &[Site] = &[
    Site {
        name: "ddpdec.layout.outmode",
        va: 0x1404A8A80,
        kind: Kind::Patch {
            new: "02 00 00 00 02 00 00 00 01 00 00 00 ff ff ff ff",
        },
        expect: "02 00 00 00 02 00 00 00 01 00 00 00 07 00 00 00",
        note: "Decoder setup, the map from the output layout to the decoder's output mode: the entry for 5.1, mode 7 (3/2).",
    },
    Site {
        name: "ddpdec.layout.copy",
        va: 0x140160CE1,
        kind: Kind::Patch {
            new: "83 7f 18 7f",
        },
        expect: "83 7f 18 01",
        note: "Frame loop: the test for the 5.1 layout, which sends a frame of another channel count through the copy of its first six channels.",
    },
];
