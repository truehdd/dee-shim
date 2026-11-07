//! Import resolution for the mapped tool, and the kernel32 calls that would otherwise describe
//! the host instead of the tool.
//!
//! `LoadLibraryExW` maps and relocates an `.exe` but leaves its import address table holding
//! name RVAs, so every import is resolved here. A handful of imports report "the process":
//! its command line, its main module and its file name. Native, those are the tool; in the host
//! they would be `dee_shim`. Those slots point at the replacements below instead. The tool finds
//! `license.lic` through its own module path, so the file name matters. The LoadLibrary family
//! is replaced too: the tool loads `MediaInfo.dll` by bare name, which the native search order
//! finds in the application directory, and in the host that directory is `dee_shim`'s.

use super::sys::{self, Handle};
use std::ffi::{c_void, CStr};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[allow(non_snake_case)]
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleA(name: *const u8) -> Handle;
    fn GetModuleHandleExW(flags: u32, name: *const u16, out: *mut Handle) -> i32;
    fn GetModuleHandleExA(flags: u32, name: *const u8, out: *mut Handle) -> i32;
    fn GetModuleFileNameW(module: Handle, buf: *mut u16, size: u32) -> u32;
    fn GetModuleFileNameA(module: Handle, buf: *mut u8, size: u32) -> u32;
    fn LoadLibraryA(name: *const u8) -> Handle;
    fn LoadLibraryExA(name: *const u8, file: Handle, flags: u32) -> Handle;
}

/// What the replacements report: the tool's base, path, command line and directory.
struct Identity {
    base: usize,
    /// The tool's path, without a terminating NUL.
    path_w: Vec<u16>,
    path_a: Vec<u8>,
    /// The tool's command line, NUL-terminated, argv[0] being its path.
    command_w: Vec<u16>,
    command_a: Vec<u8>,
    /// The tool's directory: the "application directory" of the native DLL search order.
    dir: PathBuf,
}

static IDENTITY: OnceLock<Identity> = OnceLock::new();

fn identity() -> &'static Identity {
    IDENTITY
        .get()
        .expect("host identity is set before the tool runs")
}

/// `text` in the ANSI code page, as the A functions report it (no terminating NUL).
fn ansi(text: &[u16]) -> Vec<u8> {
    if text.is_empty() {
        return Vec::new();
    }
    let len = text.len() as i32;
    let (none, unused) = (std::ptr::null(), std::ptr::null_mut());
    let want = unsafe {
        sys::WideCharToMultiByte(
            sys::CP_ACP,
            0,
            text.as_ptr(),
            len,
            unused,
            0,
            none,
            unused.cast(),
        )
    };
    let mut out = vec![0u8; want.max(0) as usize];
    let got = unsafe {
        sys::WideCharToMultiByte(
            sys::CP_ACP,
            0,
            text.as_ptr(),
            len,
            out.as_mut_ptr(),
            want,
            none,
            unused.cast(),
        )
    };
    out.truncate(got.max(0) as usize);
    out
}

/// Record what the replacements report. `path` is the tool path shown to the tool (no `\\?\`
/// prefix) and `command` its full command line.
pub fn set_identity(base: usize, path: &Path, command: &str) {
    let path_w: Vec<u16> = path.as_os_str().encode_wide().collect();
    let command_w: Vec<u16> = std::ffi::OsStr::new(command).encode_wide().collect();
    let mut command_a = ansi(&command_w);
    command_a.push(0);
    let _ = IDENTITY.set(Identity {
        base,
        path_a: ansi(&path_w),
        path_w,
        command_w: command_w.into_iter().chain([0]).collect(),
        command_a,
        dir: path.parent().map(Path::to_path_buf).unwrap_or_default(),
    });
}

extern "system" fn get_command_line_w() -> *const u16 {
    identity().command_w.as_ptr()
}

extern "system" fn get_command_line_a() -> *const u8 {
    identity().command_a.as_ptr()
}

extern "system" fn get_module_handle_w(name: *const u16) -> Handle {
    if name.is_null() {
        return identity().base as Handle;
    }
    unsafe { sys::GetModuleHandleW(name) }
}

extern "system" fn get_module_handle_a(name: *const u8) -> Handle {
    if name.is_null() {
        return identity().base as Handle;
    }
    unsafe { GetModuleHandleA(name) }
}

extern "system" fn get_module_handle_ex_w(flags: u32, name: *const u16, out: *mut Handle) -> i32 {
    if name.is_null() && flags & sys::GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS == 0 && !out.is_null()
    {
        unsafe { *out = identity().base as Handle };
        return 1;
    }
    unsafe { GetModuleHandleExW(flags, name, out) }
}

extern "system" fn get_module_handle_ex_a(flags: u32, name: *const u8, out: *mut Handle) -> i32 {
    if name.is_null() && flags & sys::GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS == 0 && !out.is_null()
    {
        unsafe { *out = identity().base as Handle };
        return 1;
    }
    unsafe { GetModuleHandleExA(flags, name, out) }
}

