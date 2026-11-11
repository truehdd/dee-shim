//! The coupled-lever features of dee_ddpjoc_encoder. Each feature is a set of sites applied all
//! or none: every site is resolved and byte-checked before any is written, and a requested feature
//! that cannot be applied in full ends the process (exit 3) instead of encoding a stream that
//! decodes cleanly but is wrong.
//!
//! Levers (dshim-levers; the CLI checks names and values):
//!   DDPJOC_LAYER=7.1        7.1 backbone layer instead of 5.1.2 (render code 21).
//!   DDPJOC_LAYER=5.1.2-top  5.1.2 top-surround backbone (Lts/Rts) instead of the front-height
//!                           5.1.2 (render code 28); the 7.1 mechanism with a different code.
//!   DDPJOC_CORE=eac3        E-AC-3 core instead of the Blu-ray AC-3 core (needs -e bluray).
//!   DDPJOC_RATES=r0,r1      per-substream kbps (core or independent, dependent); needs
//!                           -e bluray. Start DEE at a legal --data-rate; frames follow r0/r1.
//!   DDPJOC_PHASE90=off      no 90 degree phase shift on the surrounds of the coded 5.1 (the DD+
//!                           core's phase-90 filter, which ddpjoc always enables). Both modes.
//!                           The object render is unchanged; 5.1 and 7.1 legacy decoders gain.
//!
//! The switches below take on or off, default off; on is the as-is direction.
//!   DDPJOC_OAMD_EXACT_POSITIONS=on
//!                           code the DAMF positions as authored, unsmoothed (by default a 25 ms
//!                           one-pole per 512 samples). For masters whose positions were already
//!                           smoothed once, such as a TrueHD decode.
//!   DDPJOC_OAMD_EXACT_SIZES=on
//!                           the same for object size (a 20 ms one-pole per 512 samples).
//!   DDPJOC_OAMD_OWN_CLUSTERS=on
//!                           when the objects that reach the clusterer fit in its clusters, give
//!                           each its own cluster instead of folding near ones together; above
//!                           the budget the clusterer runs as shipped (frame by frame).
//!   DDPJOC_OAMD_EXACT_TIMING=on
//!                           in a frame that fits, code every position change in its own frame
//!                           instead of holding or advancing it by one frame at a jump.
//!   DDPJOC_OAMD_KEEP_QUIET=on
//!                           when every object with any energy fits, let quiet objects (weight
//!                           under 0.5, about -90 dBFS) reach the clusterer instead of leaving
//!                           them out with a leftover position.
//!   DDPJOC_OAMD_OWN_GAINS=on
//!                           in a frame that fits, mix each object only into its own cluster; as
//!                           shipped, an object at the same position as another cluster's object
//!                           is mixed into that one and its own cluster is frozen, not active.
//!   DDPJOC_OAMD_KEEP_ORDER=on
//!                           when the master's elements fit, code them in master order (bed
//!                           channels other than LFE that sound from the start, then the
//!                           objects); as shipped, in the order the clusterer first picked
//!                           them.
//!   DDPJOC_OAMD_CLUSTER_PASSTHROUGH=on
//!                           all five above: an already clustered master, such as a TrueHD
//!                           decode, is coded as it is.
//!   DDPJOC_OAMD_EXT_PRECISION=on
//!                           code positions with OAMD extended precision (element 5, a fifth of a
//!                           6-bit step) as dthd does with legacy_authoring=0; stock DD+ JOC
//!                           codes the 6-bit grid only.
//!   DDPJOC_OAMD_AS_IS=on    exact positions and sizes, the cluster pass-through and extended
//!                           precision.
//!
//! An umbrella only supplies defaults: a switch set on its own wins (DDPJOC_OAMD_AS_IS=on with
//! DDPJOC_OAMD_EXACT_TIMING=off keeps the stock jump timing).

use crate::detour_install;
use crate::modules::cluster;
use crate::modules::common::require_all;
use crate::site::{Resolved, Resolver};
use crate::write_code;
use dshim_levers::{Core, Layer};
use dshim_profiles::{hex, Kind};
use std::io::Write;
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

type Fn6 = extern "C" fn(usize, usize, usize, usize, usize, usize) -> i64;

// backbone layer
static PE_TRAMP: AtomicUsize = AtomicUsize::new(0);
const RENDER_7_1: i32 = 21;
// 5.1.2 top-surround backbone (Lts/Rts): the render fan-out's code bitmask covers {19, 21, 28}
// and the channel mask table has an entry for it.
const RENDER_5_1_2_TOP: i32 = 28;
// The backbone render code the hook forces (21 or 28).
static RENDER_TARGET: AtomicI32 = AtomicI32::new(0);
// Engine state: the master backbone layout code, which the per-frame process broadcasts into
// state+0x24 (the field the render fan-out branches on).
const STATE_LAYOUT_MASTER_OFF: usize = 0x34;

