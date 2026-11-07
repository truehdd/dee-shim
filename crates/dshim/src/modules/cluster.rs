//! Shared spatial-coder cluster controls for TrueHD and DD+ JOC.

use crate::modules::common::require_all;
use crate::site::{Resolved, Resolver};
use crate::{alloc_rwx, detour_install, flush_icache, write_code};
use dshim_profiles::{hex, Kind, Site};
use std::io::Write;
use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::OnceLock;

type Fn10 =
    extern "C" fn(usize, usize, usize, usize, usize, usize, usize, usize, usize, usize) -> i64;
type FnSelect = extern "C" fn(
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
) -> i64;
type FnActivity = extern "C" fn(
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
) -> i64;
type FnGains = extern "C" fn(usize, usize, usize, usize, usize, usize, usize, usize, usize) -> i64;

static SEL_TRAMP: AtomicUsize = AtomicUsize::new(0);
static ACT_TRAMP: AtomicUsize = AtomicUsize::new(0);
static FILTER_TRAMP: AtomicUsize = AtomicUsize::new(0);
static GAINS_TRAMP: AtomicUsize = AtomicUsize::new(0);
static THRESHOLD_CELL: AtomicUsize = AtomicUsize::new(0);
static SLOT_IDS: AtomicUsize = AtomicUsize::new(0);
static EXACT_TIMING: AtomicU8 = AtomicU8::new(0);
static OWN_GAINS: AtomicU8 = AtomicU8::new(0);
// 1 while the current frame's candidates fit in its clusters; set at each greedy selection, read
// by the keep stub inside it and by the activity and gains hooks after it in the same frame.
static FITS: AtomicU8 = AtomicU8::new(0);
static KEEP_TRAMP: AtomicUsize = AtomicUsize::new(0);
// Counts for the exit line: selections, fitting selections, frames whose jump controls were
// zeroed, filter calls with the threshold lowered, gain columns moved, of which set to unity,
// silence flags cleared, fitting selections folded, gain columns changed.
static STATS: [AtomicU64; 9] = [const { AtomicU64::new(0) }; 9];

const STOCK_THRESHOLD: f32 = 0.5;
// A moved column's total is the stock snapped gain, min(column sum, 1). The column is a
// normalised panning distribution, so for an object that owns its cluster the sum is 1 up to
// float rounding, and a sum one step under 1 scales the samples by 1 - 2^-24: full scale samples
// lose 1 LSB. Within this of 1 the gain is set to exactly 1.
const UNIT_TOLERANCE: f32 = 1e-5;
const RECORD_SIZE: usize = 60;
const RECORD_FLAG: usize = 4;
// Word 0 of a record: the element is silent, written by the preprocessor's silence detector.
const RECORD_SILENT: usize = 0;
const STATE_SLOT_IDS: usize = 32;

fn bump(i: usize) {
    STATS[i].fetch_add(1, Ordering::Relaxed);
}

extern "C" fn hook_keep(
    positions: usize,
    coverage: usize,
    sizes: usize,
    count: i32,
    out: *mut u32,
    chosen: i32,
) -> i64 {
    if FITS.load(Ordering::Relaxed) != 0 && count > 0 {
        let mut kept = 0;
        for j in 0..count {
            if j != chosen {
                unsafe { *out.add(kept) = j as u32 };
                kept += 1;
            }
        }
        for j in kept..count as usize {
            unsafe { *out.add(j) = 0 };
        }
        return kept as i64;
    }
    type Keep = extern "C" fn(usize, usize, usize, i32, *mut u32, i32) -> i64;
    let original: Keep = unsafe { std::mem::transmute(KEEP_TRAMP.load(Ordering::Relaxed)) };
    original(positions, coverage, sizes, count, out, chosen)
}

extern "C" {
    fn atexit(cb: extern "C" fn()) -> i32;
}

extern "C" fn report() {
    let s: Vec<u64> = STATS.iter().map(|c| c.load(Ordering::Relaxed)).collect();
    log::debug!(target: "dshim::cluster",
        "cluster: cluster pass-through, selections {}, fitting {}, jump controls zeroed {}, filters relaxed {}, gain columns moved {} (set to unity {}), silence flags cleared {}, fitting selections folded {}, gain columns changed {}",
        s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7], s[8]
    );
}

