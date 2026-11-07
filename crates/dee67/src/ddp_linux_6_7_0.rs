//! DD+ 6.7.0, Linux x86_64.

use crate::{Ident, Kind, Os, Profile, Site};

pub static PROFILE: Profile = Profile {
    tool: "dee_ddp_encoder",
    os: Os::Linux,
    version: "6.7.0",
    ident: Ident::GnuBuildId("d93dbaf7669512f0f750608a106b255394baa1ea"),
    sites: SITES,
};

static SITES: &[Site] = &[
    Site {
        name: "ddp.core.surround_downmix_type",
        va: 0x10AE674,
        kind: Kind::Patch {
            new: "c7 47 18 01 00 00 00 c3 90 90 90 90 90 90 90 90",
        },
        expect: "83 fe 01 76 07 c3 66 0f 1f 44 00 00 89 77 18 c3",
        note: "Surround downmix type setter for the 7.x to 5.x core, just past its endbr64; DDP_CORE_TYPE replaces it with a store of the selector at context+24 (0 PLIIx, 1 Standard).",
    },
    Site {
        name: "ddp.p4.evo_acmod",
        va: 0x599500,
        kind: Kind::Hook { steal: 15 },
        expect: "41 57 45 31 ff 41 56 49 89 fe 41 55 41 89 f5",
        note: "EVO metadata acmod chooser; channel count is argument 2.",
    },
    Site {
        name: "ddp.p4.chooser",
        va: 0x59F620,
        kind: Kind::Hook { steal: 16 },
        expect: "41 57 66 0f ef c0 41 56 41 55 41 54 49 89 fc 55",
        note: "Chooses acmod at output+0 and rate at output+8; output is the first argument.",
    },
    Site {
        name: "ddp.p4.set_params",
        va: 0x100C450,
        kind: Kind::Hook { steal: 14 },
        expect: "f3 0f 1e fa 41 57 41 56 41 55 41 54 55 53",
        note: "Encoder parameter setter (encoder, program, count, key/value pairs).",
    },
    Site {
        name: "ddp.p4.downmix_mode",
        va: 0x501E40,
        kind: Kind::Hook { steal: 13 },
        expect: "41 57 b8 04 00 00 00 41 56 41 55 41 54",
        note: "Frontend downmix mode.",
    },
    Site {
        name: "ddp.p4.route_table",
        va: 0x164C220,
        kind: Kind::Data,
        expect: "00 00 00 00 00 00 00 00 00 00 00 00 02 00 00 00",
        note: "Input routing table; 88-byte records.",
    },
    Site {
        name: "ddp.p4.chan_mask",
        va: 0x167B560,
        kind: Kind::Data,
        expect: "03 00 00 00 04 00 00 00 03 00 00 00 0f 00 00 00",
        note: "Acmod-to-channel-enable mask.",
    },
    Site {
        name: "ddp.rate.outbuf_size",
        va: 0x59F542,
        kind: Kind::Patch {
            new: "48 c7 83 18 01 00 00 00 40 00 00",
        },
        expect: "48 c7 83 18 01 00 00 00 1a 00 00",
        note: "Default output buffer size, 6656 bytes.",
    },
    Site {
        name: "ddp.rate.emdf_gate",
        va: 0x1016879,
        kind: Kind::Patch { new: "eb 35" },
        expect: "7f 35",
        note: "Rate-index guard in the EVO metadata support check (error 217); DDP_EMDF_OFF makes the jg unconditional.",
    },
    Site {
        name: "ddp.p4.e137_set_params",
        va: 0x1018991,
        kind: Kind::Patch {
            new: "90 90 90 90 90 90",
        },
        expect: "0f 8e a1 00 00 00",
        note: "Low-acmod LFE guard in parameter validation.",
    },
    Site {
        name: "ddp.p4.e137_after_preproc",
        va: 0x101A825,
        kind: Kind::Patch {
            new: "e9 f1 fb ff ff 90",
        },
        expect: "0f 8f f0 fb ff ff",
        note: "Low-acmod LFE guard in process validation.",
    },
    Site {
        name: "ddp.p4.e137_process",
        va: 0x101CD97,
        kind: Kind::Patch { new: "eb 9d" },
        expect: "75 9d",
        note: "Low-acmod LFE guard after preprocessing.",
    },
    Site {
        name: "ddp.core.engine_check",
        va: 0x101CCA0,
        kind: Kind::Hook { steal: 14 },
        expect: "f3 0f 1e fa 55 48 89 f5 31 f6 53 48 89 fb",
        note: "Per-substream validation; coded rate is i16 at config+120.",
    },
    Site {
        name: "ddp.core.phase90_param_write",
        va: 0x1018EBA,
        kind: Kind::Patch { new: "31 db 90 90" },
        expect: "8b 5c 24 30",
        note: "Load of the value written to the phase-90 field at engine config+0x7C; DDP_PHASE90=off loads 0 instead.",
    },
];
