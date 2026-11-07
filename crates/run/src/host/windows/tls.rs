//! Static TLS for the mapped tool.
//!
//! The tool's static MSVC CRT keeps per-thread data in implicit TLS (`_Init_thread_epoch` for
//! every function-local static, plus the tool's own `thread_local` variables). Compiled code
//! reaches it in three steps: `mov ecx, [_tls_index]`, `mov rax, gs:[58h]` (the TEB's
//! ThreadLocalStoragePointer, ntdll's per-thread array of module blocks), then
//! `[rax + rcx*8]` plus a fixed offset. ntdll fills that array only for modules it processed as
//! having TLS; `LoadLibraryExW` on an `.exe` does not, so `_tls_index` stays 0, the host's slot.
//!
//! An index of our own in ntdll's array is not safe: ntdll hands its indices out to every DLL
//! with static TLS loaded later, and nothing marks ours as taken. dthd loads enough shell DLLs
//! with TLS that ntdll would hand one of them the tool's index, replace the tool's block on every
//! thread, and fault freeing it (heap corruption, 0xC0000374). So the tool does not use ntdll's
//! array at all:
//!
//! - Every `mov r64, gs:[58h]` in the image's executable sections (one 9-byte form,
//!   `65 48|4C 8B /r 25 58 00 00 00`; 166 sites in ddp, 153 in dthd) gets its displacement
//!   changed to `0x1480 + 8 * i`: TEB TlsSlots[i], the slot of a dynamic TLS index `i` from
//!   TlsAlloc. Same length, a 4-byte edit in the private copy-on-write image. Any other gs
//!   access with displacement 0x58 stops the host, since it would be a form this does not know.
//! - Each thread the tool runs on gets a heap cell holding its block pointer, followed by the
//!   block built from the image's TLS template. TlsSetValue(i, cell - 8 * _tls_index) makes
//!   `[TlsSlots[i] + _tls_index * 8]` read the cell.
//! - `_tls_index` is set to 2^31, so a site the scan missed reads 16 GiB past ntdll's array
//!   and faults at once instead of quietly using the host's block.
//!
//! TlsSlots is a documented TEB field (winternl.h), at 0x1480 on x64, and TlsGetValue reads
//! exactly that slot for indices below 64; the host allocates its index first thing, so it is
//! always below 64 (checked). No undocumented structure is relied on.
//!
//! The host runs the image's TLS callbacks where ntdll would: DLL_PROCESS_ATTACH on the tool's
//! first thread, DLL_THREAD_ATTACH / DLL_THREAD_DETACH on every later one (seen through the
//! host's own TLS callback), DLL_PROCESS_DETACH on the thread that calls ExitProcess.

use super::sys::{self, rd32, rd64, Handle};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::OnceLock;

/// TEB offset of TlsSlots[0] on x64 (winternl.h: `PVOID TlsSlots[64]`).
const TLS_SLOTS: u32 = 0x1480;
/// The value given to the image's `_tls_index` (see the module comment).
const TRAP_INDEX: u32 = 0x8000_0000;
/// Bytes before the block in each thread's allocation: the cell, padded to keep the block
/// 16-byte aligned.
const CELL: usize = 16;

/// The image's TLS template and the slot standing in for its array.
struct Image {
    base: usize,
    /// Runtime address and length of the initialized template, and the zero fill after it.
    start: usize,
    len: usize,
    zero_fill: usize,
    callbacks: Vec<usize>,
    /// The dynamic TLS index whose TEB slot the patched code reads.
    slot: u32,
}

static IMAGE: OnceLock<Image> = OnceLock::new();

/// Set once the tool's first thread has its block; from then on new threads get one too.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Threads given a block so far, the first one included (for the debug log at exit).
static THREADS: AtomicUsize = AtomicUsize::new(0);

/// How many threads have had a TLS block for the tool.
pub fn threads_seen() -> usize {
    THREADS.load(Ordering::Relaxed)
}

/// What `prepare` did, for the debug log.
pub struct Prepared {
    pub slot: u32,
    pub sites: usize,
}

