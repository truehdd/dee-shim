//! A raw PCM input that holds only its layout's channels, for dee_ddp_encoder.
//!
//! DEE reads raw PCM as bytes from a stream, a file or a named pipe, and takes the channel count
//! from the input format. The runner names the padded count there. Here the stream's read gives
//! DEE padded frames: it reads the input's own frames and moves each sample to its slot. DEE
//! copies a pipe into a temporary file as it reads it, so that copy is padded too.
//!
//! The read and the seek can throw, so they are entered through the stubs of `wav_slots`.

use super::wav_slots::stub;
use crate::modules::common::require_all;
use crate::site::Resolver;
use dshim_levers::PcmSlots;
use std::cell::Cell;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

/// The stream's name, a std::string.
const NAME: usize = if cfg!(target_os = "macos") {
    32
} else if cfg!(windows) {
    24
} else {
    56
};

static INPUTS: OnceLock<Vec<PcmSlots>> = OnceLock::new();
static READ: AtomicUsize = AtomicUsize::new(0);
static SEEK: AtomicUsize = AtomicUsize::new(0);

type Read = extern "C" fn(usize, *mut u8, i64) -> i64;
type Seek = extern "C" fn(usize, i64, u32) -> i64;

#[cfg(target_os = "macos")]
stub!("_dshim_pcm_read_stub", ours, read, READ);
#[cfg(target_os = "macos")]
stub!("_dshim_pcm_seek_stub", ours, seek, SEEK);
#[cfg(not(target_os = "macos"))]
stub!("dshim_pcm_read_stub", ours, read, READ);
#[cfg(not(target_os = "macos"))]
stub!("dshim_pcm_seek_stub", ours, seek, SEEK);

extern "C" {
    fn dshim_pcm_read_stub();
    fn dshim_pcm_seek_stub();
}

/// The bytes of the std::string at `at`, by the build's C++ library.
///
/// # Safety
/// `at` points at a live std::string.
unsafe fn string<'a>(at: usize) -> &'a [u8] {
    let word = |offset: usize| *((at + offset) as *const usize);
    let (data, size) = if cfg!(target_os = "macos") {
        let first = *(at as *const u8);
        if first & 1 == 0 {
            (at + 1, (first >> 1) as usize)
        } else {
            (word(16), word(8))
        }
    } else if cfg!(windows) {
        (if word(24) >= 16 { word(0) } else { at }, word(16))
    } else {
        (word(0), word(8))
    };
    std::slice::from_raw_parts(data as *const u8, size)
}

/// The input a stream of this name reads. DEE names a pipe's stream by its URI and may put its
/// own fields after it.
fn input_for<'a>(list: &'a [PcmSlots], name: &[u8]) -> Option<&'a PcmSlots> {
    list.iter().find(|p| {
        let path = p.slots.path.as_bytes();
        name == path || (name.starts_with(path) && name[path.len()..].starts_with(b":stream_"))
    })
}

fn input_of(stream: usize) -> Option<&'static PcmSlots> {
    input_for(INPUTS.get()?, unsafe { string(stream + NAME) })
}

thread_local! {
    /// Inside the read of one of the inputs: a pipe's stream reads through a second stream of
    /// the same name, which must give the bytes as they are.
    static READING: Cell<bool> = const { Cell::new(false) };
}

extern "C" fn ours(stream: usize) -> bool {
    !READING.get() && input_of(stream).is_some()
}

/// Spread `frames` frames of the input's channels, packed at the start of `buffer`, over the
/// slots, last sample first, and clear the other slots.
fn widen(buffer: &mut [u8], frames: usize, p: &PcmSlots) {
    let (bytes, total, slots) = (p.bytes, p.slots.total, &p.slots.slots);
    for frame in (0..frames).rev() {
        let mut channel = slots.len();
        for slot in (0..total).rev() {
            let to = (frame * total + slot) * bytes;
            if channel > 0 && slots[channel - 1] == slot {
                channel -= 1;
                let from = (frame * slots.len() + channel) * bytes;
                buffer.copy_within(from..from + bytes, to);
            } else {
                buffer[to..to + bytes].fill(0);
            }
        }
    }
}

