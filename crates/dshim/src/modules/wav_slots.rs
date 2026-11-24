//! A WAV input that holds only its layout's channels, for dee_dthd_encoder and dee_ddp_encoder.
//!
//! DEE reads a WAV by position and wants one channel per input slot, so a layout with empty
//! slots needs a padded file. Here the WAV reader takes a file with only the named slots: it
//! reports the padded channel count, and each read moves the file's channels to their slots and
//! clears the rest. Everything after the reader sees the padded file.
//!
//! The input's channel count is also in the source info DEE collects before it opens the reader.
//! dee_dthd_encoder only checks it against the presentation, so that check lets these inputs
//! pass. dee_ddp_encoder builds its job from it, so there the source info gets the padded count.
//!
//! DEE's check, constructor and read throw C++ exceptions, which must not meet a Rust frame.
//! Each of those hooks is entered through a stub that asks whether the call is for one of these
//! inputs and otherwise jumps to the original.

use crate::modules::common::require_all;
use crate::site::Resolver;
use dshim_levers::{wav_format, WavSlots};
use std::ffi::CStr;
use std::io::Write;
use std::os::raw::c_char;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

/// The file source's channel count.
const CHANNELS: usize = 8;

static INPUTS: OnceLock<Vec<WavSlots>> = OnceLock::new();
/// The open file sources that read such an input, with its index in INPUTS.
static SOURCES: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());
static CHECK: AtomicUsize = AtomicUsize::new(0);
static OPEN: AtomicUsize = AtomicUsize::new(0);
static READ: AtomicUsize = AtomicUsize::new(0);
static INFO: AtomicUsize = AtomicUsize::new(0);

type Info = extern "C" fn(usize, usize, usize) -> usize;
type Open = extern "C" fn(usize, *const c_char, u64, u64) -> usize;
type Read = extern "C" fn(usize, *const *mut f32, u64) -> u64;

// Ask `ours` with the hook's own arguments; then run `taken`, or the original.
macro_rules! stub {
    ($name:literal, $ours:ident, $taken:ident, $original:ident) => {
        std::arch::global_asm!(
            concat!(".globl ", $name),
            concat!($name, ":"),
            "push rdi",
            "push rsi",
            "push rdx",
            "push rcx",
            "push r8",
            "push r9",
            "sub rsp, 40",
            "call {ours}",
            "add rsp, 40",
            "pop r9",
            "pop r8",
            "pop rcx",
            "pop rdx",
            "pop rsi",
            "pop rdi",
            "test al, al",
            "jnz 2f",
            "jmp qword ptr [rip + {original}]",
            "2:",
            "jmp {taken}",
            ours = sym $ours,
            taken = sym $taken,
            original = sym $original,
        );
    };
}
#[cfg(target_os = "macos")]
stub!("_dshim_wav_check_stub", ours_count, pass, CHECK);
#[cfg(target_os = "macos")]
stub!("_dshim_wav_open_stub", ours_file, open, OPEN);
#[cfg(target_os = "macos")]
stub!("_dshim_wav_read_stub", ours_source, read, READ);
#[cfg(not(target_os = "macos"))]
stub!("dshim_wav_check_stub", ours_count, pass, CHECK);
#[cfg(not(target_os = "macos"))]
stub!("dshim_wav_open_stub", ours_file, open, OPEN);
#[cfg(not(target_os = "macos"))]
stub!("dshim_wav_read_stub", ours_source, read, READ);

extern "C" {
    fn dshim_wav_check_stub();
    fn dshim_wav_open_stub();
    fn dshim_wav_read_stub();
}

fn inputs() -> &'static [WavSlots] {
    INPUTS.get().map_or(&[], Vec::as_slice)
}

fn sources() -> std::sync::MutexGuard<'static, Vec<(usize, usize)>> {
    SOURCES.lock().unwrap_or_else(|e| e.into_inner())
}

/// The input check (count, allowed counts): a count one of the inputs has is theirs.
extern "C" fn ours_count(count: i32) -> bool {
    inputs().iter().any(|w| w.slots.len() as i32 == count)
}

extern "C" fn pass() -> usize {
    0
}

