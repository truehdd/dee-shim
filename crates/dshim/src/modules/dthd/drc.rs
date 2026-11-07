//! DRC profiles for dee_dthd_encoder's presentations, `none` included.
//!
//! The CLI sets a presentation's profile with `--presentation <type>:drc_profile=`, five profiles
//! and not_indicated (which the .mll writes as Film-Light); there is no `none`. The TrueHD core
//! builds a DRC curve per presentation with the DRC profile builder (genre, -dialnorm), and asks
//! the curve's gain computer for each gain (level, ceiling), which returns the curve's gain
//! capped at the ceiling, the most gain the block's peak allows (-20 log10 peak - 0.15 dB). The
//! engine picks the genre per presentation at the first; for `none` it answers the second with
//! 0 dB capped at the ceiling: no compression, only the peak protection, as dee_ddpjoc_encoder's
//! own `none` keeps it.
//!
//!   DTHD_DRC=<profile>                  every presentation
//!   DTHD_DRC=<type>=<profile>,...       per presentation; type 2ch, 6ch, 8ch or atmos
//!
//! Profiles: film_standard, film_light, music_standard, music_light, speech, none. An entry
//! without a type covers the presentations no typed entry names. The 2ch presentation's
//! drc_default_on (whether a decoder applies DRC by default) stays DEE's.
//!
//! The profile builder does not receive the presentation; its one caller, the encoder parameter
//! setup, holds it in a callee-saved register at the call (r13, or r14 on Windows), so a stub
//! passes it on as the hook's second argument. The hook uses it only when it is one of the
//! options' four presentations and holds the genre passed.

use crate::detour_install;
use crate::modules::common::require_all;
use crate::site::{Resolved, Resolver};
use dshim_levers::{Profile, DTHD_TYPES};
use std::collections::HashSet;
use std::io::Write;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

type FnMake = extern "C" fn(i32, f64) -> usize;
type FnCompute = extern "C" fn(usize, f64, f64, usize) -> f64;

static MAKE_TRAMP: AtomicUsize = AtomicUsize::new(0);
static COMPUTE_TRAMP: AtomicUsize = AtomicUsize::new(0);

/// The TrueHD core's genre of a profile; None is no DRC.
fn genre(profile: Profile) -> Option<i32> {
    match profile {
        Profile::FilmStandard => Some(0),
        Profile::FilmLight => Some(1),
        Profile::MusicStandard => Some(2),
        Profile::MusicLight => Some(3),
        Profile::Speech => Some(4),
        Profile::None => None,
    }
}

/// Per presentation type: None leaves DEE's profile, Some(None) is no DRC.
static CHOICE: Mutex<[Option<Option<i32>>; 4]> = Mutex::new([None; 4]);
// Curves made for a presentation set to none.
static NONE_CURVES: Mutex<Option<HashSet<usize>>> = Mutex::new(None);
// Counts for the exit line: curves made, curves set, gains answered for none, of them cut.
static STATS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

// Presentation +OPTIONS points at its options, which store the 2ch, 6ch, 8ch and 16ch presentation
// pointers in that order at +SLOTS. The Windows offsets are selected at install.
const OPTIONS: usize = 0x68;
const SLOTS: usize = 3032;
static LAYOUT: OnceLock<(usize, usize)> = OnceLock::new();
// Presentation +8: the presentation's DRC genre (its <dynamic-range-compression>).
const GENRE: usize = 8;

extern "C" {
    fn atexit(cb: extern "C" fn()) -> i32;
}

extern "C" fn report() {
    log::debug!(target: "dshim::dthd",
        "drc curves made {}, set {}, none gains {} (cut by the ceiling {})",
        STATS[0].load(Ordering::Relaxed),
        STATS[1].load(Ordering::Relaxed),
        STATS[2].load(Ordering::Relaxed),
        STATS[3].load(Ordering::Relaxed)
    );
}

/// The presentation's type index, if `p` is one of its options' four presentations.
fn presentation_type(p: usize) -> Option<usize> {
    if p == 0 || !p.is_multiple_of(8) {
        return None;
    }
    let &(options, slots) = LAYOUT.get().expect("DRC layout initialized");
    unsafe {
        let opts = *((p + options) as *const usize);
        if opts == 0 || !opts.is_multiple_of(8) {
            return None;
        }
        (0..4).find(|&k| *((opts + slots + 8 * k) as *const usize) == p)
    }
}

