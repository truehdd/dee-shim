//! Output length for dee_dthd_encoder.
//!
//! DEE's Atmos reader pads what it reads to a whole 512-sample block, and the TrueHD encoder
//! would end the stream there. This module ends the encoder's audio at the count the reader's
//! storage holds, without the padding, so the stream ends with the master's last sample.

use crate::modules::common::require_all;
use crate::site::Resolver;
use std::io::Write;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// The reader's implementation, and in it the master storage it reads.
const IMPL: usize = 8;
const STORAGE: usize = 0x70;

static OPEN: AtomicUsize = AtomicUsize::new(0);
static COUNT: AtomicUsize = AtomicUsize::new(0);
static READ: AtomicUsize = AtomicUsize::new(0);
/// The samples the encode holds (the range read, with any added silence), unpadded.
static END: AtomicU64 = AtomicU64::new(0);

type Open = extern "C" fn(usize, usize) -> usize;
type Count = extern "C" fn(usize) -> u64;
type Read = extern "C" fn(usize, usize, *mut u8) -> u32;

/// The reader is open: ask its storage for the count it padded.
extern "C" fn open(reader: usize, config: usize) -> usize {
    let original: Open = unsafe { std::mem::transmute(OPEN.load(Ordering::Relaxed)) };
    let r = original(reader, config);
    let count: Count = unsafe { std::mem::transmute(COUNT.load(Ordering::Relaxed)) };
    let storage = unsafe { *((*((reader + IMPL) as *const usize) + STORAGE) as *const usize) };
    END.store(count(storage), Ordering::Relaxed);
    r
}

/// The samples a read of `n` at `pos` keeps when the audio ends at `end`, if it reaches it.
fn cut(pos: u64, n: u32, end: u64) -> Option<u32> {
    (end != 0 && pos + u64::from(n) >= end).then(|| end.saturating_sub(pos) as u32)
}

/// The encoder's audio supply: `buf` holds the position at +8; the end flag goes in `eos`.
extern "C" fn read(this: usize, buf: usize, eos: *mut u8) -> u32 {
    let original: Read = unsafe { std::mem::transmute(READ.load(Ordering::Relaxed)) };
    let n = original(this, buf, eos);
    let pos = unsafe { *((buf + 8) as *const u64) };
    match cut(pos, n, END.load(Ordering::Relaxed)) {
        Some(kept) => {
            unsafe { *eos = 1 };
            kept
        }
        None => n,
    }
}

pub fn install(res: &Resolver, log: &mut dyn Write) {
    if !crate::levers().dthd.audio_exact_length() {
        return;
    }
    let hooks: [(&str, usize, &AtomicUsize); 2] = [
        ("dthd.reader.open", open as *const () as usize, &OPEN),
        ("dthd.core.read_audio", read as *const () as usize, &READ),
    ];
    let mut names: Vec<&str> = hooks.iter().map(|h| h.0).collect();
    names.push("dthd.storage.sample_count");
    if names.iter().all(|name| res.profile.site(name).is_none()) {
        ldebug!(
            log,
            "dthd",
            "exact length: no sites in this profile, skipped"
        );
        return;
    }
    let sites = require_all(res, "dthd exact length", &names, log);
    COUNT.store(sites[2].addr, Ordering::Relaxed);
    for ((name, hook, tramp), site) in hooks.iter().zip(&sites) {
        match site.detour(*hook) {
            Some(t) => {
                tramp.store(t, Ordering::Relaxed);
                lok!(log, "dthd", "exact length: hook {name} @ {:#x}", site.addr);
            }
            None => {
                lerr!(
                    log,
                    "dthd",
                    "exact length: could not detour {name}, stopping"
                );
                let _ = log.flush();
                std::process::exit(3);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::cut;

    #[test]
    fn the_read_that_reaches_the_end_is_cut_there() {
        assert_eq!(cut(92760, 3496, 96001), Some(3241));
        assert_eq!(cut(87920, 4840, 96001), None);
        // No count seen: DEE's end.
        assert_eq!(cut(92760, 3496, 0), None);
    }
}