/// greedy_select(..., sixth argument {clusters K, candidates N}, ...): note whether the frame fits.
#[allow(clippy::too_many_arguments)]
extern "C" fn hook_greedy_select(
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
    a8: usize,
    a9: usize,
    a10: usize,
    a11: usize,
) -> i64 {
    let (k, n) = unsafe { (*(a6 as *const u32), *((a6 + 4) as *const u32)) };
    FITS.store((n <= k) as u8, Ordering::Relaxed);
    bump(0);
    if n <= k {
        bump(1);
    }
    let tr: FnSelect = unsafe { std::mem::transmute(SEL_TRAMP.load(Ordering::Relaxed)) };
    let result = tr(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11);
    if n <= k && unsafe { *(a9 as *const u32) } < n {
        bump(7);
    }
    result
}

/// cluster_activity(..., 7th argument state + 16 (pointing at the cluster count), ..., 15th
/// argument jump controls int[K], ...): in a frame that fits, after the original, every cluster
/// keeps the common one-frame delay.
#[allow(clippy::too_many_arguments)]
extern "C" fn hook_cluster_activity(
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
    a8: usize,
    a9: usize,
    a10: usize,
    a11: usize,
    a12: usize,
    a13: usize,
    a14: usize,
    a15: usize,
    a16: usize,
) -> i64 {
    let tr: FnActivity = unsafe { std::mem::transmute(ACT_TRAMP.load(Ordering::Relaxed)) };
    let r = tr(
        a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16,
    );
    if EXACT_TIMING.load(Ordering::Relaxed) != 0
        && FITS.load(Ordering::Relaxed) != 0
        && a7 != 0
        && a15 != 0
    {
        let k = unsafe { *(a7 as *const u32) } as usize;
        for i in 0..k {
            unsafe { *(a15 as *mut i32).add(i) = 0 };
        }
        bump(2);
    }
    r
}

/// candidate_filter(count, K, ?, records, weights, ?, state, ...): note the slot element ids;
/// with quiet objects kept, clear the silence flag of every element whose block is not all zero
/// and lower the threshold when every record with any weight fits.
///
/// The flag is set by the preprocessor's silence detector, which tests a buffer other than the
/// one the mixer mixes (the preprocessor state's *(state + 192), not *(state + 88)); at dither
/// level that buffer can be all zero while the mixed block is not. The block is read where the
/// window left it (overlap.rs), records and elements sharing their index.
#[allow(clippy::too_many_arguments)]
extern "C" fn hook_candidate_filter(
    count: usize,
    k: usize,
    a3: usize,
    recs: usize,
    weights: usize,
    a6: usize,
    a7: usize,
    a8: usize,
    a9: usize,
    a10: usize,
) -> i64 {
    SLOT_IDS.store(
        if a7 != 0 {
            unsafe { *((a7 + STATE_SLOT_IDS) as *const usize) }
        } else {
            0
        },
        Ordering::Relaxed,
    );
    let cell = THRESHOLD_CELL.load(Ordering::Relaxed);
    if cell != 0 && recs != 0 {
        let record_size = RECORD_SIZE;
        for i in 0..count as u32 as usize {
            let flag = (recs + record_size * i + RECORD_SILENT) as *mut u32;
            if unsafe { *flag } != 0 && ELEMENT_LIVE.get().and_then(|f| f(i)) == Some(true) {
                unsafe { *flag = 0 };
                bump(6);
            }
        }
    }
    if cell != 0 && weights != 0 {
        let record_size = RECORD_SIZE;
        let (n, clusters) = (count as u32 as usize, k as u32 as usize);
        let live = unsafe {
            let (rows, cols) = (
                *(weights as *const u32) as usize,
                *((weights + 4) as *const u32) as usize,
            );
            let w = *((weights + 8) as *const *const f32);
            (0..n.min(cols))
                .filter(|&i| {
                    *((recs + record_size * i + RECORD_FLAG) as *const u32) == 0
                        && (0..rows).map(|r| *w.add(r * cols + i)).sum::<f32>() >= f32::MIN_POSITIVE
                })
                .count()
        };
        let t = if live <= clusters {
            f32::MIN_POSITIVE
        } else {
            STOCK_THRESHOLD
        };
        if live <= clusters {
            bump(3);
        }
        // The dthd Linux port reapplies RX protection after its constructors run, so make the
        // code-cave cell writable at the point of use as well as during installation.
        if !write_code(cell, &t.to_le_bytes()) {
            log::error!(target: "dshim::cluster", "threshold cell write failed");
            std::process::exit(3);
        }
    }
    let tr: Fn10 = unsafe { std::mem::transmute(FILTER_TRAMP.load(Ordering::Relaxed)) };
    tr(count, k, a3, recs, weights, a6, a7, a8, a9, a10)
}