// profile builder(genre, level) entered through the stub, which adds the caller's presentation
// as the hook's middle argument. On System V (macOS and the Linux port) the presentation is in r13
// and the level stays in xmm0 (the first float register), so the stub only sets rsi. On Win64 the
// presentation is in r14 at the call and the arguments are positional across the integer and
// float files, so the level, incoming in xmm1, moves to xmm2 (hook_make's third parameter) while
// rdx takes the presentation.
#[cfg(target_os = "macos")]
std::arch::global_asm!(
    ".globl _dshim_dthd_make_profile_stub",
    "_dshim_dthd_make_profile_stub:",
    "mov rsi, r13",
    "jmp {hook}",
    hook = sym hook_make,
);
#[cfg(windows)]
std::arch::global_asm!(
    ".globl dshim_dthd_make_profile_stub",
    "dshim_dthd_make_profile_stub:",
    "movaps xmm2, xmm1",
    "mov rdx, r14",
    "jmp {hook}",
    hook = sym hook_make,
);
#[cfg(all(not(target_os = "macos"), not(windows)))]
std::arch::global_asm!(
    ".globl dshim_dthd_make_profile_stub",
    "dshim_dthd_make_profile_stub:",
    "mov rsi, r13",
    "jmp {hook}",
    hook = sym hook_make,
);

extern "C" {
    fn dshim_dthd_make_profile_stub();
}

/// profile builder(genre, level), with the caller's presentation: make the curve with the
/// genre set for the presentation's type, and note a curve made for `none`.
extern "C" fn hook_make(genre: i32, pres: usize, level: f64) -> usize {
    let tr: FnMake = unsafe { std::mem::transmute(MAKE_TRAMP.load(Ordering::Relaxed)) };
    STATS[0].fetch_add(1, Ordering::Relaxed);
    // `pres` is the presentation when it is one of the options' four and holds the genre passed.
    let ty =
        presentation_type(pres).filter(|_| unsafe { *((pres + GENRE) as *const i32) } == genre);
    let choice = ty.and_then(|k| CHOICE.lock().unwrap_or_else(|e| e.into_inner())[k]);
    match choice {
        None => tr(genre, level),
        Some(Some(g)) => {
            STATS[1].fetch_add(1, Ordering::Relaxed);
            tr(g, level)
        }
        Some(None) => {
            STATS[1].fetch_add(1, Ordering::Relaxed);
            // Keep the presentation's own genre: the curve exists but is never asked.
            let curve = tr(genre, level);
            NONE_CURVES
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_with(HashSet::new)
                .insert(curve);
            curve
        }
    }
}

/// gain computer(curve, level, ceiling, flag): for a curve made for `none`, 0 dB capped at the
/// ceiling.
extern "C" fn hook_compute(curve: usize, level: f64, ceiling: f64, flag: usize) -> f64 {
    let none = NONE_CURVES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|s| s.contains(&curve));
    if none {
        STATS[2].fetch_add(1, Ordering::Relaxed);
        if ceiling < 0.0 {
            STATS[3].fetch_add(1, Ordering::Relaxed);
        }
        return ceiling.min(0.0);
    }
    let tr: FnCompute = unsafe { std::mem::transmute(COMPUTE_TRAMP.load(Ordering::Relaxed)) };
    tr(curve, level, ceiling, flag)
}

fn fail(msg: &str, log: &mut dyn Write) -> ! {
    lerr!(log, "dthd", "drc: {msg}, stopping");
    let _ = log.flush();
    std::process::exit(3);
}

fn hook(name: &str, r: &Resolved, f: usize, tramp: &AtomicUsize, log: &mut dyn Write) {
    match detour_install(r.addr, r.steal, f) {
        Some(t) => {
            tramp.store(t, Ordering::Relaxed);
            lok!(log, "dthd", "drc: hook {name} @ {:#x}", r.addr);
        }
        None => fail(&format!("could not detour {name}"), log),
    }
}

pub fn install(res: &Resolver, log: &mut dyn Write) {
    // DTHD_TYPES is in the TrueHD core's order (the presentation type index).
    let Some(drc) = &crate::levers().dthd.drc else {
        return;
    };
    let profiles = drc.choice();
    if profiles.iter().all(Option::is_none) {
        return;
    }
    for (t, p) in DTHD_TYPES.iter().zip(profiles) {
        if let Some(p) = p {
            lok!(log, "dthd", "drc: {t} presentation {}", p.name());
        }
    }
    let choice = profiles.map(|p| p.map(genre));
    let layout = match (res.profile.os, res.profile.version) {
        (dshim_profiles::Os::Windows, "6.7.0") => (0x60, 0xCB8),
        _ => (OPTIONS, SLOTS),
    };
    let _ = LAYOUT.set(layout);
    *CHOICE.lock().unwrap_or_else(|e| e.into_inner()) = choice;
    // Resolve and byte-check both sites before hooking either.
    let s = require_all(
        res,
        "drc",
        &["dthd.drc.make_profile", "dthd.drc.compute"],
        log,
    );
    hook(
        "dthd.drc.make_profile",
        &s[0],
        dshim_dthd_make_profile_stub as *const () as usize,
        &MAKE_TRAMP,
        log,
    );
    hook(
        "dthd.drc.compute",
        &s[1],
        hook_compute as *const () as usize,
        &COMPUTE_TRAMP,
        log,
    );
    unsafe { atexit(report) };
}
