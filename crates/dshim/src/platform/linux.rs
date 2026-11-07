//! Linux: page protection, instruction cache and trampoline memory. The DEE tools are non-PIE
//! (ET_EXEC), so the host maps them at their link-time addresses and the slide is 0.

use std::ffi::c_void;

extern "C" {
    fn mprotect(addr: *mut c_void, len: usize, prot: i32) -> i32;
    fn sysconf(name: i32) -> i64;
    fn mmap(addr: *mut c_void, len: usize, prot: i32, flags: i32, fd: i32, off: i64)
        -> *mut c_void;
    fn __clear_cache(start: *mut c_void, end: *mut c_void);
}

const PROT_READ: i32 = 1;
const PROT_WRITE: i32 = 2;
const PROT_EXEC: i32 = 4;
const MAP_PRIVATE: i32 = 2;
const MAP_ANONYMOUS: i32 = 0x20;
const SC_PAGESIZE: i32 = 30;

fn page() -> usize {
    unsafe { sysconf(SC_PAGESIZE) as usize }
}

pub fn make_writable(addr: usize, len: usize) -> bool {
    let pg = page();
    let start = addr & !(pg - 1);
    let end = addr + len;
    unsafe {
        mprotect(
            start as *mut c_void,
            end - start,
            PROT_READ | PROT_WRITE | PROT_EXEC,
        ) == 0
    }
}

pub fn flush_icache(addr: usize, len: usize) {
    unsafe { __clear_cache(addr as *mut c_void, (addr + len) as *mut c_void) }
}

pub fn alloc_rwx(len: usize) -> *mut u8 {
    unsafe {
        mmap(
            std::ptr::null_mut(),
            len,
            PROT_READ | PROT_WRITE | PROT_EXEC,
            MAP_PRIVATE | MAP_ANONYMOUS,
            -1,
            0,
        ) as *mut u8
    }
}
