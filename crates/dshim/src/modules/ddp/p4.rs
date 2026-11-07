//! p4: force the DD+ core's channel-configuration (core param 4) and the layout-reachability
//! edits around it, in dee_ddp_encoder. Every capability is one site or a small set of sites; a
//! build whose profile lacks a site skips that capability.
//!
//! Levers:
//!   DDP_ACMOD=n             overwrite the chooser's core param 4 (acmod code)
//!   DDP_DATA_RATE=k         overwrite the data rate the chooser chose
//!   DDP_DOWNMIX_MODE=n      force the front-end downmix mode for the encode pass (4=off)
//!   DDP_SET_PARAMS=id=v,..  override or append core params at set_params
//!   DDP_LFE_ROUTE=1         route the LFE for codes 0..2 and NOP the error-137 checks
//!   DDP_EMDF_OFF=1          remove the EMDF data-rate gate (error 217)

use crate::site::{Resolved, Resolver};
use crate::{detour_install, mem_read, write_code};
use dshim_profiles::Kind;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

static CHOOSER_TRAMP: AtomicUsize = AtomicUsize::new(0);
static P4_VAL: AtomicI32 = AtomicI32::new(-1);
static RATE_VAL: AtomicI32 = AtomicI32::new(-1);
/// The chooser's out struct is its first argument on the GCC builds but its second on the MSVC
/// Windows build (the chooser writes through arg2). Set from the matched profile's OS at install.
static CHOOSER_OUT_IS_A2: AtomicBool = AtomicBool::new(false);

static OCL_TRAMP: AtomicUsize = AtomicUsize::new(0);
static OCL_VAL: AtomicI32 = AtomicI32::new(-1);
static EVO_ACMOD_TRAMP: AtomicUsize = AtomicUsize::new(0);

static SETP_TRAMP: AtomicUsize = AtomicUsize::new(0);
// The DDP_SET_PARAMS overrides, in order; a later pair for the same id wins.
static DDP_SET_PARAMS: std::sync::Mutex<Vec<(i32, i32)>> = std::sync::Mutex::new(Vec::new());

type Fn3 = extern "C" fn(usize, usize, usize) -> i64;
type Fn6 = extern "C" fn(usize, usize, usize, usize, usize, usize) -> i64;

/// chooser(out, codec, cfg): run the original, then overwrite the acmod and rate it chose.
extern "C" fn hook_chooser(a1: usize, a2: usize, a3: usize) -> i64 {
    let tr: Fn3 = unsafe { std::mem::transmute(CHOOSER_TRAMP.load(Ordering::Relaxed)) };
    let r = tr(a1, a2, a3);
    let p4 = P4_VAL.load(Ordering::Relaxed);
    let rate = RATE_VAL.load(Ordering::Relaxed);
    // out[0] = acmod, out[2] = rate. The out struct is arg1 on the GCC builds, arg2 on Windows.
    let out = if CHOOSER_OUT_IS_A2.load(Ordering::Relaxed) {
        a2
    } else {
        a1
    };
    unsafe {
        if p4 >= 0 {
            *(out as *mut i32) = p4;
        }
        if rate >= 0 {
            *((out + 8) as *mut i32) = rate;
        }
    }
    r
}

/// downmix_mode(): discard the real return, force DDP_DOWNMIX_MODE.
extern "C" fn hook_ocl(a1: usize, a2: usize, a3: usize, a4: usize, a5: usize, a6: usize) -> i64 {
    let tr: Fn6 = unsafe { std::mem::transmute(OCL_TRAMP.load(Ordering::Relaxed)) };
    let _ = tr(a1, a2, a3, a4, a5, a6);
    OCL_VAL.load(Ordering::Relaxed) as i64
}

extern "C" fn hook_evo_acmod(layout: usize, channels: u32) -> i64 {
    // With downmixing disabled, the hash metadata must describe all eight input channels.
    if channels == 8 {
        return 21;
    }
    let original: extern "C" fn(usize, u32) -> i64 =
        unsafe { std::mem::transmute(EVO_ACMOD_TRAMP.load(Ordering::Relaxed)) };
    original(layout, channels)
}

