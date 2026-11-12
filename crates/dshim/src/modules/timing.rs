//! Update times for dee_dthd_encoder and dee_ddpjoc_encoder.
//!
//! DEE codes each frame's metadata payload at the frame start with a 1536-sample ramp, so a
//! decoded master's updates, at a fixed phase of each frame, would come out early. Each payload
//! is coded at its frame's update instead, with its ramp; a frame without one restates the last
//! update's phase, and a frame with several (a join) takes the one that moves the most elements.

use crate::modules::common::require_all;
use crate::site::Resolver;
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

const EVENT: usize = 64;
const EVENT_TIME: usize = 8;
const EVENT_HELD: usize = 16;
const EVENT_RAMP: usize = 20;
const UNIT: u64 = 40;
const FRAME: u64 = 1536;
const MAX_RAMP: u64 = 2047;

type Event = [u8; EVENT];

/// Where the emitter keeps what this module reads. dthd on Linux is the macOS binary
/// (dthd-mac2linux) with its offsets; the Linux ddpjoc has the Windows ones. On Windows the hooks
/// get the emitter through one more pointer.
struct Layout {
    inner: bool,
    position: usize,
    converter: usize,
    buffer: usize,
}
#[cfg(not(windows))]
const DTHD: Layout = Layout {
    inner: false,
    position: 352,
    converter: 40,
    buffer: 320,
};
#[cfg(target_os = "macos")]
const DDPJOC: Layout = DTHD;
#[cfg(not(any(windows, target_os = "macos")))]
const DDPJOC: Layout = Layout {
    inner: false,
    position: 360,
    converter: 48,
    buffer: 328,
};
#[cfg(windows)]
const DTHD: Layout = Layout {
    inner: true,
    position: 360,
    converter: 48,
    buffer: 328,
};
#[cfg(windows)]
const DDPJOC: Layout = DTHD;

fn layout() -> &'static Layout {
    if JOC.load(Ordering::Relaxed) {
        &DDPJOC
    } else {
        &DTHD
    }
}

static PREPROCESS: AtomicUsize = AtomicUsize::new(0);
static PRESAMPLE: AtomicUsize = AtomicUsize::new(0);
static EMIT: AtomicUsize = AtomicUsize::new(0);
static WRITE: AtomicUsize = AtomicUsize::new(0);
static OFFSET: AtomicUsize = AtomicUsize::new(0);
static SPLIT: AtomicUsize = AtomicUsize::new(0);
static CONVERT: AtomicUsize = AtomicUsize::new(0);
static SERIALIZE: AtomicUsize = AtomicUsize::new(0);
static JOC: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<State> = Mutex::new(State {
    updates: BTreeMap::new(),
    last: None,
    emitter: 0,
    payload: None,
});

type Preprocess = extern "C" fn(usize, usize, usize) -> usize;
type Emit = extern "C" fn(usize, usize, usize);
type EmitJoc = extern "C" fn(usize, usize, usize, usize) -> usize;
type Offset = extern "C" fn(usize, u32, u16) -> usize;
type Split = extern "C" fn(usize, usize, usize) -> usize;
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

/// Lead and ramp of the payload of the frame at `start`: at the frame's update (time, ramp), or
/// without one at the last update's phase (phase, ramp), after the start of the `unit` the frame
/// starts in. Dolby's decoder applies a payload at its time, even after the next frame's.
fn timing(
    start: u64,
    unit: u64,
    update: Option<(u64, u64)>,
    last: Option<(u64, u64)>,
) -> Option<(u64, u64)> {
    let (time, ramp) = update.or(last.map(|(phase, ramp)| (start + phase, ramp)))?;
    Some((time - start / unit * unit, ramp.min(MAX_RAMP)))
}

/// Of a frame's updates, the one that moves the most elements, the later one on a tie: the payload
/// holds the state after the frame's last update, and only one update can be coded at its sample.
fn pick<'a>(updates: impl Iterator<Item = (&'a u64, &'a (u64, u32))>) -> Option<(u64, u64)> {
    updates
        .max_by_key(|(&t, &(_, n))| (n, t))
        .map(|(&t, &(r, _))| (t, r))
}

/// Note the master's updates among a frame's events.
fn note_updates(frame: usize) {
    let events = unsafe { frame_events(frame) };
    state(|s| {
        for e in events.iter().filter(|e| e[EVENT_HELD] == 0) {
            let (ramp, count) = s.updates.entry(read(e, EVENT_TIME, 8)).or_default();
            *ramp = (*ramp).max(read(e, EVENT_RAMP, 4));
            *count += 1;
        }
    });
}

