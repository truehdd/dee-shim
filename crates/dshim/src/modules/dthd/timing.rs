//! Update times for dee_dthd_encoder.
//!
//! DEE codes each frame's metadata payload at the frame start with a 1536-sample ramp, so a
//! decoded master's updates, at a fixed phase of each frame, would come out early. Each payload
//! is coded at its frame's update instead, with its ramp; a frame without one restates the last
//! update's phase, and a frame with several (a join) takes the one that moves the most elements.

use crate::modules::common::require_all;
use crate::site::Resolver;
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const EVENT: usize = 64;
const EVENT_TIME: usize = 8;
const EVENT_HELD: usize = 16;
const EVENT_RAMP: usize = 20;
const UNIT: u64 = 40;
const MAX_RAMP: u64 = 2047;

type Event = [u8; EVENT];

// Offsets in the writer objects, which differ between the builds.
#[cfg(not(windows))]
mod at {
    pub const INNER: bool = false;
    pub const POSITION: usize = 352;
    pub const CONVERTER: usize = 40;
    pub const BUFFER: usize = 320;
}
#[cfg(windows)]
mod at {
    pub const INNER: bool = true;
    pub const POSITION: usize = 360;
    pub const CONVERTER: usize = 48;
    pub const BUFFER: usize = 328;
}

static PREPROCESS: AtomicUsize = AtomicUsize::new(0);
static EMIT: AtomicUsize = AtomicUsize::new(0);
static WRITE: AtomicUsize = AtomicUsize::new(0);
static OFFSET: AtomicUsize = AtomicUsize::new(0);
static CONVERT: AtomicUsize = AtomicUsize::new(0);
static SERIALIZE: AtomicUsize = AtomicUsize::new(0);
static STATE: Mutex<State> = Mutex::new(State {
    updates: BTreeMap::new(),
    last: None,
    emitter: 0,
    payload: None,
});

type Preprocess = extern "C" fn(usize, usize, usize) -> usize;
type Emit = extern "C" fn(usize, usize, usize);
type Offset = extern "C" fn(usize, u32, u16) -> usize;
type WriteFrame = extern "C" fn(usize, usize, usize);
type Convert = extern "C" fn(usize, usize, u32) -> usize;
type Serialize = extern "C" fn(usize, usize) -> usize;

struct State {
    /// The master's update times, each with the longest ramp of the elements updated then and
    /// their number.
    updates: BTreeMap<u64, (u64, u32)>,
    /// Phase in its frame and ramp of the last update coded.
    last: Option<(u64, u64)>,
    emitter: usize,
    /// The current frame's payload: its lead after its access unit's start, its ramp, and
    /// DEE's events for the frame.
    payload: Option<(u64, u64, Vec<Event>)>,
}

fn state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(&mut STATE.lock().unwrap_or_else(|e| e.into_inner()))
}

unsafe fn frame_events(frame: usize) -> &'static [Event] {
    let count = *((frame + 24) as *const usize);
    let events = *((frame + 32) as *const *const Event);
    std::slice::from_raw_parts(events, count)
}

fn read(e: &Event, at: usize, len: usize) -> u64 {
    let mut b = [0u8; 8];
    b[..len].copy_from_slice(&e[at..at + len]);
    u64::from_le_bytes(b)
}

/// Lead and ramp of the payload of the frame of `size` samples at `start`: at the frame's update
/// (time, ramp), or without one at the last update's phase (phase, ramp). The payload goes out
/// in the access unit the frame starts in, and the next frame's in the unit the next frame
/// starts in, so it is held to start before that.
fn timing(
    start: u64,
    size: u64,
    update: Option<(u64, u64)>,
    last: Option<(u64, u64)>,
) -> Option<(u64, u64)> {
    let (time, ramp) = update.or(last.map(|(phase, ramp)| (start + phase, ramp)))?;
    let next = (start + size) / UNIT * UNIT;
    Some((time.min(next - 1) - start / UNIT * UNIT, ramp.min(MAX_RAMP)))
}

/// Of a frame's updates, the one that moves the most elements, the later one on a tie: the payload
/// holds the state after the frame's last update, and only one update can be coded at its sample.
fn pick<'a>(updates: impl Iterator<Item = (&'a u64, &'a (u64, u32))>) -> Option<(u64, u64)> {
    updates
        .max_by_key(|(&t, &(_, n))| (n, t))
        .map(|(&t, &(r, _))| (t, r))
}

extern "C" fn preprocess(ret: usize, this: usize, input: usize) -> usize {
    let events = unsafe { frame_events(input) };
    state(|s| {
        for e in events.iter().filter(|e| e[EVENT_HELD] == 0) {
            let (ramp, count) = s.updates.entry(read(e, EVENT_TIME, 8)).or_default();
            *ramp = (*ramp).max(read(e, EVENT_RAMP, 4));
            *count += 1;
        }
    });
    let original: Preprocess = unsafe { std::mem::transmute(PREPROCESS.load(Ordering::Relaxed)) };
    original(ret, this, input)
}

