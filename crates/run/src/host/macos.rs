//! In-process host loader for macOS (see mod.rs).
//!
//! The DEE tools are PIE `MH_EXECUTE` images, and dyld will `dlopen` an executable into a
//! non-hardened x86_64 process. dyld maps, rebases and binds the image (its own
//! `LC_RPATH @loader_path/` finds libmediainfo beside it) and runs its static initialisers. The
//! host then reads the slide from the dyld image list, hands it to the engine, points the image's
//! executable-path imports at the tool, and calls `main` at the `LC_MAIN` entry offset.

use dshim_levers::Levers;
use std::ffi::{c_void, CStr, CString, OsString};
use std::os::raw::{c_char, c_int};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlerror() -> *const c_char;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn signal(signum: c_int, handler: usize) -> usize;
    fn mach_task_self() -> u32;
    fn mach_vm_protect(task: u32, addr: u64, size: u64, set_max: i32, prot: i32) -> i32;

    fn _dyld_image_count() -> u32;
    fn _dyld_get_image_header(i: u32) -> *const c_void;
    fn _dyld_get_image_vmaddr_slide(i: u32) -> isize;
    fn _dyld_get_image_name(i: u32) -> *const c_char;

    static environ: *const *const c_char;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFBundleGetMainBundle() -> *const c_void;
    fn CFBundleCopyExecutableURL(bundle: *const c_void) -> *const c_void;
    fn CFURLCreateFromFileSystemRepresentation(
        allocator: *const c_void,
        buffer: *const u8,
        length: isize,
        is_directory: u8,
    ) -> *const c_void;
}

const RTLD_NOW: c_int = 2;
const RTLD_LOCAL: c_int = 4;
const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;
const SIGPIPE: c_int = 13;
const SIG_DFL: usize = 0;
const MH_MAGIC_64: u32 = 0xfeed_facf;
const LC_SEGMENT_64: u32 = 0x19;
const LC_MAIN: u32 = 0x8000_0028;
const VM_PROT_READ: i32 = 1;
const VM_PROT_WRITE: i32 = 2;
const VM_PROT_COPY: i32 = 0x10;

type Main = unsafe extern "C" fn(c_int, *const *const c_char, *const *const c_char) -> c_int;

/// The tool's own path, NUL terminated, reported by the redirected executable-path imports.
static TOOL_PATH: std::sync::OnceLock<CString> = std::sync::OnceLock::new();

/// Map and link `binary`, call `dshim_engine::init_hosted`, then enter the tool with `args`.
/// Returns the tool's exit status, or `super::LOAD_FAILED` if it could not be started.
pub(super) fn run(binary: &Path, args: &[OsString], levers: Levers) -> i32 {
    match load(binary, levers) {
        Ok(main) => enter(main, binary, args),
        Err(error) => {
            eprintln!("dee_shim: in-process host: {}: {error}", binary.display());
            super::LOAD_FAILED
        }
    }
}