/// The stream's read (stream, buffer, bytes) -> bytes read.
extern "C" fn read(stream: usize, buffer: *mut u8, length: i64) -> i64 {
    let original: Read = unsafe { std::mem::transmute(READ.load(Ordering::Relaxed)) };
    let Some(p) = input_of(stream) else {
        return original(stream, buffer, length);
    };
    let (own, padded) = (p.slots.slots.len() * p.bytes, p.slots.total * p.bytes);
    let want = length.max(0) as usize / padded * own;
    let mut got = 0;
    READING.set(true);
    // A pipe gives what it has; go on to the end of a frame.
    while got < want {
        let n = original(stream, unsafe { buffer.add(got) }, (want - got) as i64);
        if n <= 0 {
            if got == 0 {
                READING.set(false);
                return n;
            }
            break;
        }
        got += n as usize;
    }
    READING.set(false);
    let frames = got / own;
    let buffer = unsafe { std::slice::from_raw_parts_mut(buffer, frames * padded) };
    widen(buffer, frames, p);
    (frames * padded) as i64
}

/// The stream's seek (stream, offset, whence): DEE's offset counts padded frames.
extern "C" fn seek(stream: usize, offset: i64, whence: u32) -> i64 {
    let original: Seek = unsafe { std::mem::transmute(SEEK.load(Ordering::Relaxed)) };
    let offset = match input_of(stream) {
        Some(p) => offset / p.slots.total as i64 * p.slots.slots.len() as i64,
        None => offset,
    };
    original(stream, offset, whence)
}

pub fn install(res: &Resolver, list: &[PcmSlots], log: &mut dyn Write) {
    if list.is_empty() {
        return;
    }
    let hooks: [(&str, usize, &AtomicUsize); 2] = [
        (
            "ddp.stream.read",
            dshim_pcm_read_stub as *const () as usize,
            &READ,
        ),
        (
            "ddp.stream.seek",
            dshim_pcm_seek_stub as *const () as usize,
            &SEEK,
        ),
    ];
    let names: Vec<&str> = hooks.iter().map(|h| h.0).collect();
    let sites = require_all(res, "pcm slots", &names, log);
    let _ = INPUTS.set(list.to_vec());
    for ((name, hook, tramp), site) in hooks.iter().zip(&sites) {
        match site.detour(*hook) {
            Some(t) => {
                tramp.store(t, Ordering::Relaxed);
                lok!(log, "ddp", "pcm slots: hook {name} @ {:#x}", site.addr);
            }
            None => {
                lerr!(log, "ddp", "pcm slots: could not detour {name}, stopping");
                let _ = log.flush();
                std::process::exit(3);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dshim_levers::WavSlots;

    fn input(bytes: usize, total: usize, slots: &[usize], path: &str) -> PcmSlots {
        PcmSlots {
            bytes,
            slots: WavSlots {
                total,
                slots: slots.to_vec(),
                path: path.to_string(),
            },
        }
    }

    #[test]
    fn samples_move_to_their_slots_and_the_rest_is_silent() {
        // Two frames of L R C S, two bytes a sample, in a buffer for six slots.
        let mut buffer = [0xffu8; 24];
        buffer[..16].copy_from_slice(&[1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8]);
        widen(&mut buffer, 2, &input(2, 6, &[0, 1, 2, 4], "/a.pcm"));
        assert_eq!(buffer[..12], [1, 1, 2, 2, 3, 3, 0, 0, 4, 4, 0, 0]);
        assert_eq!(buffer[12..], [5, 5, 6, 6, 7, 7, 0, 0, 8, 8, 0, 0]);
    }

    #[test]
    fn a_stream_is_found_by_its_name() {
        let list = [input(3, 6, &[2, 3], "named_pipe:///x/in")];
        let found = |name: &str| input_for(&list, name.as_bytes()).is_some();
        assert!(found("named_pipe:///x/in"));
        assert!(found(
            "named_pipe:///x/in:stream_object=0x1:stream_origin=named_pipe:///x/in"
        ));
        assert!(!found("named_pipe:///x/in2"));
        assert!(!found("/tmp/1-2/inputFile0"));
    }
}
