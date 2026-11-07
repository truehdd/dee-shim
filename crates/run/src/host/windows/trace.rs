//! Debugging aid (DEE_SHIM_HOST_TRACE set): print every first-chance exception before the
//! tool's own handlers run. A C++ throw gets one line; anything else (an access violation, a
//! heap failure) gets its address relative to the tool image and a stack trace.

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

#[link(name = "kernel32")]
extern "system" {
    fn AddVectoredExceptionHandler(
        first: u32,
        handler: unsafe extern "system" fn(*mut *mut c_void) -> i32,
    ) -> *mut c_void;
    fn RtlCaptureStackBackTrace(skip: u32, count: u32, out: *mut usize, hash: *mut u32) -> u16;
    fn GetModuleHandleExW(flags: u32, name: *const u16, out: *mut *mut c_void) -> i32;
    fn GetModuleFileNameW(module: *mut c_void, buf: *mut u16, size: u32) -> u32;
}

unsafe fn describe(address: usize) -> String {
    let mut module = std::ptr::null_mut();
    // FROM_ADDRESS | UNCHANGED_REFCOUNT
    if GetModuleHandleExW(6, address as *const u16, &mut module) == 0 {
        return format!("{address:#x}");
    }
    let mut buf = [0u16; 260];
    let n = GetModuleFileNameW(module, buf.as_mut_ptr(), 260) as usize;
    let name = String::from_utf16_lossy(&buf[..n]);
    let name = name.rsplit('\\').next().unwrap_or("").to_string();
    format!("{name}+{:#x}", address - module as usize)
}

static BASE: AtomicUsize = AtomicUsize::new(0);

unsafe extern "system" fn handler(pointers: *mut *mut c_void) -> i32 {
    // EXCEPTION_POINTERS { ExceptionRecord, ContextRecord }; EXCEPTION_RECORD: ExceptionCode u32
    // at +0, ExceptionAddress at +16, NumberParameters at +24, ExceptionInformation at +32.
    let record = *pointers as usize;
    let code = *(record as *const u32);
    if code == 0xE06D_7363 {
        let address = *((record + 16) as *const usize);
        eprintln!("host TRACE: C++ throw from {}", describe(address));
    } else if code != 0x406D_1388 {
        let address = *((record + 16) as *const usize);
        let info = *((record + 32) as *const usize);
        let info2 = *((record + 40) as *const usize);
        let base = BASE.load(Ordering::Relaxed);
        eprintln!(
            "host TRACE: exception {code:#x} at {address:#x} (image+{:#x}) info {info:#x} {info2:#x}",
            address.wrapping_sub(base)
        );
        let mut frames = [0usize; 40];
        let n = RtlCaptureStackBackTrace(0, 40, frames.as_mut_ptr(), std::ptr::null_mut());
        for &frame in &frames[..n as usize] {
            eprintln!("host TRACE:   {}", describe(frame));
        }
    }
    0 // EXCEPTION_CONTINUE_SEARCH
}

pub unsafe fn install(base: usize) {
    BASE.store(base, Ordering::Relaxed);
    AddVectoredExceptionHandler(1, handler);
}