/// `dlopen` the tool, patch it, and return its `main`.
fn load(binary: &Path, levers: Levers) -> Result<Main, String> {
    let path = CString::new(binary.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let _ = TOOL_PATH.set(path.clone());
    let handle = unsafe { dlopen(path.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
    if handle.is_null() {
        let reason = unsafe { dlerror() };
        let reason = if reason.is_null() {
            "unknown error".into()
        } else {
            unsafe { CStr::from_ptr(reason) }
                .to_string_lossy()
                .into_owned()
        };
        return Err(format!("dlopen: {reason}"));
    }
    let (header, slide) = image(binary).ok_or("dlopen succeeded but dyld does not list it")?;
    let commands = Commands::new(header)?;
    let entry = commands.entry().ok_or("no LC_MAIN entry point")?;
    // The engine exits on a fail-closed error. It also sets up logging, which the retarget below
    // reports through.
    dshim_engine::init_hosted(binary, slide as i64, levers);
    retarget_executable_path(&commands, slide)?;
    Ok(unsafe { std::mem::transmute::<usize, Main>(header + entry as usize) })
}

/// Call the tool's `main` with argv[0] set to its own path.
fn enter(main: Main, binary: &Path, args: &[OsString]) -> i32 {
    let mut owned = vec![CString::new(binary.as_os_str().as_bytes()).unwrap_or_default()];
    for arg in args {
        match CString::new(arg.as_bytes()) {
            Ok(arg) => owned.push(arg),
            Err(_) => {
                eprintln!("dee_shim: in-process host: an argument contains a NUL byte");
                return super::LOAD_FAILED;
            }
        }
    }
    let mut argv: Vec<*const c_char> = owned.iter().map(|a| a.as_ptr()).collect();
    argv.push(std::ptr::null());
    // Rust ignores SIGPIPE before main; the native tool runs with the default action.
    unsafe {
        signal(SIGPIPE, SIG_DFL);
        main(owned.len() as c_int, argv.as_ptr(), environ)
    }
}

/// The dyld image list entry for `binary`: header address and slide.
fn image(binary: &Path) -> Option<(usize, isize)> {
    let count = unsafe { _dyld_image_count() };
    // The newest entry is the one just loaded, so search from the end.
    for i in (0..count).rev() {
        let name = unsafe { _dyld_get_image_name(i) };
        if name.is_null() {
            continue;
        }
        let name = Path::new(std::ffi::OsStr::from_bytes(
            unsafe { CStr::from_ptr(name) }.to_bytes(),
        ));
        if name == binary || name.canonicalize().ok().as_deref() == Some(binary) {
            let header = unsafe { _dyld_get_image_header(i) } as usize;
            let slide = unsafe { _dyld_get_image_vmaddr_slide(i) };
            return (header != 0).then_some((header, slide));
        }
    }
    None
}

/// The load commands of a mapped 64-bit Mach-O image.
struct Commands {
    header: usize,
    ncmds: u32,
}

impl Commands {
    fn new(header: usize) -> Result<Commands, String> {
        let magic = unsafe { (header as *const u32).read() };
        if magic != MH_MAGIC_64 {
            return Err(format!("not a 64-bit Mach-O image (magic {magic:#x})"));
        }
        let ncmds = unsafe { ((header + 16) as *const u32).read() };
        Ok(Commands { header, ncmds })
    }

    /// Each load command as (cmd, address of the command).
    fn iter(&self) -> impl Iterator<Item = (u32, usize)> + '_ {
        let mut at = self.header + 32;
        (0..self.ncmds).map(move |_| {
            let cmd = unsafe { (at as *const u32).read() };
            let size = unsafe { ((at + 4) as *const u32).read() } as usize;
            let here = at;
            at += size;
            (cmd, here)
        })
    }

    /// `LC_MAIN` entryoff: the offset of `main` from the `__TEXT` start, which is the header.
    fn entry(&self) -> Option<u64> {
        self.iter()
            .find(|&(cmd, _)| cmd == LC_MAIN)
            .map(|(_, at)| unsafe { ((at + 8) as *const u64).read_unaligned() })
    }

    /// Pointer sections that hold bound imports: `__got`, `__auth_got`, `__la_symbol_ptr`, as
    /// (link-time address, size).
    fn import_slots(&self) -> Vec<(u64, u64)> {
        let mut out = Vec::new();
        for (cmd, at) in self.iter() {
            if cmd != LC_SEGMENT_64 {
                continue;
            }
            let nsects = unsafe { ((at + 64) as *const u32).read() };
            for s in 0..nsects as usize {
                let section = at + 72 + s * 80;
                let name = unsafe { std::slice::from_raw_parts(section as *const u8, 16) };
                let name = name.split(|&b| b == 0).next().unwrap_or_default();
                if matches!(name, b"__got" | b"__auth_got" | b"__la_symbol_ptr") {
                    let addr = unsafe { ((section + 32) as *const u64).read_unaligned() };
                    let size = unsafe { ((section + 40) as *const u64).read_unaligned() };
                    out.push((addr, size));
                }
            }
        }
        out
    }
}

/// The tools find their own location through `_NSGetExecutablePath` and
/// `CFBundleCopyExecutableURL(CFBundleGetMainBundle())`. In a host both name dee_shim, so point
/// the tool's import slots for them at replacements that name the tool. Only this image's slots
/// change; dee_shim and the system libraries keep the real functions.
fn retarget_executable_path(commands: &Commands, slide: isize) -> Result<(), String> {
    let targets: [(&CStr, usize); 2] = [
        (
            c"_NSGetExecutablePath",
            ns_get_executable_path as *const () as usize,
        ),
        (
            c"CFBundleCopyExecutableURL",
            bundle_copy_executable_url as *const () as usize,
        ),
    ];
    let sections = commands.import_slots();
    for (symbol, replacement) in targets {
        let real = unsafe { dlsym(RTLD_DEFAULT, symbol.as_ptr()) } as usize;
        if real == 0 {
            return Err(format!("dlsym {}", symbol.to_string_lossy()));
        }
        for &(addr, size) in &sections {
            let start = (addr as i64 + slide as i64) as usize;
            for slot in (start..start + size as usize).step_by(8) {
                if unsafe { (slot as *const usize).read() } != real {
                    continue;
                }
                write_slot(slot, replacement)?;
                log::debug!(target: "dshim", "host: import {} at {slot:#x} now names the tool", symbol.to_string_lossy());
            }
        }
    }
    Ok(())
}

/// Write one pointer into a bound import slot. `__DATA_CONST` is read only after binding, so
/// take a private copy of the page first, as the engine does for its data patches.
fn write_slot(slot: usize, value: usize) -> Result<(), String> {
    let page = slot & !0xfff;
    let kr = unsafe {
        mach_vm_protect(
            mach_task_self(),
            page as u64,
            0x1000,
            0,
            VM_PROT_READ | VM_PROT_WRITE | VM_PROT_COPY,
        )
    };
    if kr != 0 {
        return Err(format!("make import slot {slot:#x} writable: kern {kr}"));
    }
    unsafe { (slot as *mut usize).write(value) };
    Ok(())
}

/// `_NSGetExecutablePath` as the tool would see it when run natively.
unsafe extern "C" fn ns_get_executable_path(buf: *mut c_char, size: *mut u32) -> c_int {
    let path = TOOL_PATH
        .get()
        .map(|p| p.as_bytes_with_nul())
        .unwrap_or(b"\0");
    let need = path.len() as u32;
    if buf.is_null() || size.is_null() || *size < need {
        if !size.is_null() {
            *size = need;
        }
        return -1;
    }
    std::ptr::copy_nonoverlapping(path.as_ptr(), buf as *mut u8, path.len());
    0
}

/// `CFBundleCopyExecutableURL`: the main bundle's executable is the tool; any other bundle is
/// answered by CoreFoundation.
unsafe extern "C" fn bundle_copy_executable_url(bundle: *const c_void) -> *const c_void {
    if !bundle.is_null() && bundle == CFBundleGetMainBundle() {
        if let Some(path) = TOOL_PATH.get() {
            let bytes = path.as_bytes();
            return CFURLCreateFromFileSystemRepresentation(
                std::ptr::null(),
                bytes.as_ptr(),
                bytes.len() as isize,
                0,
            );
        }
    }
    CFBundleCopyExecutableURL(bundle)
}