// per-substream rate
static ENG_TRAMP: AtomicUsize = AtomicUsize::new(0);
static RATES: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];
const STATE_FROM_ARG2: usize = 4; // the caller passes state + 4 as the second argument
const STATE_NSUB: usize = 0x4328; // i32 substream count
const STATE_CFGS: usize = 0x4330; // substream config pointers, 8 bytes each
const CFG_RATE: usize = 120; // i16 substream rate in kbps

/// The outer core's per-frame process(state, ...): force the master backbone layout field
/// (state+0x34) to the backbone target (21 for 7.1, 28 for 5.1.2-top) on entry. The body
/// broadcasts state+0x34 into state+0x24, and the per-frame render fan-out then skips the
/// 5.1.2-only handle (which the allocator did not create once render_param4 selected the target)
/// and takes the target branch. The fan-out gates on `cmp [state+0x24],19`; without this the
/// field stays 19 and the fan-out dereferences the missing handle.
extern "C" fn hook_pre_engine(
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
) -> i64 {
    if a1 != 0 {
        unsafe {
            *((a1 + STATE_LAYOUT_MASTER_OFF) as *mut i32) = RENDER_TARGET.load(Ordering::Relaxed)
        };
    }
    let tr: Fn6 = unsafe { std::mem::transmute(PE_TRAMP.load(Ordering::Relaxed)) };
    tr(a1, a2, a3, a4, a5, a6)
}

/// engine_check(cfg, state + 4): write the forced rate of this substream into cfg+120 first.
extern "C" fn hook_engine_check(
    cfg: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
) -> i64 {
    let state = a2.wrapping_sub(STATE_FROM_ARG2);
    unsafe {
        let n = (*((state + STATE_NSUB) as *const i32)).clamp(0, 8) as usize;
        for (i, rate) in RATES.iter().enumerate().take(n) {
            if *((state + STATE_CFGS + 8 * i) as *const usize) == cfg {
                let r = rate.load(Ordering::Relaxed);
                if r > 0 {
                    *((cfg + CFG_RATE) as *mut i16) = r as i16;
                }
                break;
            }
        }
    }
    let tr: Fn6 = unsafe { std::mem::transmute(ENG_TRAMP.load(Ordering::Relaxed)) };
    tr(cfg, a2, a3, a4, a5, a6)
}

fn fail(feature: &str, what: &str, log: &mut dyn Write) -> ! {
    lerr!(log, "ddpjoc", "{feature}: {what}, stopping (all or none)");
    let _ = log.flush();
    std::process::exit(3);
}

fn patch(feature: &str, name: &str, r: &Resolved, log: &mut dyn Write) {
    let Kind::Patch { new } = r.kind else {
        fail(feature, &format!("{name} is not a patch site"), log)
    };
    if !write_code(r.addr, &hex(new)) {
        fail(feature, &format!("could not write {name}"), log);
    }
    lok!(
        log,
        "ddpjoc",
        "{feature}: patch {name} @ {:#x} ({new})",
        r.addr
    );
}

fn hook(
    feature: &str,
    name: &str,
    r: &Resolved,
    f: usize,
    tramp: &AtomicUsize,
    log: &mut dyn Write,
) {
    match detour_install(r.addr, r.steal, f) {
        Some(t) => {
            tramp.store(t, Ordering::Relaxed);
            lok!(log, "ddpjoc", "{feature}: hook {name} @ {:#x}", r.addr);
        }
        None => fail(feature, &format!("could not detour {name}"), log),
    }
}

