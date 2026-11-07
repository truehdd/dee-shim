//! DD+ 6.7.0, Windows x86_64.

use crate::{Ident, Kind, Os, Profile, Site};

pub static PROFILE: Profile = Profile {
    tool: "dee_ddp_encoder",
    os: Os::Windows,
    version: "6.7.0",
    ident: Ident::PeModuleId("6AE0E857-01131000"),
    sites: SITES,
};

static SITES: &[Site] = &[
    Site {
        name: "ddp.core.surround_downmix_type",
        va: 0x14094DDF0,
        kind: Kind::Patch {
            new: "c7 41 18 01 00 00 00 c3 90",
        },
        expect: "83 fa 01 77 03 89 51 18 c3",
        note: "Surround downmix type setter for the 7.x to 5.x core; DDP_CORE_TYPE replaces it with a store of the selector at context+24 (0 PLIIx, 1 Standard).",
    },
    Site {
        name: "ddp.p4.evo_acmod",
        va: 0x1403B0FD0,
        kind: Kind::Hook { steal: 15 },
        expect: "4c 8b dc 49 89 5b 08 49 89 73 10 49 89 7b 18",
        note: "EVO metadata acmod chooser; channel count is argument 2.",
    },
    Site {
        name: "ddp.p4.chooser",
        va: 0x1403B68A0,
        kind: Kind::Hook { steal: 12 },
        expect: "48 89 5c 24 20 55 56 57 41 56 41 57",
        note: "Chooses acmod at output+0 and rate at output+8; output is the second argument.",
    },
    Site {
        name: "ddp.p4.set_params",
        va: 0x140916BA0,
        kind: Kind::Hook { steal: 15 },
        expect: "48 89 6c 24 20 41 54 41 56 41 57 48 83 ec 30",
        note: "Encoder parameter setter, called by the frontend configuration builder.",
    },
    Site {
        name: "ddp.p4.downmix_mode",
        va: 0x14034F480,
        kind: Kind::Hook { steal: 13 },
        expect: "48 8b c4 48 89 58 10 48 89 70 20 55 57",
        note: "Frontend downmix mode.",
    },
    Site {
        name: "ddp.p4.route_table",
        va: 0x140E32000,
        kind: Kind::Data,
        expect: "00 00 00 00 00 00 00 00 00 00 00 00 02 00 00 00",
        note: "Input routing table; 88-byte records.",
    },
    Site {
        name: "ddp.p4.chan_mask",
        va: 0x140E31260,
        kind: Kind::Data,
        expect: "03 00 00 00 04 00 00 00 03 00 00 00 0f 00 00 00",
        note: "Acmod-to-channel-enable mask.",
    },
    Site {
        name: "ddp.rate.outbuf_size",
        va: 0x1403B53BA,
        kind: Kind::Patch {
            new: "48 c7 86 18 01 00 00 00 40 00 00",
        },
        expect: "48 c7 86 18 01 00 00 00 1a 00 00",
        note: "Default output buffer size, 6656 bytes at frontend+0x118.",
    },
    Site {
        name: "ddp.rate.emdf_gate",
        va: 0x14091EBF0,
        kind: Kind::Patch { new: "eb 07" },
        expect: "7d 07",
        note: "Rate-index guard in the EVO metadata support check (error 217); DDP_EMDF_OFF makes the jge unconditional.",
    },
    Site {
        name: "ddp.p4.e137_set_params",
        va: 0x14095265C,
        kind: Kind::Patch {
            new: "90 90 90 90 90 90",
        },
        expect: "0f 8c 9e 00 00 00",
        note: "Low-acmod LFE guard in parameter validation.",
    },
    Site {
        name: "ddp.p4.e137_after_preproc",
        va: 0x140918AD8,
        kind: Kind::Patch { new: "eb 10" },
        expect: "77 10",
        note: "Low-acmod LFE guard after preprocessing.",
    },
    Site {
        name: "ddp.p4.e137_process",
        va: 0x140954B10,
        kind: Kind::Patch { new: "eb 0c" },
        expect: "74 0c",
        note: "Low-acmod LFE guard in process validation.",
    },
    Site {
        name: "ddp.p4.e137_param_apply",
        va: 0x140953263,
        kind: Kind::Patch {
            new: "90 90 90 90 90 90",
        },
        expect: "0f 8c 9e 00 00 00",
        note: "Low-acmod LFE guard in the parameter-apply loop.",
    },
    Site {
        name: "ddp.core.engine_check",
        va: 0x140918980,
        kind: Kind::Hook { steal: 15 },
        expect: "48 89 5c 24 18 48 89 74 24 20 57 48 83 ec 20",
        note: "Per-substream validation; coded rate is i16 at config+120.",
    },
    Site {
        name: "ddp.core.phase90_param_write",
        va: 0x14091C9F9,
        kind: Kind::Patch { new: "eb 29" },
        expect: "74 29",
        note: "Skip the first preprocessing Hilbert call.",
    },
    Site {
        name: "ddp.core.phase90_param_write2",
        va: 0x14091CA58,
        kind: Kind::Patch { new: "eb 29" },
        expect: "75 29",
        note: "Skip the second, opposite-sense Hilbert call.",
    },
];
