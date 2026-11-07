//! In-process host loader for Windows (see ../mod.rs).
//!
//! Steps, in order:
//!   1. `LoadLibraryExW` maps the untouched tool `.exe` and applies its relocations. It leaves
//!      the imports and the static TLS alone, and never calls the entry point.
//!   2. `link` resolves the imports and points the few that describe the process (command line,
//!      main module, its file name, ExitProcess) at replacements that describe the tool.
//!   3. Exception tables: the loader registers the image's `.pdata`; this is checked, and the
//!      host registers it with RtlAddFunctionTable if the loader did not.
//!   4. `tls` points the image's static TLS accesses at a dynamic TLS slot of the host and gives
//!      it a block on every thread it runs on.
//!   5. `dshim_engine::init_hosted` patches the mapped image.
//!   6. The CRT entry point runs on a new thread with the tool's own stack reserve. Its `exit`
//!      ends the host process with the tool's status, as the native tool would.

mod link;
mod sys;
mod tls;
mod trace;

use dshim_levers::Levers;
use std::ffi::{c_void, OsString};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Map and link `binary`, call `dshim_engine::init_hosted`, then enter the tool with `args`.
/// Returns the tool's exit status, or `super::LOAD_FAILED` if it could not be started.
pub(super) fn run(binary: &Path, args: &[OsString], levers: Levers) -> i32 {
    match start(binary, args, levers) {
        Ok(status) => status,
        Err(error) => {
            eprintln!("dee_shim: in-process host: {}: {error}", binary.display());
            super::LOAD_FAILED
        }
    }
}

/// The tool's CRT entry point, for the thread that runs it.
static ENTRY: AtomicUsize = AtomicUsize::new(0);

/// The path the tool sees as its own. `canonicalize` gives a `\\?\` path, which the tool's
/// own path handling (and the loader name for its module) should not see.
fn plain_path(binary: &Path) -> PathBuf {
    let text = binary.as_os_str().to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        binary.to_path_buf()
    }
}

fn start(binary: &Path, args: &[OsString], levers: Levers) -> Result<i32, String> {
    let path = plain_path(binary);
    let tool_dir = path.parent().ok_or("the tool path has no directory")?;
    let native: Vec<String> = args
        .iter()
        .map(|a| {
            a.to_str()
                .map(String::from)
                .ok_or("an argument is not Unicode")
        })
        .collect::<Result<_, _>>()?;
    let path_text = path.to_str().ok_or("the tool path is not Unicode")?;
    let command = command_line(path_text, &native);

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let base = unsafe { sys::LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), 0) } as usize;
    if base == 0 {
        return Err(format!("LoadLibraryExW: error {}", unsafe {
            sys::GetLastError()
        }));
    }
    let headers = unsafe { sys::Headers::read(base)? };
    link::set_identity(base, &path, &command);
    let redirected = unsafe { link::resolve_imports(&headers, tool_dir)? };
    let unwind = unsafe { exception_tables(&headers)? };
    let tls = match unsafe { tls::prepare(&headers)? } {
        Some(p) => format!("TEB slot {} for {} TLS array loads", p.slot, p.sites),
        None => "none".into(),
    };
    let slide = base as i64 - sys::file_image_base(&path)? as i64;

    dshim_engine::init_hosted(&path, slide, levers);
    log::debug!(
        target: "host",
        "mapped {} at {base:#x} (slide {slide:#x}); redirected {}; unwind tables {unwind}; \
         static TLS {tls}",
        path.display(),
        redirected.join(", ")
    );

    if std::env::var_os("DEE_SHIM_HOST_TRACE").is_some() {
        unsafe { trace::install(base) };
    }
    ENTRY.store(headers.entry, Ordering::Release);
    let thread = unsafe {
        sys::CreateThread(
            std::ptr::null_mut(),
            headers.stack_reserve,
            first_thread,
            std::ptr::null_mut(),
            sys::STACK_SIZE_PARAM_IS_A_RESERVATION,
            std::ptr::null_mut(),
        )
    };
    if thread.is_null() {
        return Err(format!("CreateThread: error {}", unsafe {
            sys::GetLastError()
        }));
    }
    // The tool normally ends the process from `exit`; this is reached only if its entry point
    // returns or its thread exits by itself.
    let mut status = 0u32;
    unsafe {
        sys::WaitForSingleObject(thread, sys::INFINITE);
        sys::GetExitCodeThread(thread, &mut status);
    }
    dshim_log::flush();
    Ok(status as i32)
}

/// Whether unwinding finds the image's `.pdata`, which C++ exceptions inside the tool need.
/// `LoadLibraryExW` puts the image in the loader's function table; if it did not, register it.
unsafe fn exception_tables(headers: &sys::Headers) -> Result<&'static str, String> {
    let mut found_base = 0u64;
    let entry =
        sys::RtlLookupFunctionEntry(headers.entry as u64, &mut found_base, std::ptr::null_mut());
    if !entry.is_null() && found_base as usize == headers.base {
        return Ok("from the loader");
    }
    let Some((table, size)) = headers.dir(sys::DIR_EXCEPTION) else {
        return Err("no exception directory".into());
    };
    // RUNTIME_FUNCTION entries are 12 bytes.
    if sys::RtlAddFunctionTable(
        table as *mut c_void,
        (size / 12) as u32,
        headers.base as u64,
    ) == 0
    {
        return Err("RtlAddFunctionTable failed".into());
    }
    Ok("added with RtlAddFunctionTable")
}

/// The tool's first thread: its TLS block (with DLL_PROCESS_ATTACH callbacks), then the CRT
/// entry point (`mainCRTStartup`, which ignores its argument).
unsafe extern "system" fn first_thread(_parameter: *mut c_void) -> u32 {
    tls::start_first_thread();
    let entry: unsafe extern "system" fn(*mut c_void) -> u32 =
        std::mem::transmute(ENTRY.load(Ordering::Acquire));
    entry(std::ptr::null_mut())
}

/// Quote one command-line argument by the rules CommandLineToArgvW parses (backslashes only
/// matter before a quote). An argument with no space, tab or quote is passed as is.
fn quote_arg(a: &str) -> String {
    if !a.is_empty() && !a.contains([' ', '\t', '"']) {
        return a.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0usize;
    for c in a.chars() {
        match c {
            '\\' => {
                backslashes += 1;
            }
            '"' => {
                // Escape every pending backslash, then the quote.
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                backslashes = 0;
                out.push('"');
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                backslashes = 0;
                out.push(c);
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2)); // before the closing quote
    out.push('"');
    out
}

/// The tool's command line as GetCommandLineW reports it: the binary as argv[0], then the
/// arguments.
fn command_line(binary: &str, args: &[String]) -> String {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(quote_arg(binary));
    parts.extend(args.iter().map(|a| quote_arg(a)));
    parts.join(" ")
}
