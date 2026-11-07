//! DD+ 6.7.0, macOS x86_64. Optional odd-count WAV gates are not profiled.

use crate::{Ident, Kind, Os, Profile, Site};

pub static PROFILE: Profile = Profile {
    tool: "dee_ddp_encoder",
    os: Os::Macos,
    version: "6.7.0",
    ident: Ident::MachoUuid("C5B5BB01-C207-353E-A3A4-8D943A92765D"),
    sites: SITES,
};

static SITES: &[Site] = &[
    Site {
        name: "ddp.core.surround_downmix_type",
        va: 0x100985AB0,
        kind: Kind::Patch {
            new: "c7 47 18 01 00 00 00 c3 90 90 90 90 90 90",
        },
        expect: "55 48 89 e5 83 fe 01 77 03 89 77 18 5d c3",
        note: "Surround downmix type setter for the 7.x to 5.x core; DDP_CORE_TYPE replaces it with a store of the selector at context+24 (0 PLIIx, 1 Standard).",
    },
    Site {
        name: "ddp.p4.evo_acmod",
        va: 0x100107960,
        kind: Kind::Hook { steal: 13 },
        expect: "55 48 89 e5 41 57 41 56 41 55 41 54 53",
        note: "EVO metadata acmod chooser; channel count is argument 2.",
    },
    Site {
        name: "ddp.p4.chooser",
        va: 0x10010D360,
        kind: Kind::Hook { steal: 13 },
        expect: "55 48 89 e5 41 57 41 56 41 55 41 54 53",
        note: "Chooses acmod at output+0 and rate at output+8; output is the first argument.",
    },
    Site {
        name: "ddp.p4.set_params",
        va: 0x10096B7F0,
        kind: Kind::Hook { steal: 14 },
        expect: "55 48 89 e5 41 57 41 56 41 55 41 54 53 50",
        note: "Encoder parameter setter (encoder, program, count, key/value pairs).",
    },
    Site {
        name: "ddp.p4.downmix_mode",
        va: 0x100090130,
        kind: Kind::Hook { steal: 16 },
        expect: "55 48 89 e5 41 57 41 56 53 48 81 ec 88 01 00 00",
        note: "Frontend downmix mode; return value is overridden after the original call.",
    },
    Site {
        name: "ddp.rate.outbuf_size",
        va: 0x100111574,
        kind: Kind::Patch {
            new: "48 c7 83 10 01 00 00 00 40 00 00",
        },
        expect: "48 c7 83 10 01 00 00 00 1a 00 00",
        note: "Output buffer size at frontend+0x110; default 6656 bytes.",
    },
    Site {
        name: "ddp.rate.emdf_gate",
        va: 0x10097015F,
        kind: Kind::Patch { new: "90 90" },
        expect: "7c 78",
        note: "Rate-index guard in the EVO metadata support check (error 217); DDP_EMDF_OFF NOPs the jl.",
    },
    Site {
        name: "ddp.p4.route_table",
        va: 0x100D01F30,
        kind: Kind::Data,
        expect: "00 00 00 00 00 00 00 00 00 00 00 00 02 00 00 00",
        note: "Input routing table; 88-byte records.",
    },
    Site {
        name: "ddp.p4.chan_mask",
        va: 0x100D002F0,
        kind: Kind::Data,
        expect: "03 00 00 00 04 00 00 00 03 00 00 00 0f 00 00 00",
        note: "Acmod-to-channel-enable mask.",
    },
    Site {
        name: "ddp.p4.e137_set_params",
        va: 0x100973623,
        kind: Kind::Patch { new: "eb 1d" },
        expect: "7f 1d",
        note: "Accept LFE for low acmod values in parameter validation.",
    },
    Site {
        name: "ddp.p4.e137_after_preproc",
        va: 0x100976B84,
        kind: Kind::Patch { new: "90 90" },
        expect: "74 27",
        note: "Accept LFE for low acmod values after preprocessing.",
    },
    Site {
        name: "ddp.p4.e137_process",
        va: 0x10097516A,
        kind: Kind::Patch { new: "eb 0e" },
        expect: "7f 0e",
        note: "Accept LFE for low acmod values in process validation.",
    },
    Site {
        name: "ddp.core.engine_check",
        va: 0x100976A20,
        kind: Kind::Hook { steal: 13 },
        expect: "55 48 89 e5 41 57 41 56 53 50 49 89 f7",
        note: "Per-substream validation; coded rate remains i16 at config+120.",
    },
    Site {
        name: "ddp.core.phase90_param_write",
        va: 0x100953FF1,
        kind: Kind::Patch { new: "eb 3d" },
        expect: "74 3d",
        note: "Skip the first preprocessing Hilbert call.",
    },
    Site {
        name: "ddp.core.phase90_param_write2",
        va: 0x100954133,
        kind: Kind::Patch {
            new: "90 90 90 90 90 90",
        },
        expect: "0f 84 eb 00 00 00",
        note: "Skip the second, opposite-sense Hilbert call.",
    },
];
