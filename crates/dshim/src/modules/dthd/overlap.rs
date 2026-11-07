//! Exact overlap for dee_dthd_encoder.
//!
//! dthd's clusterer works on blocks of two frames hopping by one: each element block is
//! multiplied by a triangular window (dthd.sc.window), the mixer mixes the windowed blocks
//! (dthd.sc.mixer), and the overlap-add (dthd.sc.overlap_add) sums the first half of each cluster
//! block with the second half of the previous one. The window halves sum to 1, but in float
//! x*w + x*(1-w) is not always x, so even an object that owns its cluster comes out up to 1 LSB
//! off on samples at or above a quarter of full scale.
//!
//! With the switch on, a cluster gets an element's own samples instead of the sum, which is what
//! the sum is in exact arithmetic, when across the overlap it carries that element alone at gain
//! 1 in both blocks, or in one block while the other leaves the cluster empty and mixes the
//! element nowhere (the element starts or stops, or the encode ends: DEE mixes nothing into the
//! block after the last frame, which leaves the last part frame at the window's falling half).
//! The samples are the second half of the element's previous block before windowing: the first
//! half of the current block is the same samples, but for the block after the last frame, which
//! DEE leaves empty.
//!
//! Every other cluster (a real mix, a gain other than 1, an element that changed cluster) keeps
//! the stock crossfade, so the switch changes nothing unless the clusters pass objects through
//! (passthrough.rs).
//!
//! The window hook also keeps where the element blocks are, for the pass-through's keep quiet
//! (passthrough.rs), which needs to know whether an element's block is all zero; it is installed
//! when either needs it.
//!
//! The clusterer runs on one thread per encode; the state sits behind a mutex all the same.

use crate::detour_install;
use crate::modules::common::require_all;
use crate::site::{Resolved, Resolver};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::Mutex;

type FnWindow = extern "C" fn(usize, u32, usize);
type FnMixer = extern "C" fn(u32, usize, usize, usize, usize, u32) -> i64;
type FnOverlap = extern "C" fn(usize, u32, u32, usize, usize);

static WINDOW_TRAMP: AtomicUsize = AtomicUsize::new(0);
static MIXER_TRAMP: AtomicUsize = AtomicUsize::new(0);
static OVERLAP_TRAMP: AtomicUsize = AtomicUsize::new(0);
// set when the exact overlap is on: the window hook copies the blocks
static EXACT: AtomicU8 = AtomicU8::new(0);
// the element buffers of the last windowing, their count and the half length N
static ELEMS: AtomicUsize = AtomicUsize::new(0);
static ELEM_COUNT: AtomicUsize = AtomicUsize::new(0);
static HALF: AtomicUsize = AtomicUsize::new(0);
// counts for the exit line: overlaps, cluster halves restored
static STATS: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

/// What a cluster carries in one block.
#[derive(Clone, Copy, PartialEq)]
enum Carry {
    Empty,
    /// one element buffer alone, at gain 1
    One(usize),
    /// anything else
    Mix,
}

/// One block's mix into a cluster buffer array.
#[derive(Clone)]
struct Mixed {
    carry: Vec<Carry>,
    // every element buffer mixed anywhere, at any positive gain
    used: HashSet<usize>,
}

impl Mixed {
    /// The element a cluster should carry across an overlap of this block (after) and `before`.
    fn across(&self, before: &Mixed, c: usize) -> Option<usize> {
        match (before.carry.get(c)?, self.carry.get(c)?) {
            (Carry::One(p), Carry::One(q)) if p == q => Some(*p),
            (Carry::One(p), Carry::Empty) if !self.used.contains(p) => Some(*p),
            (Carry::Empty, Carry::One(q)) if !before.used.contains(q) => Some(*q),
            _ => None,
        }
    }
}