/// Give each input that is one file of such a count the padded count. `list` is a vector of
/// vectors of source info; one is 88 or 96 bytes by the build's string type, and its channel
/// count sits 64 bytes before its end.
///
/// # Safety
/// `list` points at the source info list DEE just built.
unsafe fn pad_counts(list: usize, inputs: &[WavSlots]) {
    let (mut input, end) = (*(list as *const usize), *((list + 8) as *const usize));
    while input < end {
        let (first, last) = (*(input as *const usize), *((input + 8) as *const usize));
        if matches!(last - first, 88 | 96) {
            let count = (last - 64) as *mut i32;
            let channels = count.read_unaligned();
            if let Some(w) = inputs.iter().find(|w| w.slots.len() as i32 == channels) {
                count.write_unaligned(w.total as i32);
            }
        }
        input += 24;
    }
}

/// getSourceInfo (result, input, reporter). It throws only for an input it cannot read, and the
/// CLI has read these.
extern "C" fn source_info(out: usize, input: usize, reporter: usize) -> usize {
    let original: Info = unsafe { std::mem::transmute(INFO.load(Ordering::Relaxed)) };
    let r = original(out, input, reporter);
    unsafe { pad_counts(out, inputs()) };
    r
}

/// The input a file of `channels` at `path` is: the one named by path, or else the only one
/// with that many channels (DEE may spell the path differently). A mono file can also be a
/// wav_list entry, so it is matched by path alone.
fn input_for(list: &[WavSlots], path: &str, channels: usize) -> Option<usize> {
    let fits = |w: &&WavSlots| w.slots.len() == channels;
    if let Some(at) = list.iter().position(|w| fits(&w) && w.path == path) {
        return Some(at);
    }
    let mut same = list.iter().enumerate().filter(|(_, w)| fits(w));
    match (same.next(), same.next()) {
        (Some((at, _)), None) if channels > 1 => Some(at),
        _ => None,
    }
}

fn input_at(path: *const c_char) -> Option<usize> {
    let path = unsafe { CStr::from_ptr(path) }.to_string_lossy();
    let (channels, _) = wav_format(&path)?;
    input_for(inputs(), &path, channels as usize)
}

/// The file source's constructor (this, path, start, end): a readable WAV of one of the inputs.
extern "C" fn ours_file(_this: usize, path: *const c_char) -> bool {
    input_at(path).is_some()
}

/// Report the padded count.
extern "C" fn open(this: usize, path: *const c_char, start: u64, end: u64) -> usize {
    let original: Open = unsafe { std::mem::transmute(OPEN.load(Ordering::Relaxed)) };
    let r = original(this, path, start, end);
    let mut sources = sources();
    sources.retain(|(source, _)| *source != this);
    if let Some(at) = input_at(path) {
        unsafe { *((this + CHANNELS) as *mut u32) = inputs()[at].total as u32 };
        sources.push((this, at));
    }
    r
}

/// The file source's read (this, buffers, max frames).
extern "C" fn ours_source(this: usize) -> bool {
    sources().iter().any(|(source, _)| *source == this)
}

/// Move channel i of `frames` samples to `slots[i]`, highest first, and clear the other slots.
///
/// # Safety
/// `channels` holds `total` pointers to at least `frames` samples each.
unsafe fn spread(channels: *const *mut f32, total: usize, slots: &[usize], frames: usize) {
    for (i, &slot) in slots.iter().enumerate().rev() {
        if slot != i {
            std::ptr::copy_nonoverlapping(*channels.add(i), *channels.add(slot), frames);
        }
    }
    for slot in (0..total).filter(|slot| !slots.contains(slot)) {
        std::ptr::write_bytes(*channels.add(slot), 0, frames);
    }
}

/// One float buffer per channel; DEE fills the file's channels into the first ones.
extern "C" fn read(this: usize, channels: *const *mut f32, max: u64) -> u64 {
    let original: Read = unsafe { std::mem::transmute(READ.load(Ordering::Relaxed)) };
    let frames = original(this, channels, max);
    let at = sources()
        .iter()
        .find(|(source, _)| *source == this)
        .map(|s| s.1);
    if let Some(input) = at.and_then(|at| inputs().get(at)) {
        unsafe { spread(channels, input.total, &input.slots, frames as usize) };
    }
    frames
}