extern "C" fn emit(this: usize, frame: usize, il: usize) {
    let emitter = if at::INNER {
        unsafe { *(this as *const usize) }
    } else {
        this
    };
    let size = unsafe { *((frame + 16) as *const u64) };
    state(|s| {
        s.payload = None;
        if size == 0 || unsafe { *(emitter as *const u32) } != 1 {
            return;
        }
        s.emitter = emitter;
        let start = unsafe { *((emitter + at::POSITION) as *const u64) };
        let later = s.updates.split_off(&(start + size));
        let update = pick(std::mem::replace(&mut s.updates, later).range(start..));
        let Some((lead, ramp)) = timing(start, size, update, s.last) else {
            return;
        };
        if let Some((time, ramp)) = update {
            s.last = Some((time - start, ramp));
        }
        s.payload = Some((lead, ramp, unsafe { frame_events(frame) }.to_vec()));
    });
    let original: Emit = unsafe { std::mem::transmute(EMIT.load(Ordering::Relaxed)) };
    original(this, frame, il)
}

/// The frame's payload is about to go into its access unit: serialize it again with its lead
/// and ramp, through DEE's own converter.
extern "C" fn write(writer: usize, data: usize, size: usize) {
    let original: WriteFrame = unsafe { std::mem::transmute(WRITE.load(Ordering::Relaxed)) };
    let (emitter, payload) = state(|s| (s.emitter, s.payload.take()));
    let Some((lead, ramp, events)) = payload else {
        return original(writer, data, size);
    };
    let converter = unsafe { *((emitter + at::CONVERTER) as *const usize) };
    let buffer = emitter + at::BUFFER;
    let set_offset: Offset = unsafe { std::mem::transmute(OFFSET.load(Ordering::Relaxed)) };
    let convert: Convert = unsafe { std::mem::transmute(CONVERT.load(Ordering::Relaxed)) };
    let serialize: Serialize = unsafe { std::mem::transmute(SERIALIZE.load(Ordering::Relaxed)) };
    // Block 0's ramp in the converter's update record; DEE sets it once, so put it back after.
    let ramp_field = unsafe { (*((converter + 48) as *const usize) + 6) as *mut u16 };
    let dee_ramp = unsafe { ramp_field.read() };
    unsafe { ramp_field.write(ramp as u16) };
    set_offset(converter, (lead & 31) as u32, (lead >> 5) as u16);
    let size = serialize(
        buffer,
        convert(converter, events.as_ptr() as usize, events.len() as u32),
    );
    unsafe { ramp_field.write(dee_ramp) };
    let bytes = unsafe { *(buffer as *const *const u8) };
    original(writer, bytes as usize, size);
}

pub fn install(res: &Resolver, log: &mut dyn Write) {
    if !crate::levers().dthd.oamd_update_times() {
        return;
    }
    let hooks: [(&str, usize, &AtomicUsize); 3] = [
        (
            "dthd.oa.preprocess",
            preprocess as *const () as usize,
            &PREPROCESS,
        ),
        ("dthd.oa.emit_frame", emit as *const () as usize, &EMIT),
        ("dthd.oa.write_frame", write as *const () as usize, &WRITE),
    ];
    let calls: [(&str, &AtomicUsize); 3] = [
        ("dthd.oa.frame_offset", &OFFSET),
        ("dthd.oa.convert_events", &CONVERT),
        ("dthd.oa.serialize", &SERIALIZE),
    ];
    let names: Vec<&str> = hooks
        .iter()
        .map(|h| h.0)
        .chain(calls.iter().map(|c| c.0))
        .collect();
    if names.iter().all(|name| res.profile.site(name).is_none()) {
        ldebug!(
            log,
            "dthd",
            "update times: no sites in this profile, skipped"
        );
        return;
    }
    let sites = require_all(res, "dthd update times", &names, log);
    for ((name, cell), site) in calls.iter().zip(&sites[hooks.len()..]) {
        cell.store(site.addr, Ordering::Relaxed);
        lok!(log, "dthd", "update times: {name} @ {:#x}", site.addr);
    }
    for ((name, hook, tramp), site) in hooks.iter().zip(&sites) {
        match site.detour(*hook) {
            Some(t) => {
                tramp.store(t, Ordering::Relaxed);
                lok!(log, "dthd", "update times: hook {name} @ {:#x}", site.addr);
            }
            None => {
                lerr!(
                    log,
                    "dthd",
                    "update times: could not detour {name}, stopping"
                );
                let _ = log.flush();
                std::process::exit(3);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{pick, timing};

    #[test]
    fn payload_starts_at_the_update() {
        // Frame at 1536 (its unit starts at 1520): an update at phase 1112 with ramp 0.
        assert_eq!(timing(1536, 1536, Some((2648, 0)), None), Some((1128, 0)));
        // An update after the next frame's unit starts is held to just before it.
        assert_eq!(timing(1536, 1536, Some((3070, 0)), None), Some((1519, 0)));
        // No update: the last update's phase and ramp.
        assert_eq!(
            timing(3072, 1536, None, Some((1112, 1536))),
            Some((1144, 1536))
        );
    }

    #[test]
    fn a_join_is_coded_at_the_second_master() {
        // The first master's update moves 4 elements, the second master starts with all 16.
        let frame = [(312920, (1536, 4)), (313020, (0, 16))];
        let updates = frame.iter().map(|(t, u)| (t, u));
        assert_eq!(pick(updates), Some((313020, 0)));
    }
}