/// The presampler's result, two words: returned in rax:rdx by the SysV builds, through a pointer
/// after `this` by MSVC.
#[cfg(not(windows))]
#[repr(C)]
struct Pair(u64, u64);
#[cfg(not(windows))]
type Presample = extern "C" fn(usize, usize) -> Pair;
#[cfg(windows)]
type Presample = extern "C" fn(usize, usize, usize) -> usize;

/// DD+ JOC: DEE moves each element's events in a 512-sample block to the block start before its
/// preprocessor, so the update times are noted at that step's input.
#[cfg(not(windows))]
extern "C" fn presample(this: usize, frame: usize) -> Pair {
    note_updates(frame);
    let original: Presample = unsafe { std::mem::transmute(PRESAMPLE.load(Ordering::Relaxed)) };
    original(this, frame)
}
#[cfg(windows)]
extern "C" fn presample(this: usize, ret: usize, frame: usize) -> usize {
    note_updates(frame);
    let original: Presample = unsafe { std::mem::transmute(PRESAMPLE.load(Ordering::Relaxed)) };
    original(this, ret, frame)
}

extern "C" fn preprocess(ret: usize, this: usize, input: usize) -> usize {
    note_updates(input);
    let original: Preprocess = unsafe { std::mem::transmute(PREPROCESS.load(Ordering::Relaxed)) };
    original(ret, this, input)
}

/// Note the frame's payload timing; the emitter codes in 40-sample units in mode 1 (TrueHD),
/// else in frames.
fn note(this: usize, frame: usize) {
    let at = layout();
    let emitter = if at.inner {
        unsafe { *(this as *const usize) }
    } else {
        this
    };
    let size = unsafe { *((frame + 16) as *const u64) };
    let mode = unsafe { *(emitter as *const u32) };
    state(|s| {
        s.payload = None;
        if size == 0 || (mode != 1 && !JOC.load(Ordering::Relaxed)) {
            return;
        }
        s.emitter = emitter;
        let unit = if mode == 1 { UNIT } else { FRAME };
        let start = unsafe { *((emitter + at.position) as *const u64) };
        let later = s.updates.split_off(&(start + size));
        let update = pick(std::mem::replace(&mut s.updates, later).range(start..));
        let Some((lead, ramp)) = timing(start, unit, update, s.last) else {
            return;
        };
        if let Some((time, ramp)) = update {
            s.last = Some((time - start, ramp));
        }
        s.payload = Some((lead, ramp, unsafe { frame_events(frame) }.to_vec()));
    });
}

extern "C" fn emit(this: usize, frame: usize, il: usize) {
    note(this, frame);
    let original: Emit = unsafe { std::mem::transmute(EMIT.load(Ordering::Relaxed)) };
    original(this, frame, il)
}

/// DD+ JOC's emitter returns a value: through a pointer before `this` in SysV, after it in MSVC.
#[cfg(not(windows))]
extern "C" fn emit_joc(ret: usize, this: usize, frame: usize, il: usize) -> usize {
    note(this, frame);
    let original: EmitJoc = unsafe { std::mem::transmute(EMIT.load(Ordering::Relaxed)) };
    original(ret, this, frame, il)
}
#[cfg(windows)]
extern "C" fn emit_joc(this: usize, ret: usize, frame: usize, il: usize) -> usize {
    note(this, frame);
    let original: EmitJoc = unsafe { std::mem::transmute(EMIT.load(Ordering::Relaxed)) };
    original(this, ret, frame, il)
}

/// The frame's payload is about to be written: serialize it again with its lead and ramp,
/// through DEE's own converter. Returns the new bytes and their size.
fn rewrite() -> Option<(usize, usize)> {
    let (emitter, payload) = state(|s| (s.emitter, s.payload.take()));

    let (lead, ramp, events) = payload?;
    let converter = unsafe { *((emitter + DTHD.converter) as *const usize) };
    let buffer = emitter + DTHD.buffer;
    let set_offset: Offset = unsafe { std::mem::transmute(OFFSET.load(Ordering::Relaxed)) };
    let convert: Convert = unsafe { std::mem::transmute(CONVERT.load(Ordering::Relaxed)) };
    let serialize: Serialize = unsafe { std::mem::transmute(SERIALIZE.load(Ordering::Relaxed)) };
    // DEE sets the ramp once, so put it back after.
    let ramp_field = unsafe { ramp_field(converter) };
    let dee_ramp = unsafe { ramp_field.read() };
    unsafe { ramp_field.write(ramp as u16) };
    set_offset(converter, (lead & 31) as u32, (lead >> 5) as u16);
    let size = serialize(
        buffer,
        convert(converter, events.as_ptr() as usize, events.len() as u32),
    );
    unsafe { ramp_field.write(dee_ramp) };
    Some((unsafe { *(buffer as *const usize) }, size))
}