/// GetModuleFileName's contract: copy as much as fits with a terminating NUL; on truncation
/// return `size` and set ERROR_INSUFFICIENT_BUFFER, otherwise return the length.
unsafe fn copy_name<T: Copy + Default>(name: &[T], buf: *mut T, size: u32) -> u32 {
    let size = size as usize;
    if size == 0 {
        sys::SetLastError(sys::ERROR_INSUFFICIENT_BUFFER);
        return 0;
    }
    let n = name.len().min(size - 1);
    std::ptr::copy_nonoverlapping(name.as_ptr(), buf, n);
    *buf.add(n) = T::default();
    if n < name.len() {
        sys::SetLastError(sys::ERROR_INSUFFICIENT_BUFFER);
        return size as u32;
    }
    n as u32
}

extern "system" fn get_module_file_name_w(module: Handle, buf: *mut u16, size: u32) -> u32 {
    if module.is_null() {
        return unsafe { copy_name(&identity().path_w, buf, size) };
    }
    unsafe { GetModuleFileNameW(module, buf, size) }
}

extern "system" fn get_module_file_name_a(module: Handle, buf: *mut u8, size: u32) -> u32 {
    if module.is_null() {
        return unsafe { copy_name(&identity().path_a, buf, size) };
    }
    unsafe { GetModuleFileNameA(module, buf, size) }
}

/// The tool's `exit` ends here. The process exits with the tool's status, as it would natively;
/// the engine's log is flushed first because no one else runs after this.
extern "system" fn exit_process(code: u32) -> ! {
    log::debug!(
        target: "host",
        "tool exits with {code}; {} threads ran with its TLS",
        super::tls::threads_seen()
    );
    dshim_log::flush();
    unsafe { sys::ExitProcess(code) }
}

// LoadLibraryEx search flags (LOAD_LIBRARY_SEARCH_*): DLL_LOAD_DIR through DEFAULT_DIRS, and
// the two that include the application directory.
const SEARCH_FLAGS: u32 = 0x1F00;
const SEARCH_WITH_APPLICATION_DIR: u32 = 0x0200 | 0x1000;

/// Where the native loader would find `name` in the application directory, which for the host
/// is `dee_shim`'s directory rather than the tool's. The tool loads `MediaInfo.dll` by bare name
/// and relies on that. Returns the NUL-terminated full path when `name` is a bare file name, the
/// search order includes the application directory, and the file is there.
fn beside_tool(name: &[u16], flags: u32) -> Option<Vec<u16>> {
    if flags & SEARCH_FLAGS != 0 && flags & SEARCH_WITH_APPLICATION_DIR == 0 {
        return None;
    }
    if name.is_empty()
        || name
            .iter()
            .any(|&c| c == '\\' as u16 || c == '/' as u16 || c == ':' as u16)
    {
        return None;
    }
    let mut file = std::ffi::OsString::from_wide(name);
    // As LoadLibrary does, a name without an extension means a .dll.
    if !name.contains(&('.' as u16)) {
        file.push(".dll");
    }
    let path = identity().dir.join(file);
    path.is_file()
        .then(|| path.as_os_str().encode_wide().chain([0]).collect())
}

unsafe fn wide_name(name: *const u16) -> &'static [u16] {
    let mut len = 0;
    while *name.add(len) != 0 {
        len += 1;
    }
    std::slice::from_raw_parts(name, len)
}

/// The A name widened through the ANSI code page, for the search check only.
unsafe fn ansi_name(name: *const u8) -> Vec<u16> {
    let bytes = CStr::from_ptr(name.cast()).to_bytes();
    // MultiByteToWideChar is not needed: a bare file name the tool passes is ASCII; a name that
    // is not ASCII simply goes to the real loader unchanged.
    if bytes.is_ascii() {
        bytes.iter().map(|&b| b as u16).collect()
    } else {
        Vec::new()
    }
}

extern "system" fn load_library_w(name: *const u16) -> Handle {
    unsafe {
        if !name.is_null() {
            if let Some(path) = beside_tool(wide_name(name), 0) {
                return sys::LoadLibraryW(path.as_ptr());
            }
        }
        sys::LoadLibraryW(name)
    }
}

extern "system" fn load_library_ex_w(name: *const u16, file: Handle, flags: u32) -> Handle {
    unsafe {
        if !name.is_null() {
            if let Some(path) = beside_tool(wide_name(name), flags) {
                return sys::LoadLibraryExW(path.as_ptr(), file, flags);
            }
        }
        sys::LoadLibraryExW(name, file, flags)
    }
}

extern "system" fn load_library_a(name: *const u8) -> Handle {
    unsafe {
        if !name.is_null() {
            if let Some(path) = beside_tool(&ansi_name(name), 0) {
                return sys::LoadLibraryW(path.as_ptr());
            }
        }
        LoadLibraryA(name)
    }
}