/// set_params(enc, program, count, pairs): merge DDP_SET_PARAMS overrides into the pair array.
extern "C" fn hook_setp(
    enc: usize,
    program: usize,
    count: usize,
    pairs: usize,
    a5: usize,
    a6: usize,
) -> i64 {
    const MAX: usize = 64;
    let over = DDP_SET_PARAMS.lock().unwrap().clone();
    // The count is a 32-bit int: macOS passes it in edx with the upper half undefined.
    let mut n = (count as u32 as usize).min(MAX);
    let mut buf: Vec<i32> = Vec::with_capacity(2 * MAX);
    unsafe {
        let src = std::slice::from_raw_parts(pairs as *const i32, 2 * n);
        buf.extend_from_slice(src);
    }
    buf.resize(2 * MAX, 0);
    for (id, val) in over {
        let mut found = None;
        for j in 0..n {
            if buf[2 * j] == id {
                found = Some(j);
                break;
            }
        }
        let j = found.unwrap_or_else(|| {
            let j = n;
            n += 1;
            j
        });
        buf[2 * j] = id;
        buf[2 * j + 1] = val;
    }
    let tr: Fn6 = unsafe { std::mem::transmute(SETP_TRAMP.load(Ordering::Relaxed)) };
    tr(enc, program, n, buf.as_ptr() as usize, a5, a6)
}

/// Apply a Patch site: write its `new` bytes (from the profile) at the resolved address.
fn apply_patch(res: &Resolver, name: &str, log: &mut dyn Write) -> bool {
    let Some(r) = res.site(name, log) else {
        return false;
    };
    if let Kind::Patch { new } = r.kind {
        let bytes = dshim_profiles::hex(new);
        if write_code(r.addr, &bytes) {
            lok!(log, "p4", "patch {name} @ {:#x} ({new})", r.addr);
            return true;
        }
        lerr!(log, "p4", "patch {name} write failed");
    }
    false
}

/// Grow the output frame buffer so a higher-than-default rate does not overrun it. The default
/// `mov [ctx+off], 6656` sizes the buffer for a 1664 kbps frame (rate * 4 bytes at 48 kHz); a
/// frame above 1664 kbps would write past it and corrupt the heap. Only the instruction's imm32
/// (at offset 7 of the 11-byte `mov`) is rewritten, to cover `rate` kbps with margin. The
/// encoder's own validators still cap the real rate (e.g. EMDF error 217 at >= 1920), so this
/// only removes the crash; it does not force an out-of-range stream.
fn size_outbuf(res: &Resolver, rate: u32, log: &mut dyn Write) {
    const DEFAULT_BYTES: u32 = 6656; // = 1664 kbps frame; at or below this the stock buffer fits
    let frame = rate.saturating_mul(4); // output frame bytes at 48 kHz
    if frame <= DEFAULT_BYTES {
        return; // default already holds this rate; leave the binary's bytes untouched
    }
    let need = frame.saturating_add(1024); // a margin over the frame
    let Some(r) = res.site("ddp.rate.outbuf_size", log) else {
        lerr!(log, "p4", "cannot size the output buffer for {rate} kbps");
        let _ = log.flush();
        std::process::exit(3);
    };
    // imm32 sits at byte 7 of the `mov qword [reg+disp32], imm32` form the profile pins.
    let imm = need.to_le_bytes();
    if write_code(r.addr + 7, &imm) {
        lok!(
            log,
            "p4",
            "output buffer grown to {need} bytes @ {:#x} for rate {rate}",
            r.addr + 7
        );
    } else {
        lerr!(log, "p4", "output buffer grow failed");
        let _ = log.flush();
        std::process::exit(3);
    }
}

fn hook_site(
    res: &Resolver,
    name: &str,
    hook: usize,
    tramp: &AtomicUsize,
    log: &mut dyn Write,
) -> Option<Resolved> {
    let r = res.site(name, log)?;
    match detour_install(r.addr, r.steal, hook) {
        Some(t) => {
            tramp.store(t, Ordering::Relaxed);
            lok!(log, "p4", "hook {name} @ {:#x} steal {}", r.addr, r.steal);
            Some(r)
        }
        None => {
            lerr!(log, "p4", "hook {name} detour failed");
            None
        }
    }
}

