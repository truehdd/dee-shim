//! What ld.so would have told the tool about itself, and the symbol binding it would have done.
//!
//! The tool runs inside `dee_shim`, so libc's answers about "the program" describe dee_shim: its
//! path (`/proc/self/exe`, `AT_EXECFN`), its program headers, its place in `dl_iterate_phdr`
//! and `dladdr`, its RPATH for `dlopen`. The tools use these to find libmediainfo, the default
//! license and (the TrueHD port) their own unwind tables. Those imports are bound to the
//! wrappers here, which answer for the tool and pass every other question through.
//!
//! `interpose` does the other half of COPY relocations: under ld.so the libraries' references
//! to an object the executable copied (std::cout, libstdc++ vtables and typeinfo, stdout,
//! `__environ`) bind to the executable's copy. Here the libraries were linked before the tool
//! existed, so their GOT slots are rebound to the copies afterwards.

use super::elf::Copy;
use super::sys::{self, DlInfo, DlPhdrInfo, Phdr, PhdrCallback};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The tool as the wrappers describe it.
pub struct Tool {
    pub path: CString,
    pub search: Vec<PathBuf>,
    pub ranges: Vec<(usize, usize)>,
    pub base: usize,
    pub phdr: usize,
    pub phnum: u16,
    pub entry: usize,
    pub tls: Option<super::tls::Block>,
    pub copies: Vec<Copy>,
}

static TOOL: OnceLock<Tool> = OnceLock::new();

/// Record the tool. Must happen before any of its code runs; the wrappers rely on it.
pub fn set(tool: Tool) {
    let _ = TOOL.set(tool);
}

fn tool() -> &'static Tool {
    TOOL.get().expect("in-process host state")
}

/// The wrapper bound in place of import `name`, if there is one.
pub fn replacement(name: &CStr) -> Option<usize> {
    let f: usize = match name.to_bytes() {
        b"readlink" => readlink as *const () as usize,
        b"realpath" => realpath as *const () as usize,
        b"getauxval" => getauxval as *const () as usize,
        b"dladdr" => dladdr as *const () as usize,
        b"dl_iterate_phdr" => dl_iterate_phdr as *const () as usize,
        b"dlopen" => dlopen as *const () as usize,
        _ => return None,
    };
    Some(f)
}

/// `dlopen` as seen from the tool: a bare name is looked for in its RPATH/RUNPATH first, which
/// ld.so takes from the calling object and libc cannot find for a hand-mapped one.
pub fn open_library(name: &str, search: &[PathBuf], mode: c_int) -> *mut c_void {
    if !name.contains('/') {
        for dir in search {
            let candidate = dir.join(name);
            if candidate.is_file() {
                if let Ok(path) = CString::new(candidate.as_os_str().as_encoded_bytes()) {
                    let handle = unsafe { sys::dlopen(path.as_ptr(), mode) };
                    if !handle.is_null() {
                        return handle;
                    }
                }
            }
        }
    }
    match CString::new(name) {
        Ok(name) => unsafe { sys::dlopen(name.as_ptr(), mode) },
        Err(_) => std::ptr::null_mut(),
    }
}

unsafe extern "C" fn readlink(path: *const c_char, buf: *mut c_char, size: usize) -> isize {
    if !sys::is_self_exe(path) {
        return sys::readlink(path, buf, size);
    }
    let own = tool().path.as_bytes();
    let n = own.len().min(size);
    std::ptr::copy_nonoverlapping(own.as_ptr(), buf as *mut u8, n);
    n as isize
}

unsafe extern "C" fn realpath(path: *const c_char, resolved: *mut c_char) -> *mut c_char {
    if sys::is_self_exe(path) {
        return sys::realpath(tool().path.as_ptr(), resolved);
    }
    sys::realpath(path, resolved)
}

unsafe extern "C" fn getauxval(kind: u64) -> u64 {
    let t = tool();
    match kind {
        sys::AT_PHDR => t.phdr as u64,
        sys::AT_PHENT => std::mem::size_of::<Phdr>() as u64,
        sys::AT_PHNUM => t.phnum as u64,
        sys::AT_ENTRY => t.entry as u64,
        sys::AT_EXECFN => t.path.as_ptr() as u64,
        _ => sys::getauxval(kind),
    }
}

fn in_tool(addr: usize) -> bool {
    tool().ranges.iter().any(|(a, b)| (*a..*b).contains(&addr))
}

unsafe extern "C" fn dladdr(addr: *const c_void, info: *mut DlInfo) -> c_int {
    if !in_tool(addr as usize) {
        return sys::dladdr(addr, info);
    }
    // glibc reports the main program by its argv[0], which is the tool's path here.
    let info = &mut *info;
    info.fname = tool().path.as_ptr();
    info.fbase = tool().base as *mut c_void;
    info.sname = std::ptr::null();
    info.saddr = std::ptr::null_mut();
    1
}

struct Iteration {
    callback: PhdrCallback,
    data: *mut c_void,
    first: bool,
}