#[derive(Default)]
struct State {
    // windowings so far
    frame: u64,
    // element buffer -> (frame, the second half of its block before windowing); used at the
    // overlap-add of the frame after
    raw: HashMap<usize, (u64, Vec<f32>)>,
    // the same, taken in the current frame, for the overlap-add of the next
    next: HashMap<usize, (u64, Vec<f32>)>,
    // cluster buffer array -> the last mix into it since its last overlap-add
    mix: HashMap<usize, Mixed>,
    // cluster buffer array -> the mix of its previous overlap-add
    prev: HashMap<usize, Mixed>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(State::default))
}

extern "C" {
    fn atexit(cb: extern "C" fn()) -> i32;
}

extern "C" fn report() {
    log::debug!(target: "dshim::dthd",
        "exact overlap, overlaps {}, cluster halves restored {}",
        STATS[0].load(Ordering::Relaxed),
        STATS[1].load(Ordering::Relaxed)
    );
}

/// Whether element `i`'s block (2N samples, as windowed in this frame) has a sample other than 0;
/// None before the first windowing or past the element count.
pub fn element_live(i: usize) -> Option<bool> {
    let (elems, n) = (ELEMS.load(Ordering::Relaxed), HALF.load(Ordering::Relaxed));
    if elems == 0 || i >= ELEM_COUNT.load(Ordering::Relaxed) {
        return None;
    }
    let p = unsafe { *(elems as *const *const f32).add(i) };
    if p.is_null() {
        return None;
    }
    // the window is 0 only at the block's last sample, so a windowed block is all zero exactly
    // when the element's samples are, but for that one
    Some(
        unsafe { std::slice::from_raw_parts(p, 2 * n) }
            .iter()
            .any(|v| *v != 0.0),
    )
}

/// window(window {u32 N, pad, float *}, count, element buffers): note the buffers; with the exact
/// overlap, keep the second half of each element's block. Then let the original window them.
extern "C" fn hook_window(win: usize, count: u32, elems: usize) {
    if win != 0 && elems != 0 {
        let n = unsafe { *(win as *const u32) } as usize;
        ELEMS.store(elems, Ordering::Relaxed);
        ELEM_COUNT.store(count as usize, Ordering::Relaxed);
        HALF.store(n, Ordering::Relaxed);
        if EXACT.load(Ordering::Relaxed) != 0 {
            with_state(|s| {
                s.frame += 1;
                let frame = s.frame;
                // what the coming overlap-add needs is the copy of the previous frame
                let mut keep: HashMap<usize, (u64, Vec<f32>)> = HashMap::new();
                for e in 0..count as usize {
                    let p = unsafe { *(elems as *const *const f32).add(e) };
                    if p.is_null() {
                        continue;
                    }
                    let src = unsafe { std::slice::from_raw_parts(p.add(n), n) };
                    keep.insert(p as usize, (frame, src.to_vec()));
                }
                s.next = keep;
            });
        }
    }
    let tr: FnWindow = unsafe { std::mem::transmute(WINDOW_TRAMP.load(Ordering::Relaxed)) };
    tr(win, count, elems)
}

/// mixer(samples, element buffers, active slots, gains {rows, cols, float *}, cluster buffers,
/// cluster count): after the original, note what each cluster carries.
extern "C" fn hook_mixer(
    n: u32,
    elems: usize,
    active: usize,
    gains: usize,
    outs: usize,
    k: u32,
) -> i64 {
    let tr: FnMixer = unsafe { std::mem::transmute(MIXER_TRAMP.load(Ordering::Relaxed)) };
    let r = tr(n, elems, active, gains, outs, k);
    if elems == 0 || active == 0 || gains == 0 || outs == 0 {
        return r;
    }
    let mut m = Mixed {
        carry: vec![Carry::Empty; k as usize],
        used: HashSet::new(),
    };
    unsafe {
        let (rows, cols) = (
            *(gains as *const u32) as usize,
            *((gains + 4) as *const u32) as usize,
        );
        let g = *((gains + 8) as *const *const f32);
        for row in 0..rows {
            let slot = *(active as *const i32).add(row);
            if slot < 0 || slot as usize >= m.carry.len() {
                continue;
            }
            for e in 0..cols {
                // the mixer adds only positive gains
                let v = *g.add(row * cols + e);
                if v.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
                    continue;
                }
                let p = *(elems as *const usize).add(e);
                m.used.insert(p);
                let c = &mut m.carry[slot as usize];
                *c = match c {
                    Carry::Empty if v == 1.0 => Carry::One(p),
                    _ => Carry::Mix,
                };
            }
        }
    }
    with_state(|s| {
        s.mix.insert(outs, m);
    });
    r
}