pub fn install(res: &Resolver, log: &mut dyn Write) {
    let ddp = &crate::levers().ddp;
    let p4 = ddp.acmod;
    let rate = ddp.data_rate;
    let ocl = ddp.downmix_mode;
    let setp = ddp.set_params.clone();
    let lfe = ddp.lfe_route;
    let emdf_off = ddp.emdf_off;

    if p4.is_none() && rate.is_none() && ocl.is_none() && setp.is_empty() && !lfe && !emdf_off {
        return; // module idle
    }
    ldebug!(log, "p4", "arming DDP_ACMOD={p4:?} RATE={rate:?} DDP_DOWNMIX_MODE={ocl:?} DDP_SET_PARAMS={} DDP_LFE_ROUTE={lfe} DDP_EMDF_OFF={emdf_off}", setp.len());

    // DDP_ACMOD / DDP_DATA_RATE: always via the chooser detour (installed when either is set).
    if let Some(v) = p4 {
        P4_VAL.store(v, Ordering::Relaxed);
    }
    if let Some(v) = rate {
        RATE_VAL.store(v, Ordering::Relaxed);
    }
    if p4.is_some() || rate.is_some() {
        CHOOSER_OUT_IS_A2.store(
            res.profile.os == dshim_profiles::Os::Windows,
            Ordering::Relaxed,
        );
        hook_site(
            res,
            "ddp.p4.chooser",
            hook_chooser as *const () as usize,
            &CHOOSER_TRAMP,
            log,
        );
    }
    let split_total = ddp.rates.map(|rates| rates[0] + rates[1]);
    let output_rate = rate.unwrap_or(0).max(split_total.unwrap_or(0));
    if output_rate > 0 {
        size_outbuf(res, output_rate as u32, log);
    }

    if let Some(v) = ocl {
        OCL_VAL.store(v, Ordering::Relaxed);
        hook_site(
            res,
            "ddp.p4.downmix_mode",
            hook_ocl as *const () as usize,
            &OCL_TRAMP,
            log,
        );
    }
    if p4.is_some()
        && ocl == Some(4)
        && res.profile.site("ddp.p4.evo_acmod").is_some()
        && hook_site(
            res,
            "ddp.p4.evo_acmod",
            hook_evo_acmod as *const () as usize,
            &EVO_ACMOD_TRAMP,
            log,
        )
        .is_none()
    {
        let _ = log.flush();
        std::process::exit(3);
    }

    if !setp.is_empty() {
        *DDP_SET_PARAMS.lock().unwrap() = setp;
        hook_site(
            res,
            "ddp.p4.set_params",
            hook_setp as *const () as usize,
            &SETP_TRAMP,
            log,
        );
    }

    if lfe {
        route_lfe(res, log);
        apply_patch(res, "ddp.p4.e137_set_params", log);
        apply_patch(res, "ddp.p4.e137_after_preproc", log);
        apply_patch(res, "ddp.p4.e137_process", log);
        // The Windows build duplicates the acmod/LFE 137 guard into a fourth validate; apply it
        // where the profile defines one. The GCC and Clang builds have three throws.
        if res.profile.site("ddp.p4.e137_param_apply").is_some() {
            apply_patch(res, "ddp.p4.e137_param_apply", log);
        }
    }

    if emdf_off {
        // Remove the EMDF/evolution data-rate gate so the DD+ height layouts encode above 1792.
        // Above 1792 the encoder uses 3-block frames, which EMDF (a 6-block feature) cannot ride;
        // the gate normally refuses this with error 217. Without it the base channels still
        // encode and decode (verified to 3024 kbps in FFmpeg), but the evolution-metadata path is
        // skipped, so the stream is non-standard. The output buffer must grow too, which
        // DDP_DATA_RATE or DDP_RATES does above.
        lwarn!(
            log,
            "p4",
            "EMDF rate gate removed, streams above 1792 kbps are non-standard"
        );
        apply_patch(res, "ddp.rate.emdf_gate", log);
    }
}

/// Route the LFE for codes 0..2 (DDP_LFE_ROUTE): add pair 3 = {substream 0, slot 5} to records 0..2
/// of the input routing table, and OR bit 3 into their channel mask.
fn route_lfe(res: &Resolver, log: &mut dyn Write) {
    if let Some(r) = res.site("ddp.p4.route_table", log) {
        for rec in 0..3usize {
            let pair = r.addr + 88 * rec + 8 * 3;
            write_code(pair, &0i32.to_le_bytes()); // substream 0
            write_code(pair + 4, &5i32.to_le_bytes()); // slot 5 (core LFE)
        }
        lok!(log, "p4", "LFE routed, codes 0..2");
    }
    if let Some(r) = res.site("ddp.p4.chan_mask", log) {
        for rec in 0..3usize {
            let addr = r.addr + 4 * rec;
            let cur = i32::from_le_bytes(mem_read(addr, 4).try_into().unwrap());
            write_code(addr, &(cur | (1 << 3)).to_le_bytes());
        }
        lok!(log, "p4", "LFE bit set in channel mask, codes 0..2");
    }
}