/// Reports the tool first, as ld.so lists the main program first, then every real object.
unsafe extern "C" fn iterate_step(info: *mut DlPhdrInfo, size: usize, data: *mut c_void) -> c_int {
    let it = &mut *(data as *mut Iteration);
    if it.first {
        it.first = false;
        let t = tool();
        let real = &*info;
        let mut own = DlPhdrInfo {
            addr: 0,
            name: c"".as_ptr(),
            phdr: t.phdr as *const Phdr,
            phnum: t.phnum,
            adds: real.adds,
            subs: real.subs,
            tls_modid: t.tls.map_or(0, |b| b.modid),
            tls_data: t.tls.map_or(std::ptr::null_mut(), |b| {
                (sys::thread_pointer() - b.offset as usize) as *mut c_void
            }),
        };
        let r = (it.callback)(&mut own, size, it.data);
        if r != 0 {
            return r;
        }
    }
    (it.callback)(info, size, it.data)
}

unsafe extern "C" fn dl_iterate_phdr(callback: PhdrCallback, data: *mut c_void) -> c_int {
    let mut it = Iteration {
        callback,
        data,
        first: true,
    };
    sys::dl_iterate_phdr(iterate_step, &mut it as *mut Iteration as *mut c_void)
}

unsafe extern "C" fn dlopen(file: *const c_char, mode: c_int) -> *mut c_void {
    let handle = if file.is_null() {
        sys::dlopen(file, mode)
    } else {
        let name = CStr::from_ptr(file).to_string_lossy().into_owned();
        open_library(&name, &tool().search, mode)
    };
    if !handle.is_null() {
        interpose_tool();
    }
    handle
}

/// `interpose` for the recorded tool's COPY relocations.
pub fn interpose_tool() {
    if !tool().copies.is_empty() {
        interpose(&tool().copies);
    }
}

/// Rebind every loaded object's symbol relocations that point at a COPY source to the tool's
/// copy, keeping the addend. Idempotent, so it is rerun after each `dlopen` the tool makes.
fn interpose(copies: &[Copy]) {
    unsafe extern "C" fn each(info: *mut DlPhdrInfo, _: usize, data: *mut c_void) -> c_int {
        let copies = &*(data as *const &[Copy]);
        rebind(&*info, copies);
        0
    }
    let mut copies = copies;
    unsafe {
        sys::dl_iterate_phdr(each, &mut copies as *mut &[Copy] as *mut c_void);
    }
}

unsafe fn rebind(info: &DlPhdrInfo, copies: &[Copy]) {
    let base = info.addr as usize;
    let phdrs = std::slice::from_raw_parts(info.phdr, info.phnum as usize);
    let Some(dynamic) = phdrs.iter().find(|p| p.p_type == 2) else {
        return;
    };
    let relro = phdrs.iter().find(|p| p.p_type == 0x6474_e552).map(|p| {
        (
            sys::page_down(base + p.p_vaddr as usize),
            sys::page_down(base + (p.p_vaddr + p.p_memsz) as usize),
        )
    });
    // glibc has usually rewritten these to absolute addresses in place; a read-only dynamic
    // section (the vDSO) still holds offsets.
    let absolute = |v: u64| {
        if (v as usize) < base {
            base + v as usize
        } else {
            v as usize
        }
    };
    let (mut symtab, mut rela, mut relasz, mut jmprel, mut pltrelsz) = (0, 0, 0, 0, 0);
    let mut at = base + dynamic.p_vaddr as usize;
    loop {
        let tag = *(at as *const i64);
        let value = *((at + 8) as *const u64);
        match tag {
            0 => break,
            6 => symtab = absolute(value),
            7 => rela = absolute(value),
            8 => relasz = value as usize,
            23 => jmprel = absolute(value),
            2 => pltrelsz = value as usize,
            _ => {}
        }
        at += 16;
    }
    if symtab == 0 {
        return;
    }
    for (start, size) in [(rela, relasz), (jmprel, pltrelsz)] {
        if start == 0 {
            continue;
        }
        for i in 0..size / 24 {
            let r = start + i * 24;
            let offset = *(r as *const u64) as usize;
            let info = *((r + 8) as *const u64);
            let addend = *((r + 16) as *const i64);
            let kind = info as u32;
            if (info >> 32) == 0 || !matches!(kind, 1 | 6 | 7) {
                continue;
            }
            let slot = (base + offset) as *mut usize;
            let target = (*slot as i64).wrapping_sub(addend) as usize;
            let Some(copy) = copies.iter().find(|c| c.from == target) else {
                continue;
            };
            let value = (copy.to as i64).wrapping_add(addend) as usize;
            let page = sys::page_down(slot as usize);
            let in_relro = relro.is_some_and(|(a, b)| (a..b).contains(&page));
            if in_relro {
                sys::mprotect(
                    page as *mut c_void,
                    sys::PAGE,
                    sys::PROT_READ | sys::PROT_WRITE,
                );
            }
            *slot = value;
            if in_relro {
                sys::mprotect(page as *mut c_void, sys::PAGE, sys::PROT_READ);
            }
        }
    }
}

/// The tool's path as a C string.
pub fn path_c(path: &Path) -> Result<CString, String> {
    CString::new(path.as_os_str().as_encoded_bytes()).map_err(|_| "path contains NUL".into())
}