/// overlap_add(cluster buffers, count, N, tails, out): after the original, restore each cluster
/// that carries one element across this overlap (Mixed::across).
extern "C" fn hook_overlap(cur: usize, count: u32, n: u32, tail: usize, out: usize) {
    let tr: FnOverlap = unsafe { std::mem::transmute(OVERLAP_TRAMP.load(Ordering::Relaxed)) };
    tr(cur, count, n, tail, out);
    if cur == 0 || out == 0 {
        return;
    }
    let n = n as usize;
    STATS[0].fetch_add(1, Ordering::Relaxed);
    with_state(|s| {
        // this frame's copies wait for the next overlap-add; the previous frame's serve this one
        let taken = std::mem::take(&mut s.next);
        let prev_raw = std::mem::replace(&mut s.raw, taken);
        // a block the mixer did not see since the last overlap-add is not known: no restoring
        // here or at the next one
        let Some(now) = s.mix.remove(&cur) else {
            s.prev.remove(&cur);
            return;
        };
        if let Some(before) = s.prev.get(&cur) {
            for c in 0..count as usize {
                let Some(p) = now.across(before, c) else {
                    continue;
                };
                let Some((frame, raw)) = prev_raw.get(&p) else {
                    continue;
                };
                if *frame + 1 != s.frame || raw.len() < n {
                    continue;
                }
                let dst = unsafe { *(out as *const *mut f32).add(c) };
                if dst.is_null() {
                    continue;
                }
                unsafe { std::ptr::copy_nonoverlapping(raw.as_ptr(), dst, n) };
                STATS[1].fetch_add(1, Ordering::Relaxed);
            }
        }
        s.prev.insert(cur, now);
    });
}

fn hook(name: &str, r: &Resolved, f: usize, tramp: &AtomicUsize, log: &mut dyn Write) {
    match detour_install(r.addr, r.steal, f) {
        Some(t) => {
            tramp.store(t, Ordering::Relaxed);
            lok!(log, "dthd", "element blocks: hook {name} @ {:#x}", r.addr);
        }
        None => {
            lerr!(
                log,
                "dthd",
                "element blocks: {name} detour failed, stopping (all or none)"
            );
            let _ = log.flush();
            std::process::exit(3);
        }
    }
}

/// `keep_quiet`: the pass-through's keep quiet is on and reads the element blocks.
pub fn install(res: &Resolver, keep_quiet: bool, log: &mut dyn Write) {
    let exact = crate::levers().dthd.audio_exact_overlap();
    if !(exact || keep_quiet) {
        return;
    }
    let names: &[&str] = if exact {
        &["dthd.sc.window", "dthd.sc.mixer", "dthd.sc.overlap_add"]
    } else {
        &["dthd.sc.window"]
    };
    // Resolve and byte-check every site before hooking any.
    let s = require_all(res, "element blocks", names, log);
    if exact {
        EXACT.store(1, Ordering::Relaxed);
    }
    hook(
        names[0],
        &s[0],
        hook_window as *const () as usize,
        &WINDOW_TRAMP,
        log,
    );
    if exact {
        hook(
            names[1],
            &s[1],
            hook_mixer as *const () as usize,
            &MIXER_TRAMP,
            log,
        );
        hook(
            names[2],
            &s[2],
            hook_overlap as *const () as usize,
            &OVERLAP_TRAMP,
            log,
        );
        unsafe { atexit(report) };
    }
}