pub fn install(res: &Resolver, log: &mut dyn Write) {
    let joc = &crate::levers().ddpjoc;

    // A non-default backbone layer maps to (render code, frontend render_param4 site): 7.1 -> 21,
    // 5.1.2-top -> 28. Both patch render_param4 and hook the outer core's per-frame process
    // (ddpjoc.core.render_fanout). The default 5.1.2 sets none.
    let backbone_target: Option<(i32, &str)> = match joc.layer {
        None | Some(Layer::L512) => None,
        Some(Layer::L71) => Some((RENDER_7_1, "ddpjoc.frontend.render_param4")),
        Some(Layer::L512Top) => Some((RENDER_5_1_2_TOP, "ddpjoc.frontend.render_param4_top")),
    };
    // Render 28 needs a wider querymem schema. Change only the entry's check type and bounds; its
    // set array is shared.
    let qm_render_allow = (backbone_target.map(|(t, _)| t) == Some(RENDER_5_1_2_TOP))
        .then(|| res.site("ddpjoc.core.querymem_render_allow", log))
        .flatten();
    if let Some((t, _)) = backbone_target {
        RENDER_TARGET.store(t, Ordering::Relaxed);
    }
    let eac3 = joc.core == Some(Core::Eac3);
    let rates = joc.rates;
    let phase90_off = joc.phase90 == Some(false);
    // A switch set on its own wins over the umbrellas, which only supply its default.
    let oamd = &joc.oamd;
    let exact_positions = oamd.exact_positions();
    let exact_sizes = oamd.exact_sizes();
    let ext_on = joc.ext_precision();
    let cluster_options = cluster::Options {
        own_clusters: oamd.own_clusters(),
        exact_timing: oamd.exact_timing(),
        keep_quiet: oamd.keep_quiet(),
        own_gains: oamd.own_gains(),
        keep_order: oamd.keep_order(),
    };

    // Resolve requested sites before writing.
    let backbone = backbone_target.map(|(_t, rp4)| {
        require_all(
            res,
            "backbone layer",
            &[rp4, "ddpjoc.core.render_fanout"],
            log,
        )
    });
    let core_sites = eac3.then(|| {
        require_all(
            res,
            "core eac3",
            &[
                "ddpjoc.core.prog_mode11_ss0_kind",
                "ddpjoc.core.bluray_ac3_stamp",
                "ddpjoc.core.ac3_delay_mode_cmp",
            ],
            log,
        )
    });
    let rate_sites =
        rates.map(|_| require_all(res, "rates", &["ddpjoc.core.engine_check_cfg"], log));
    let phase_sites = phase90_off
        .then(|| require_all(res, "phase90 off", &["ddpjoc.core.phase90_param_cmp"], log));
    let smooth_sites = exact_positions.then(|| {
        require_all(
            res,
            "exact positions",
            &["ddpjoc.oa.sampler_position_flag"],
            log,
        )
    });
    let size_sites =
        exact_sizes.then(|| require_all(res, "exact sizes", &["ddpjoc.oa.sampler_size_flag"], log));
    let ext_sites = ext_on.then(|| {
        require_all(
            res,
            "ext precision",
            &["ddpjoc.oamd.ext_precision_flag"],
            log,
        )
    });

    // Then write.
    cluster::install(res, "ddpjoc", cluster_options, None, log);
    if let (Some(s), Some((_t, rp4))) = (backbone.as_ref(), backbone_target) {
        patch("backbone layer", rp4, &s[0], log);
        hook(
            "backbone layer",
            "ddpjoc.core.render_fanout",
            &s[1],
            hook_pre_engine as *const () as usize,
            &PE_TRAMP,
            log,
        );
    }
    if let Some(r) = qm_render_allow {
        patch(
            "backbone layer",
            "ddpjoc.core.querymem_render_allow",
            &r,
            log,
        );
    }
    if let Some(s) = core_sites {
        patch("core eac3", "ddpjoc.core.prog_mode11_ss0_kind", &s[0], log);
        patch("core eac3", "ddpjoc.core.bluray_ac3_stamp", &s[1], log);
        patch("core eac3", "ddpjoc.core.ac3_delay_mode_cmp", &s[2], log);
    }
    if let (Some(r), Some(s)) = (rates, rate_sites) {
        RATES[0].store(r[0], Ordering::Relaxed);
        RATES[1].store(r[1], Ordering::Relaxed);
        hook(
            "rates",
            "ddpjoc.core.engine_check_cfg",
            &s[0],
            hook_engine_check as *const () as usize,
            &ENG_TRAMP,
            log,
        );
        lok!(
            log,
            "ddpjoc",
            "rates: independent {} + dependent {} = {} kbps",
            r[0],
            r[1],
            r[0] + r[1]
        );
    }
    if let Some(s) = phase_sites {
        patch("phase90 off", "ddpjoc.core.phase90_param_cmp", &s[0], log);
    }
    if let Some(s) = smooth_sites {
        patch(
            "exact positions",
            "ddpjoc.oa.sampler_position_flag",
            &s[0],
            log,
        );
    }
    if let Some(s) = size_sites {
        patch("exact sizes", "ddpjoc.oa.sampler_size_flag", &s[0], log);
    }
    if let Some(s) = ext_sites {
        patch(
            "ext precision",
            "ddpjoc.oamd.ext_precision_flag",
            &s[0],
            log,
        );
    }
}