/// object_gains(?, elements, weights, clusters, active list, active count, gains {rows, cols,
/// float *}, ...): in a frame that fits, after the original, move each owned object's column
/// onto the row of its own cluster. The column keeps its total (the snapped gain), set to exactly
/// 1 when it is 1 up to rounding (UNIT_TOLERANCE).
#[allow(clippy::too_many_arguments)]
extern "C" fn hook_object_gains(
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    active: usize,
    count: usize,
    gains: usize,
    a8: usize,
    a9: usize,
) -> i64 {
    let tr: FnGains = unsafe { std::mem::transmute(GAINS_TRAMP.load(Ordering::Relaxed)) };
    let r = tr(a1, a2, a3, a4, active, count, gains, a8, a9);
    restore_own_gains(active, count, gains);
    r
}

fn restore_own_gains(active: usize, count: usize, gains: usize) {
    let ids = SLOT_IDS.load(Ordering::Relaxed);
    if OWN_GAINS.load(Ordering::Relaxed) == 0
        || FITS.load(Ordering::Relaxed) == 0
        || ids == 0
        || gains == 0
    {
        return;
    }
    unsafe {
        let (rows, cols) = (
            *(gains as *const u32) as usize,
            *((gains + 4) as *const u32) as usize,
        );
        let g = *((gains + 8) as *const *mut f32);
        let n = (count as u32 as usize).min(rows);
        for row in 0..n {
            let slot = *(active as *const i32).add(row);
            if slot < 0 {
                continue;
            }
            let e = *(ids as *const i32).add(slot as usize);
            if e < 0 || e as usize >= cols {
                continue;
            }
            let e = e as usize;
            let mut total: f32 = (0..rows).map(|q| *g.add(q * cols + e)).sum();
            if total != 1.0 && (total - 1.0).abs() <= UNIT_TOLERANCE {
                total = 1.0;
                bump(5);
            }
            if (0..rows).any(|q| *g.add(q * cols + e) != if q == row { total } else { 0.0 }) {
                bump(8);
            }
            for q in 0..rows {
                *g.add(q * cols + e) = 0.0;
            }
            *g.add(row * cols + e) = total;
            bump(4);
        }
    }
}

