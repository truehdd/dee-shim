//! Per-substream data rate for dee_ddp_encoder (DD+ core), the plain-ddp twin of the ddpjoc
//! DDPJOC_RATES feature. The DD+ 7.1 and height layouts build two coded substreams (an
//! independent and a dependent, or an AC-3 core and a dependent); this overrides the kbps of each.
//!
//! Lever:
//!   DDP_RATES=r0,r1   per-substream data rate in kbps (substream 0, substream 1); each 32..1024.
//!                     Start DEE at a legal --data-rate; the coded frames then follow r0/r1.
//!
//! Mechanism: the DD+ core calls engine_check(cfg, ...) once per substream per frame with a stable
//! per-substream config pointer; the hook overwrites that substream's coded rate (cfg+120) with the
//! forced one before the original validates it.

use crate::detour_install;
use crate::site::Resolver;
use std::io::Write;
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

static ENG_TRAMP: AtomicUsize = AtomicUsize::new(0);
static RATES: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];
// The distinct substream config pointers, in first-seen order, which is the coded substream order
// (independent or core first, then dependent). This indexes the rates. ddpjoc matches cfg against
// a config array at engine state+0x4330; the plain-ddp engine keeps no such array at a fixed state
// offset (the pointer is nowhere near the state base the caller passes), so ddp indexes by order.
static SEEN: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];

const CFG_RATE: usize = 120; // i16 substream rate in kbps (same as ddpjoc)

type Fn6 = extern "C" fn(usize, usize, usize, usize, usize, usize) -> i64;

/// The substream index of `cfg`: its slot in SEEN, claiming the next free slot on first sight.
/// Returns None for a config beyond the two we carry rates for.
fn slot_of(cfg: usize) -> Option<usize> {
    for (i, slot) in SEEN.iter().enumerate() {
        match slot.compare_exchange(0, cfg, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return Some(i),              // claimed a free slot
            Err(v) if v == cfg => return Some(i), // already ours
            Err(_) => continue,                   // taken by another config
        }
    }
    None
}

/// engine_check(cfg, state + 4): before the original validates this substream, overwrite its coded
/// rate (cfg+120) with the forced one for its position in the stream.
extern "C" fn hook_engine_check(
    cfg: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
) -> i64 {
    if let Some(i) = slot_of(cfg) {
        let r = RATES[i].load(Ordering::Relaxed);
        if r > 0 {
            unsafe { *((cfg + CFG_RATE) as *mut i16) = r as i16 };
        }
    }
    let tr: Fn6 = unsafe { std::mem::transmute(ENG_TRAMP.load(Ordering::Relaxed)) };
    tr(cfg, a2, a3, a4, a5, a6)
}

/// Install the per-substream rate override if DDP_RATES is set; a missing site ends the process.
/// The CLI checked the rates (each 32..1024 kbps); the original engine validates the codec and
/// layout limits.
pub fn install(res: &Resolver, log: &mut dyn Write) {
    let Some(r) = crate::levers().ddp.rates else {
        return;
    };
    let Some(site) = res.site("ddp.core.engine_check", log) else {
        lerr!(
            log,
            "rates",
            "DDP_RATES set but ddp.core.engine_check is not in this profile"
        );
        let _ = log.flush();
        std::process::exit(3);
    };
    match detour_install(
        site.addr,
        site.steal,
        hook_engine_check as *const () as usize,
    ) {
        Some(t) => {
            RATES[0].store(r[0], Ordering::Relaxed);
            RATES[1].store(r[1], Ordering::Relaxed);
            ENG_TRAMP.store(t, Ordering::Relaxed);
            lok!(
                log,
                "rates",
                "per-substream rate {},{} kbps, engine_check @ {:#x} steal {}",
                r[0],
                r[1],
                site.addr,
                site.steal
            );
        }
        None => {
            lerr!(
                log,
                "rates",
                "ddp.core.engine_check detour failed, stopping"
            );
            let _ = log.flush();
            std::process::exit(3);
        }
    }
}