extern "C" fn write(writer: usize, data: usize, size: usize) {
    let original: WriteFrame = unsafe { std::mem::transmute(WRITE.load(Ordering::Relaxed)) };
    let (data, size) = rewrite().unwrap_or((data, size));
    original(writer, data, size)
}

/// DD+ JOC: DEE splits the frame's position into the block offset factor (word 0) and sample
/// offset (word 1) just before it codes the payload; the lead and ramp go in there.
extern "C" fn split_joc(out: usize, position: usize, unit: usize) -> usize {
    let original: Split = unsafe { std::mem::transmute(SPLIT.load(Ordering::Relaxed)) };
    let ret = original(out, position, unit);
    let (emitter, payload) = state(|s| (s.emitter, s.payload.take()));
    if let Some((lead, ramp, _)) = payload {
        unsafe {
            let converter = *((emitter + DDPJOC.converter) as *const usize);
            ramp_field(converter).write(ramp as u16);
            (out as *mut u32).write((lead >> 5) as u32);
            ((out + 4) as *mut u32).write((lead & 31) as u32);
        }
    }
    ret
}

/// Block 0's ramp in the converter's update record.
unsafe fn ramp_field(converter: usize) -> *mut u16 {
    (*((converter + 48) as *const usize) + 6) as *mut u16
}

pub fn install(res: &Resolver, log: &mut dyn Write) {
    if !crate::levers().dthd.oamd_update_times() {
        return;
    }
    let hooks: [(&str, usize, &AtomicUsize); 3] = [
        ("preprocess", preprocess as *const () as usize, &PREPROCESS),
        ("emit_frame", emit as *const () as usize, &EMIT),
        ("write_frame", write as *const () as usize, &WRITE),
    ];
    let calls: [(&str, &AtomicUsize); 3] = [
        ("frame_offset", &OFFSET),
        ("convert_events", &CONVERT),
        ("serialize", &SERIALIZE),
    ];
    detour_all(res, "dthd", &hooks, &calls, log);
}

pub fn install_ddpjoc(res: &Resolver, log: &mut dyn Write) {
    if !crate::levers().ddpjoc.oamd_update_times() {
        return;
    }
    JOC.store(true, Ordering::Relaxed);
    let hooks: [(&str, usize, &AtomicUsize); 3] = [
        ("presample", presample as *const () as usize, &PRESAMPLE),
        ("emit_frame", emit_joc as *const () as usize, &EMIT),
        ("payload_offset", split_joc as *const () as usize, &SPLIT),
    ];
    detour_all(res, "ddpjoc", &hooks, &[], log);
}

fn detour_all(
    res: &Resolver,
    tool: &str,
    hooks: &[(&str, usize, &AtomicUsize)],
    calls: &[(&str, &AtomicUsize)],
    log: &mut dyn Write,
) {
    let names: Vec<String> = hooks
        .iter()
        .map(|h| h.0)
        .chain(calls.iter().map(|c| c.0))
        .map(|n| format!("{tool}.oa.{n}"))
        .collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    if names.iter().all(|name| res.profile.site(name).is_none()) {
        ldebug!(log, tool, "update times: no sites in this profile, skipped");
        return;
    }
    let sites = require_all(res, "update times", &names, log);
    for ((name, (_, cell)), site) in names[hooks.len()..]
        .iter()
        .zip(calls)
        .zip(&sites[hooks.len()..])
    {
        cell.store(site.addr, Ordering::Relaxed);
        lok!(log, tool, "update times: {name} @ {:#x}", site.addr);
    }
    for ((name, (_, hook, tramp)), site) in names.iter().zip(hooks).zip(&sites) {
        match site.detour(*hook) {
            Some(t) => {
                tramp.store(t, Ordering::Relaxed);
                lok!(log, tool, "update times: hook {name} @ {:#x}", site.addr);
            }
            None => {
                lerr!(log, tool, "update times: could not detour {name}, stopping");
                let _ = log.flush();
                std::process::exit(3);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{pick, timing, FRAME, UNIT};

    #[test]
    fn payload_starts_at_the_update() {
        // Frame at 1536 (its unit starts at 1520): an update at phase 1112 with ramp 0.
        assert_eq!(timing(1536, UNIT, Some((2648, 0)), None), Some((1128, 0)));
        // An update after the next frame's unit starts keeps its sample.
        assert_eq!(timing(1536, UNIT, Some((3070, 0)), None), Some((1550, 0)));
        // No update: the last update's phase and ramp.
        assert_eq!(
            timing(3072, UNIT, None, Some((1112, 1536))),
            Some((1144, 1536))
        );
        // DD+: the payload is in its own frame.
        assert_eq!(
            timing(3072, FRAME, Some((4607, 1536)), None),
            Some((1535, 1536))
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
