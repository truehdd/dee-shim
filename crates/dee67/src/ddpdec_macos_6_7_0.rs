//! DD+ decoder 6.7.0, macOS x86_64.

use crate::{Ident, Kind, Os, Profile, Site};

pub static PROFILE: Profile = Profile {
    tool: "dee_ddp_decoder",
    os: Os::Macos,
    version: "6.7.0",
    ident: Ident::MachoUuid("95ED794C-9E56-3727-AA20-B8552356A544"),
    sites: SITES,
};

static SITES: &[Site] = &[
    Site {
        name: "ddpdec.layout.outmode",
        va: 0x10005B578,
        kind: Kind::Patch {
            new: "48 b9 01 00 00 00 ff ff ff ff",
        },
        expect: "48 b9 01 00 00 00 07 00 00 00",
        note: "Decoder setup, the map from the output layout to the decoder's output mode: the entry for 5.1, mode 7 (3/2).",
    },
    Site {
        name: "ddpdec.layout.copy",
        va: 0x10005C58B,
        kind: Kind::Patch {
            new: "83 7b 18 7f",
        },
        expect: "83 7b 18 01",
        note: "Frame loop: the test for the 5.1 layout, which sends a frame of another channel count through the copy of its first six channels.",
    },
];