/// The keep stub for macOS: the stock keep test with absolute targets, or, when the frame fits,
/// keep every candidate but the chosen one. Entered by `movabs rax; jmp rax` with the coverage
/// in xmm0 (rax is dead there: the stock code reloads it). The branch labels are inside the
/// stub; the two exits return to the original loop.
fn keep_stub_67(fits: u64, half: u64, step2: u64, next: u64, kept: u64) -> Vec<u8> {
    fn jump(b: &mut Vec<u8>, opcode: &[u8]) -> usize {
        b.extend_from_slice(opcode);
        let at = b.len();
        b.extend_from_slice(&[0; 4]);
        at
    }
    fn resolve(b: &mut [u8], at: usize, target: usize) {
        let disp = (target as i32 - (at + 4) as i32).to_le_bytes();
        b[at..at + 4].copy_from_slice(&disp);
    }
    fn absolute_jump(b: &mut Vec<u8>, target: u64) {
        b.extend_from_slice(&[0x48, 0xb8]);
        b.extend_from_slice(&target.to_le_bytes());
        b.extend_from_slice(&[0xff, 0xe0]);
    }

    let mut b = Vec::with_capacity(128);
    b.extend_from_slice(&[0x48, 0xb8]);
    b.extend_from_slice(&fits.to_le_bytes());
    b.extend_from_slice(&[0x80, 0x38, 0x00]); // cmp byte [rax], 0
    let to_fit = jump(&mut b, &[0x0f, 0x85]); // jne fit

    b.extend_from_slice(&[0x48, 0xb8]);
    b.extend_from_slice(&half.to_le_bytes());
    b.extend_from_slice(&[0x0f, 0x2e, 0x00]); // ucomiss xmm0, [rax]
    let low_coverage = jump(&mut b, &[0x0f, 0x86]); // jbe next
    b.extend_from_slice(&[0xf3, 0x0f, 0x10, 0x45, 0xb8]); // movss xmm0, [rbp-0x48]
    b.extend_from_slice(&[0x48, 0xb8]);
    b.extend_from_slice(&step2.to_le_bytes());
    b.extend_from_slice(&[0x0f, 0x2e, 0x00]); // ucomiss xmm0, [rax]
    let large_step = jump(&mut b, &[0x0f, 0x87]); // ja kept
    b.extend_from_slice(&[0x48, 0x8b, 0x85, 0x78, 0xff, 0xff, 0xff]); // mov rax, [rbp-0x88]
    b.extend_from_slice(&[0x41, 0x8b, 0x04, 0x84]); // mov eax, [r12+rax*4]
    b.extend_from_slice(&[0x41, 0x39, 0x04, 0x9c]); // cmp [r12+rbx*4], eax
    let same_slot = jump(&mut b, &[0x0f, 0x84]); // je next
    let other_slot = jump(&mut b, &[0xe9]); // jmp kept

    let fit = b.len();
    b.extend_from_slice(&[0x48, 0x3b, 0x9d, 0x78, 0xff, 0xff, 0xff]); // cmp rbx, [rbp-0x88]
    let not_chosen = jump(&mut b, &[0x0f, 0x85]); // jne kept
    let chosen = jump(&mut b, &[0xe9]); // jmp next

    let next_label = b.len();
    absolute_jump(&mut b, next);
    let kept_label = b.len();
    absolute_jump(&mut b, kept);

    for at in [low_coverage, same_slot, chosen] {
        resolve(&mut b, at, next_label);
    }
    for at in [large_step, other_slot, not_chosen] {
        resolve(&mut b, at, kept_label);
    }
    resolve(&mut b, to_fit, fit);
    b
}

// The Windows keep stub: the same test, with the coverage and distance thresholds in xmm8/xmm7.
fn keep_stub_win67(fits: u64, next: u64, kept: u64) -> Vec<u8> {
    fn branch(b: &mut Vec<u8>, op: &[u8]) -> usize {
        b.extend_from_slice(op);
        let at = b.len();
        b.extend_from_slice(&[0; 4]);
        at
    }
    fn jump(b: &mut Vec<u8>, target: u64) {
        b.extend_from_slice(&[0x48, 0xb8]);
        b.extend_from_slice(&target.to_le_bytes());
        b.extend_from_slice(&[0xff, 0xe0]);
    }

    let mut b = vec![0x48, 0xb8];
    b.extend_from_slice(&fits.to_le_bytes());
    b.extend_from_slice(&[0x80, 0x38, 0]); // cmp byte [rax], 0
    let fit = branch(&mut b, &[0x0f, 0x85]);
    b.extend_from_slice(&[0x41, 0x0f, 0x2f, 0xc0]); // comiss xmm0, xmm8
    let low = branch(&mut b, &[0x0f, 0x86]);
    b.extend_from_slice(&[0x0f, 0x2f, 0xf7]); // comiss xmm6, xmm7
    let far = branch(&mut b, &[0x0f, 0x87]);
    b.extend_from_slice(&[0x43, 0x8b, 0x44, 0xa5, 0, 0x39, 0x07]);
    let same = branch(&mut b, &[0x0f, 0x84]);
    let other = branch(&mut b, &[0xe9]);

    let fitting = b.len();
    b.extend_from_slice(&[0x44, 0x39, 0xe3]); // cmp ebx, r12d
    let chosen = branch(&mut b, &[0x0f, 0x84]);
    let keep = b.len();
    jump(&mut b, kept);
    let skip = b.len();
    jump(&mut b, next);

    for (at, target) in [
        (fit, fitting),
        (low, skip),
        (far, keep),
        (same, skip),
        (other, keep),
        (chosen, skip),
    ] {
        b[at..at + 4].copy_from_slice(&((target as i32) - (at + 4) as i32).to_le_bytes());
    }
    b
}