extern "system" fn load_library_ex_a(name: *const u8, file: Handle, flags: u32) -> Handle {
    unsafe {
        if !name.is_null() {
            if let Some(path) = beside_tool(&ansi_name(name), flags) {
                return sys::LoadLibraryExW(path.as_ptr(), file, flags);
            }
        }
        LoadLibraryExA(name, file, flags)
    }
}

/// The replacement for an import, by name, if it is one that describes the process or searches
/// its application directory.
fn replacement(name: &[u8]) -> Option<usize> {
    Some(match name {
        b"GetCommandLineW" => get_command_line_w as *const () as usize,
        b"GetCommandLineA" => get_command_line_a as *const () as usize,
        b"GetModuleHandleW" => get_module_handle_w as *const () as usize,
        b"GetModuleHandleA" => get_module_handle_a as *const () as usize,
        b"GetModuleHandleExW" => get_module_handle_ex_w as *const () as usize,
        b"GetModuleHandleExA" => get_module_handle_ex_a as *const () as usize,
        b"GetModuleFileNameW" => get_module_file_name_w as *const () as usize,
        b"GetModuleFileNameA" => get_module_file_name_a as *const () as usize,
        b"ExitProcess" => exit_process as *const () as usize,
        b"LoadLibraryW" => load_library_w as *const () as usize,
        b"LoadLibraryA" => load_library_a as *const () as usize,
        b"LoadLibraryExW" => load_library_ex_w as *const () as usize,
        b"LoadLibraryExA" => load_library_ex_a as *const () as usize,
        _ => return None,
    })
}

/// Load a DLL the tool imports. Natively the loader searches the tool's own directory first, so
/// a DLL found there is loaded by its full path; anything else by name (system DLLs).
fn load_dependency(tool_dir: &Path, dll: &str) -> Handle {
    let beside = tool_dir.join(dll);
    let path: Vec<u16> = if beside.is_file() {
        beside.as_os_str().encode_wide().chain([0]).collect()
    } else {
        std::ffi::OsStr::new(dll).encode_wide().chain([0]).collect()
    };
    unsafe { sys::LoadLibraryW(path.as_ptr()) }
}

/// Resolve every import of the image at `headers.base` and write its IAT. Returns the names of
/// the imports that were redirected to the replacements above.
pub unsafe fn resolve_imports(
    headers: &sys::Headers,
    tool_dir: &Path,
) -> Result<Vec<String>, String> {
    let base = headers.base;
    let Some((mut descriptor, _)) = headers.dir(sys::DIR_IMPORT) else {
        return Ok(Vec::new());
    };
    let mut redirected = Vec::new();
    // IMAGE_IMPORT_DESCRIPTOR: OriginalFirstThunk, TimeDateStamp, ForwarderChain, Name, FirstThunk.
    while sys::rd32(descriptor + 12) != 0 {
        let dll = CStr::from_ptr((base + sys::rd32(descriptor + 12) as usize) as *const _)
            .to_string_lossy()
            .into_owned();
        let module = load_dependency(tool_dir, &dll);
        if module.is_null() {
            return Err(format!("load {dll}: error {}", sys::GetLastError()));
        }
        let lookup = match sys::rd32(descriptor) {
            0 => sys::rd32(descriptor + 16),
            rva => rva,
        } as usize;
        let iat = base + sys::rd32(descriptor + 16) as usize;
        let mut count = 0;
        while sys::rd64(base + lookup + count * 8) != 0 {
            count += 1;
        }
        let mut old = 0;
        if sys::VirtualProtect(iat as *mut c_void, count * 8, sys::PAGE_READWRITE, &mut old) == 0 {
            return Err(format!(
                "make the {dll} IAT writable: error {}",
                sys::GetLastError()
            ));
        }
        for i in 0..count {
            let thunk = sys::rd64(base + lookup + i * 8);
            let (address, label) = if thunk >> 63 == 1 {
                let ordinal = (thunk & 0xFFFF) as usize;
                let address = sys::GetProcAddress(module, ordinal as *const u8) as usize;
                (address, format!("{dll}#{ordinal}"))
            } else {
                // IMAGE_IMPORT_BY_NAME: a u16 hint, then the name.
                let name = CStr::from_ptr((base + thunk as usize + 2) as *const _);
                match replacement(name.to_bytes()) {
                    Some(address) => {
                        redirected.push(name.to_string_lossy().into_owned());
                        (address, String::new())
                    }
                    None => (
                        sys::GetProcAddress(module, name.as_ptr().cast()) as usize,
                        format!("{dll}!{}", name.to_string_lossy()),
                    ),
                }
            };
            if address == 0 {
                return Err(format!("unresolved import {label}"));
            }
            ((iat + i * 8) as *mut u64).write(address as u64);
        }
        sys::VirtualProtect(iat as *mut c_void, count * 8, old, &mut old);
        descriptor += 20;
    }
    Ok(redirected)
}