pub fn install(res: &Resolver, tool: &str, list: &[WavSlots], log: &mut dyn Write) {
    if list.is_empty() {
        return;
    }
    let count: (&str, usize, &AtomicUsize) = if tool == "ddp" {
        ("wav.source_info", source_info as *const () as usize, &INFO)
    } else {
        (
            "wav.input.count_check",
            dshim_wav_check_stub as *const () as usize,
            &CHECK,
        )
    };
    let hooks: [(&str, usize, &AtomicUsize); 3] = [
        count,
        (
            "wav.source.open",
            dshim_wav_open_stub as *const () as usize,
            &OPEN,
        ),
        (
            "wav.source.read",
            dshim_wav_read_stub as *const () as usize,
            &READ,
        ),
    ];
    let names: Vec<String> = hooks.iter().map(|h| format!("{tool}.{}", h.0)).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let sites = require_all(res, "wav slots", &names, log);
    let _ = INPUTS.set(list.to_vec());
    for ((name, hook, tramp), site) in hooks.iter().zip(&sites) {
        match site.detour(*hook) {
            Some(t) => {
                tramp.store(t, Ordering::Relaxed);
                lok!(log, tool, "wav slots: hook {name} @ {:#x}", site.addr);
            }
            None => {
                lerr!(log, tool, "wav slots: could not detour {name}, stopping");
                let _ = log.flush();
                std::process::exit(3);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(total: usize, slots: &[usize], path: &str) -> WavSlots {
        WavSlots {
            total,
            slots: slots.to_vec(),
            path: path.to_string(),
        }
    }

    #[test]
    fn channels_move_to_their_slots_and_the_rest_is_silent() {
        let mut data: Vec<Vec<f32>> = (0..8).map(|c| vec![c as f32 + 1.0; 3]).collect();
        let pointers: Vec<*mut f32> = data.iter_mut().map(|c| c.as_mut_ptr()).collect();
        // L R C Cs of 4.0 in an 8-slot presentation; the third frame was not read.
        unsafe { spread(pointers.as_ptr(), 8, &[0, 1, 2, 4], 2) };
        let first: Vec<f32> = data.iter().map(|c| c[0]).collect();
        assert_eq!(first, [1.0, 2.0, 3.0, 0.0, 4.0, 0.0, 0.0, 0.0]);
        assert_eq!(data[3][2], 4.0);
    }

    #[test]
    fn a_single_file_input_gets_the_padded_count() {
        // Two inputs: one file of 4 channels (88-byte info), and a list of two mono files.
        let mut one = [0u8; 88];
        one[24..28].copy_from_slice(&4i32.to_le_bytes());
        let mut two = [0u8; 176];
        two[24..28].copy_from_slice(&4i32.to_le_bytes());
        let ends = |b: &mut [u8]| {
            [
                b.as_mut_ptr() as usize,
                b.as_mut_ptr() as usize + b.len(),
                0,
            ]
        };
        let inner = [ends(&mut one), ends(&mut two)];
        let outer = [inner.as_ptr() as usize, inner.as_ptr() as usize + 48, 0];
        unsafe {
            pad_counts(
                outer.as_ptr() as usize,
                &[input(6, &[0, 1, 2, 4], "/a.wav")],
            )
        };
        assert_eq!(one[24], 6);
        assert_eq!(two[24], 4);
    }

    #[test]
    fn an_input_is_found_by_path_or_by_its_count() {
        let list = [input(8, &[0, 1, 2, 4], "/a.wav"), input(6, &[2], "/c.wav")];
        assert_eq!(input_for(&list, "/a.wav", 4), Some(0));
        assert_eq!(input_for(&list, "a.wav", 4), Some(0));
        assert_eq!(input_for(&list, "/a.wav", 8), None);
        assert_eq!(input_for(&list, "/c.wav", 1), Some(1));
        assert_eq!(input_for(&list, "/L.wav", 1), None);
        let two = [
            input(8, &[0, 1, 2, 4], "/a.wav"),
            input(6, &[0, 1, 4, 5], "/b.wav"),
        ];
        assert_eq!(input_for(&two, "/b.wav", 4), Some(1));
        assert_eq!(input_for(&two, "b.wav", 4), None);
    }
}