/// Runtime addresses of every `mov r64, gs:[58h]` in the image's executable sections. Errors on
/// any other instruction that looks like a gs access with displacement 0x58.
unsafe fn tls_array_loads(headers: &sys::Headers) -> Result<Vec<usize>, String> {
    let mut sites = Vec::new();
    for section in headers.sections() {
        if section.characteristics & sys::IMAGE_SCN_MEM_EXECUTE == 0 {
            continue;
        }
        let code = std::slice::from_raw_parts(section.start as *const u8, section.size);
        for at in 0..code.len().saturating_sub(8) {
            if code[at] != 0x65 {
                continue;
            }
            let known = matches!(code[at + 1], 0x48 | 0x4C)
                && code[at + 2] == 0x8B
                && code[at + 3] & 0xC7 == 0x04
                && code[at + 4..at + 9] == [0x25, 0x58, 0, 0, 0];
            if known {
                sites.push(section.start + at);
                continue;
            }
            // A gs prefix followed within four bytes by SIB 0x25 and disp32 0x58: some other
            // instruction reading the TLS array, which this host cannot redirect.
            let window = &code[at + 1..(at + 10).min(code.len())];
            if (0..4).any(|i| window.get(i..i + 5) == Some(&[0x25, 0x58, 0, 0, 0][..])) {
                return Err(format!(
                    "unknown gs:[58h] access at image+{:#x}",
                    section.start + at - headers.base
                ));
            }
        }
    }
    Ok(sites)
}

/// Point every TLS array load at TEB TlsSlots[slot] (see the module comment).
unsafe fn redirect(sites: &[usize], slot: u32) -> Result<(), String> {
    let displacement = (TLS_SLOTS + 8 * slot).to_le_bytes();
    for &site in sites {
        let field = (site + 5) as *mut c_void;
        let mut old = 0;
        if sys::VirtualProtect(field, 4, sys::PAGE_EXECUTE_READWRITE, &mut old) == 0 {
            return Err(format!("make code writable: error {}", sys::GetLastError()));
        }
        std::ptr::copy_nonoverlapping(displacement.as_ptr(), field.cast::<u8>(), 4);
        sys::VirtualProtect(field, 4, old, &mut old);
        sys::FlushInstructionCache(sys::GetCurrentProcess(), field, 4);
    }
    Ok(())
}

/// Read the image's TLS directory and redirect its TLS accesses. Must run before the tool's
/// first thread starts. An image without TLS needs nothing.
pub unsafe fn prepare(headers: &sys::Headers) -> Result<Option<Prepared>, String> {
    let Some((dir, _)) = headers.dir(sys::DIR_TLS) else {
        return Ok(None);
    };
    // IMAGE_TLS_DIRECTORY64: StartAddressOfRawData, EndAddressOfRawData, AddressOfIndex,
    // AddressOfCallBacks (all relocated VAs), SizeOfZeroFill, Characteristics.
    let start = rd64(dir) as usize;
    let end = rd64(dir + 8) as usize;
    let index = rd64(dir + 16) as usize;
    let mut callbacks = Vec::new();
    let mut at = rd64(dir + 24) as usize;
    while at != 0 && rd64(at) != 0 {
        callbacks.push(rd64(at) as usize);
        at += 8;
    }
    let zero_fill = rd32(dir + 32) as usize;
    let align_bits = (rd32(dir + 36) >> 20) & 0xF;
    if align_bits > 5 {
        return Err(format!("TLS alignment 2^{} is above 16", align_bits - 1));
    }
    let slot = sys::TlsAlloc();
    if slot >= 64 {
        // TLS_OUT_OF_INDEXES, or an expansion slot that does not live in the TEB itself.
        return Err(format!(
            "TlsAlloc gave index {slot}, not one of the 64 TEB slots"
        ));
    }
    let sites = tls_array_loads(headers)?;
    redirect(&sites, slot)?;
    let mut old = 0;
    if sys::VirtualProtect(index as *mut c_void, 4, sys::PAGE_READWRITE, &mut old) == 0 {
        return Err(format!(
            "make _tls_index writable: error {}",
            sys::GetLastError()
        ));
    }
    (index as *mut u32).write(TRAP_INDEX);
    sys::VirtualProtect(index as *mut c_void, 4, old, &mut old);
    let _ = IMAGE.set(Image {
        base: headers.base,
        start,
        len: end - start,
        zero_fill,
        callbacks,
        slot,
    });
    enable_callback();
    Ok(Some(Prepared {
        slot,
        sites: sites.len(),
    }))
}