fn fail(feature: &str, what: &str, log: &mut dyn Write) -> ! {
    lerr!(log, "cluster", "{feature}: {what}, stopping (all or none)");
    let _ = log.flush();
    std::process::exit(3);
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
            lok!(log, "cluster", "{feature}: hook {name} @ {:#x}", r.addr);
        }
        None => fail(feature, &format!("could not detour {name}"), log),
    }
}

pub struct Options {
    pub own_clusters: bool,
    pub exact_timing: bool,
    pub keep_quiet: bool,
    pub own_gains: bool,
}

type ElementLive = fn(usize) -> Option<bool>;
static ELEMENT_LIVE: OnceLock<ElementLive> = OnceLock::new();

struct Sites<'a> {
    res: &'a Resolver,
    tool: &'static str,
}

impl Sites<'_> {
    fn name(&self, role: &str) -> String {
        format!("{}.sc.{role}", self.tool)
    }

    fn profile(&self, role: &str) -> Option<&'static Site> {
        self.res.profile.site(&self.name(role))
    }

    fn require(&self, feature: &str, roles: &[&str], log: &mut dyn Write) -> Vec<Resolved> {
        let names: Vec<_> = roles.iter().map(|role| self.name(role)).collect();
        let names: Vec<_> = names.iter().map(String::as_str).collect();
        require_all(self.res, feature, &names, log)
    }
}

