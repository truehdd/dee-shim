//! macOS: page protection, instruction cache and trampoline memory. The DEE tools are PIE, so a
//! link-time VA has the host's load slide added. __TEXT maxprot is r-x, so plain
//! mprotect(WRITE) is refused; mach_vm_protect with VM_PROT_COPY forces the page private and
//! grants write.

use std::ffi::c_void;

extern "C" {
    fn mach_task_self() -> u32;
    fn mach_vm_protect(task: u32, addr: u64, size: u64, set_max: i32, prot: i32) -> i32;
    fn sysconf(name: i32) -> i64;
    fn mmap(addr: *mut c_void, len: usize, prot: i32, flags: i32, fd: i32, off: i64)
        -> *mut c_void;
    fn sys_icache_invalidate(start: *mut c_void, len: usize);
}

const PROT_READ: i32 = 1;
const PROT_WRITE: i32 = 2;
const PROT_EXEC: i32 = 4;
const VM_PROT_COPY: i32 = 0x10;
const MAP_PRIVATE: i32 = 2;
const MAP_ANON: i32 = 0x1000;
const SC_PAGESIZE: i32 = 29;

fn page() -> usize {
    unsafe { sysconf(SC_PAGESIZE) as usize }
}

pub fn make_writable(addr: usize, len: usize) -> bool {
    let pg = page();
    let start = (addr & !(pg - 1)) as u64;
    let end = (addr + len) as u64;
    unsafe {
        let task = mach_task_self();
        // The new protection cannot exceed the mapping's max protection: __TEXT is r-x so rwx|COPY
        // is granted, but __DATA_CONST (where the program-config table and other data-patch sites
        // live) has max rw-, so rwx is refused there. Data patches need no exec, so fall back to
        // rw|COPY for those pages.
        if mach_vm_protect(
            task,
            start,
            end - start,
            0,
            PROT_READ | PROT_WRITE | PROT_EXEC | VM_PROT_COPY,
        ) == 0
        {
            return true;
        }
        mach_vm_protect(
            task,
            start,
            end - start,
            0,
            PROT_READ | PROT_WRITE | VM_PROT_COPY,
        ) == 0
    }
}

pub fn flush_icache(addr: usize, len: usize) {
    unsafe { sys_icache_invalidate(addr as *mut c_void, len) }
}

pub fn alloc_rwx(len: usize) -> *mut u8 {
    unsafe {
        mmap(
            std::ptr::null_mut(),
            len,
            PROT_READ | PROT_WRITE | PROT_EXEC,
            MAP_PRIVATE | MAP_ANON,
            -1,
            0,
        ) as *mut u8
    }
}