unsafe fn run_callbacks(image: &Image, reason: u32) {
    for &callback in &image.callbacks {
        let callback: unsafe extern "system" fn(Handle, u32, *mut c_void) =
            std::mem::transmute(callback);
        callback(image.base as Handle, reason, std::ptr::null_mut());
    }
}

/// The TlsSlots value for a thread whose cell is at `cell`.
fn biased(cell: usize) -> *mut c_void {
    cell.wrapping_sub(8 * TRAP_INDEX as usize) as *mut c_void
}

/// Give the current thread a block from the template and run the image's callbacks with
/// `reason`.
unsafe fn attach(image: &Image, reason: u32) {
    let heap = sys::GetProcessHeap();
    let size = CELL + image.len + image.zero_fill;
    let cell = sys::HeapAlloc(heap, sys::HEAP_ZERO_MEMORY, size) as usize;
    if cell == 0 {
        eprintln!("dee_shim: in-process host: out of memory for a TLS block");
        std::process::abort();
    }
    let block = cell + CELL;
    std::ptr::copy_nonoverlapping(image.start as *const u8, block as *mut u8, image.len);
    (cell as *mut usize).write(block);
    sys::TlsSetValue(image.slot, biased(cell));
    THREADS.fetch_add(1, Ordering::Relaxed);
    run_callbacks(image, reason);
}

/// Run the image's callbacks with `reason` on a thread that has a block; free the block when
/// the thread (not the process) is ending.
unsafe fn detach(image: &Image, reason: u32) {
    let value = sys::TlsGetValue(image.slot);
    if value.is_null() {
        return;
    }
    run_callbacks(image, reason);
    if reason == sys::DLL_THREAD_DETACH {
        sys::TlsSetValue(image.slot, std::ptr::null_mut());
        let cell = (value as usize).wrapping_add(8 * TRAP_INDEX as usize);
        sys::HeapFree(sys::GetProcessHeap(), 0, cell as *mut c_void);
    }
}

/// Called on the tool's first thread before its entry point: the image sees
/// DLL_PROCESS_ATTACH there, as on a native main thread. Later threads get DLL_THREAD_ATTACH
/// from the host's TLS callback.
pub fn start_first_thread() {
    if let Some(image) = IMAGE.get() {
        unsafe { attach(image, sys::DLL_PROCESS_ATTACH) };
    }
    ACTIVE.store(true, Ordering::Release);
}

/// The host's own TLS callback. ntdll calls every callback of the host executable on thread
/// attach and detach, and on process detach on the thread that calls ExitProcess.
unsafe extern "system" fn on_thread(_module: *mut c_void, reason: u32, _reserved: *mut c_void) {
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let Some(image) = IMAGE.get() else {
        return;
    };
    match reason {
        sys::DLL_THREAD_ATTACH => attach(image, reason),
        sys::DLL_THREAD_DETACH | sys::DLL_PROCESS_DETACH => detach(image, reason),
        _ => {}
    }
}

// `.CRT$XLB` is Rust std's own callback; the CRT's `__xl_a`/`__xl_z` bracket the array.
#[link_section = ".CRT$XLF"]
#[used]
static CALLBACK: unsafe extern "system" fn(*mut c_void, u32, *mut c_void) = on_thread;

extern "C" {
    #[link_name = "_tls_used"]
    static TLS_USED: u8;
}

/// Keep the TLS directory and the callback from being discarded by the linker, as std does.
fn enable_callback() {
    unsafe {
        std::ptr::from_ref(&TLS_USED).read_volatile();
        std::ptr::from_ref(&CALLBACK).read_volatile();
    }
}
