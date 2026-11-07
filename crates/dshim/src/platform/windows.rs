//! Windows: page protection, instruction cache and trampoline memory.

use std::ffi::c_void;

#[allow(non_snake_case)]
extern "system" {
    fn VirtualProtect(addr: *mut c_void, size: usize, new: u32, old: *mut u32) -> i32;
    fn VirtualAlloc(addr: *mut c_void, size: usize, typ: u32, protect: u32) -> *mut c_void;
    fn GetCurrentProcess() -> *mut c_void;
    fn FlushInstructionCache(proc_: *mut c_void, addr: *const c_void, size: usize) -> i32;
}

const PAGE_EXECUTE_READWRITE: u32 = 0x40;
const MEM_COMMIT_RESERVE: u32 = 0x1000 | 0x2000;

pub fn make_writable(addr: usize, len: usize) -> bool {
    let mut old = 0u32;
    unsafe { VirtualProtect(addr as *mut c_void, len, PAGE_EXECUTE_READWRITE, &mut old) != 0 }
}

pub fn flush_icache(addr: usize, len: usize) {
    unsafe {
        FlushInstructionCache(GetCurrentProcess(), addr as *const c_void, len);
    }
}

pub fn alloc_rwx(len: usize) -> *mut u8 {
    unsafe {
        VirtualAlloc(
            std::ptr::null_mut(),
            len,
            MEM_COMMIT_RESERVE,
            PAGE_EXECUTE_READWRITE,
        ) as *mut u8
    }
}
