//! The libc and ld.so calls the Linux loader makes, declared here so the loader needs no extra
//! crate. Layouts follow glibc on x86_64.

use std::ffi::{c_char, c_int, c_void};

pub const RTLD_NOW: c_int = 2;
pub const RTLD_GLOBAL: c_int = 0x100;

pub const PROT_READ: c_int = 1;
pub const PROT_WRITE: c_int = 2;
pub const PROT_EXEC: c_int = 4;
pub const MAP_PRIVATE: c_int = 2;
pub const MAP_ANONYMOUS: c_int = 0x20;
pub const MAP_FIXED_NOREPLACE: c_int = 0x10_0000;
pub const MAP_FAILED: *mut c_void = !0usize as *mut c_void;

pub const SIGPIPE: c_int = 13;
pub const SIG_DFL: usize = 0;

pub const AT_PHDR: u64 = 3;
pub const AT_PHENT: u64 = 4;
pub const AT_PHNUM: u64 = 5;
pub const AT_ENTRY: u64 = 9;
pub const AT_EXECFN: u64 = 31;

/// `Elf64_Phdr`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Phdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

/// `struct dl_phdr_info`, including the TLS fields glibc has had since 2.4.
#[repr(C)]
pub struct DlPhdrInfo {
    pub addr: u64,
    pub name: *const c_char,
    pub phdr: *const Phdr,
    pub phnum: u16,
    pub adds: u64,
    pub subs: u64,
    pub tls_modid: usize,
    pub tls_data: *mut c_void,
}

/// `Dl_info`.
#[repr(C)]
pub struct DlInfo {
    pub fname: *const c_char,
    pub fbase: *mut c_void,
    pub sname: *const c_char,
    pub saddr: *mut c_void,
}

pub type PhdrCallback = unsafe extern "C" fn(*mut DlPhdrInfo, usize, *mut c_void) -> c_int;

extern "C" {
    pub fn dlopen(file: *const c_char, mode: c_int) -> *mut c_void;
    pub fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    pub fn dlvsym(handle: *mut c_void, name: *const c_char, version: *const c_char) -> *mut c_void;
    pub fn dlerror() -> *mut c_char;
    pub fn dladdr(addr: *const c_void, info: *mut DlInfo) -> c_int;
    pub fn dl_iterate_phdr(callback: PhdrCallback, data: *mut c_void) -> c_int;
    pub fn getauxval(kind: u64) -> u64;
    pub fn readlink(path: *const c_char, buf: *mut c_char, size: usize) -> isize;
    pub fn realpath(path: *const c_char, resolved: *mut c_char) -> *mut c_char;
    pub fn getpid() -> c_int;
    pub fn mmap(
        addr: *mut c_void,
        len: usize,
        prot: c_int,
        flags: c_int,
        fd: c_int,
        off: i64,
    ) -> *mut c_void;
    pub fn munmap(addr: *mut c_void, len: usize) -> c_int;
    pub fn mprotect(addr: *mut c_void, len: usize, prot: c_int) -> c_int;
    pub fn memfd_create(name: *const c_char, flags: u32) -> c_int;
    pub fn close(fd: c_int) -> c_int;
    pub fn atexit(f: extern "C" fn()) -> c_int;
    pub fn exit(status: c_int) -> !;
    pub fn signal(sig: c_int, handler: usize) -> usize;
    pub static environ: *const *const c_char;
}

pub const PAGE: usize = 4096;

pub fn page_down(a: usize) -> usize {
    a & !(PAGE - 1)
}

pub fn page_up(a: usize) -> usize {
    (a + PAGE - 1) & !(PAGE - 1)
}

/// The thread pointer (`%fs:0`, which glibc keeps pointing at itself).
pub fn thread_pointer() -> usize {
    let tp: usize;
    unsafe { std::arch::asm!("mov {}, qword ptr fs:[0]", out(reg) tp, options(nostack, readonly)) };
    tp
}

/// The current `dlerror()` text, or a placeholder.
pub fn last_dl_error() -> String {
    let e = unsafe { dlerror() };
    if e.is_null() {
        "unknown dynamic loader error".into()
    } else {
        unsafe { std::ffi::CStr::from_ptr(e) }
            .to_string_lossy()
            .into_owned()
    }
}

/// `true` if `path` names this process's own executable through /proc.
pub fn is_self_exe(path: *const c_char) -> bool {
    if path.is_null() {
        return false;
    }
    let path = unsafe { std::ffi::CStr::from_ptr(path) }.to_bytes();
    if path == b"/proc/self/exe" || path == b"/proc/thread-self/exe" {
        return true;
    }
    let pid = format!("/proc/{}/exe", unsafe { getpid() });
    path == pid.as_bytes()
}