pub fn install(
    res: &Resolver,
    tool: &'static str,
    options: Options,
    element_live: Option<ElementLive>,
    log: &mut dyn Write,
) -> bool {
    let Options {
        own_clusters,
        exact_timing,
        keep_quiet,
        own_gains,
    } = options;
    let sites = Sites { res, tool };
    if !(own_clusters || exact_timing || keep_quiet || own_gains) {
        return false;
    }
    if let Some(f) = element_live {
        let _ = ELEMENT_LIVE.set(f);
    }

    if sites.profile("greedy_select").is_none() {
        fail(
            "cluster pass-through",
            "no verified cluster implementation for this profile",
            log,
        );
    }

    // Resolve and byte-check every requested site before writing any.
    let select = (own_clusters || exact_timing || own_gains)
        .then(|| sites.require("cluster", &["greedy_select"], log));
    let keep_name = if sites.profile("merge_keep_fn").is_some() {
        "merge_keep_fn"
    } else if sites.profile("merge_keep_win67").is_some() {
        "merge_keep_win67"
    } else {
        "merge_keep_67"
    };
    let keep = own_clusters.then(|| sites.require("own clusters", &[keep_name], log));
    let keep_targets = (own_clusters && keep_name != "merge_keep_fn").then(|| {
        sites.require(
            "own clusters",
            &["merge_half", "merge_step", "merge_next", "merge_kept"],
            log,
        )
    });
    let activity = exact_timing.then(|| sites.require("exact timing", &["cluster_activity"], log));
    let filter =
        (keep_quiet || own_gains).then(|| sites.require("cluster", &["candidate_filter"], log));
    let quiet = keep_quiet.then(|| {
        sites.require(
            "keep quiet",
            &["candidate_threshold", "threshold_cell"],
            log,
        )
    });
    let gains = own_gains.then(|| sites.require("own gains", &["object_gains_67"], log));

    if let Some(s) = &select {
        hook(
            "cluster",
            "greedy_select",
            &s[0],
            hook_greedy_select as *const () as usize,
            &SEL_TRAMP,
            log,
        );
    }
    if let Some(s) = keep.as_ref().filter(|_| keep_name == "merge_keep_fn") {
        hook(
            "own clusters",
            keep_name,
            &s[0],
            hook_keep as *const () as usize,
            &KEEP_TRAMP,
            log,
        );
    }
    if let (Some(s), Some(targets)) = (keep.as_ref(), keep_targets.as_ref()) {
        let at = s[0].addr;
        let (half, step2, next, kept) = (
            targets[0].addr as u64,
            targets[1].addr as u64,
            targets[2].addr as u64,
            targets[3].addr as u64,
        );
        let ok =
            unsafe { *(half as *const f32) == 0.5 && *(step2 as *const f32) == 0.000_251_952_65 };
        if !ok {
            fail(
                "own clusters",
                "the keep test's constants are not 0.5 and (1/63) squared",
                log,
            );
        }
        let stub = if keep_name == "merge_keep_win67" {
            keep_stub_win67(&FITS as *const AtomicU8 as u64, next, kept)
        } else {
            keep_stub_67(&FITS as *const AtomicU8 as u64, half, step2, next, kept)
        };
        let mem = alloc_rwx(stub.len());
        if mem.is_null() {
            fail("own clusters", "no memory for the keep stub", log);
        }
        unsafe { std::ptr::copy_nonoverlapping(stub.as_ptr(), mem, stub.len()) };
        flush_icache(mem as usize, stub.len());
        let Some(len) = sites.profile(keep_name).map(|site| hex(site.expect).len()) else {
            fail("own clusters", "no keep decision site", log)
        };
        let mut jump = vec![0x90u8; len];
        jump[..2].copy_from_slice(&[0x48, 0xb8]);
        jump[2..10].copy_from_slice(&(mem as u64).to_le_bytes());
        jump[10..12].copy_from_slice(&[0xff, 0xe0]);
        if !write_code(at, &jump) {
            fail("own clusters", "could not write cluster keep decision", log);
        }
        lok!(
            log,
            "cluster",
            "own clusters: cluster keep decision @ {at:#x} jumps to the keep stub @ {mem:p}"
        );
    }
    if let Some(s) = &activity {
        EXACT_TIMING.store(1, Ordering::Relaxed);
        hook(
            "exact timing",
            "cluster_activity",
            &s[0],
            hook_cluster_activity as *const () as usize,
            &ACT_TRAMP,
            log,
        );
    }
    if let Some(s) = &quiet {
        // The cell first, so the retargeted load never reads anything but a threshold.
        let cell = s[1].addr;
        if !write_code(cell, &STOCK_THRESHOLD.to_le_bytes()) {
            fail("keep quiet", "could not write threshold cell", log);
        }
        let Kind::Patch { new } = s[0].kind else {
            fail("keep quiet", "threshold is not a patch site", log)
        };
        let Ok(disp) = <[u8; 4]>::try_from(hex(new)) else {
            fail("keep quiet", "threshold patch is not a disp32", log)
        };
        if (s[0].addr as i64 + 4 + i32::from_le_bytes(disp) as i64) as usize != cell {
            fail(
                "keep quiet",
                "the threshold patch does not point at threshold cell",
                log,
            );
        }
        if !write_code(s[0].addr, &hex(new)) {
            fail("keep quiet", "could not write candidate threshold", log);
        }
        THRESHOLD_CELL.store(cell, Ordering::Relaxed);
        lok!(
            log,
            "cluster",
            "keep quiet: patch candidate threshold, threshold cell @ {cell:#x}"
        );
    }
    if let Some(s) = &filter {
        hook(
            "cluster",
            "candidate_filter",
            &s[0],
            hook_candidate_filter as *const () as usize,
            &FILTER_TRAMP,
            log,
        );
    }
    if let Some(s) = &gains {
        OWN_GAINS.store(1, Ordering::Relaxed);
        hook(
            "own gains",
            "object_gains_67",
            &s[0],
            hook_object_gains as *const () as usize,
            &GAINS_TRAMP,
            log,
        );
    }
    unsafe { atexit(report) };
    keep_quiet
}
