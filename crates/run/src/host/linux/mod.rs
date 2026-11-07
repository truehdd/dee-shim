//! In-process host loader for Linux (see ../mod.rs).
//!
//! The DEE Linux tools are non-PIE `ET_EXEC` images linked at 0x400000 (the TrueHD port also at
//! 0x100000000), which `dlopen` refuses, so this is a small ELF loader for that one shape. ld.so
//! still loads the shared libraries the tool needs; the loader does the rest of what ld.so does
//! for a main program:
//!
//! - `tls`: give the tool's local-exec TLS block its native place below the thread pointer
//!   (this may restart the host once through ld.so, see there);
//! - `elf`: map the segments at their link addresses, `dlopen` DT_NEEDED, apply the RELA
//!   relocations with versioned lookups, protect RELRO, register `.eh_frame` with libgcc, run
//!   the constructors, find `main` in `_start`;
//! - `hooks`: answer the tool's questions about itself (its path, program headers, unwind
//!   tables, RPATH) and bind the libraries to its COPY relocations.
//!
//! Then the engine patches the mapped image (`dshim_engine::init_hosted`) and the tool's `main`
//! runs on this thread; its `exit` ends the host child with the tool's status.

mod elf;
mod hooks;
mod sys;
mod tls;

use dshim_levers::Levers;
use std::ffi::{c_char, c_int, CString, OsString};
use std::path::Path;

/// Map and link `binary`, call `dshim_engine::init_hosted`, then enter the tool with `args`.
/// Returns the tool's exit status, or `super::LOAD_FAILED` if it could not be started.
pub(super) fn run(binary: &Path, args: &[OsString], levers: Levers) -> i32 {
    match start(binary, args, levers) {
        Ok(never) => match never {},
        Err(error) => {
            eprintln!("dee_shim: in-process host: {}: {error}", binary.display());
            super::LOAD_FAILED
        }
    }
}

fn start(
    binary: &Path,
    args: &[OsString],
    levers: Levers,
) -> Result<std::convert::Infallible, String> {
    let data = std::fs::read(binary).map_err(|e| e.to_string())?;
    let image = elf::Image::parse(&data)?;
    drop(data);
    let block = match &image.tls {
        Some(template) => Some(tls::ensure(binary, args, &levers, template)?),
        None => None,
    };
    image.map(binary)?;
    let dynamic = elf::Dynamic::read(&image, binary)?;
    let libs = elf::load_needed(&dynamic)?;
    let copies = elf::relocate(&dynamic, &libs, block.map(|b| b.offset))?;
    image.protect()?;
    hooks::set(hooks::Tool {
        path: hooks::path_c(binary)?,
        search: dynamic.search.clone(),
        ranges: image.ranges(),
        base: image.base(),
        phdr: image.phdr_addr as usize,
        phnum: image.phdrs.len() as u16,
        entry: image.entry as usize,
        tls: block,
        copies,
    });
    hooks::interpose_tool();
    image.register_eh_frame()?;
    let main = image.main()?;

    dshim_engine::init_hosted(binary, 0, levers);

    // Rust ignores SIGPIPE at startup; the tool expects the default, as when run natively.
    unsafe { sys::signal(sys::SIGPIPE, sys::SIG_DFL) };
    let mut argv: Vec<*const c_char> = Vec::with_capacity(args.len() + 2);
    for arg in std::iter::once(binary.as_os_str()).chain(args.iter().map(|a| a.as_os_str())) {
        let arg = CString::new(arg.as_encoded_bytes())
            .map_err(|_| "argument contains NUL".to_string())?;
        // The tool may keep argv pointers for the life of the process.
        argv.push(arg.into_raw());
    }
    let argc = argv.len() as c_int;
    argv.push(std::ptr::null());
    let argv = argv.leak().as_ptr();
    unsafe {
        let envp = sys::environ;
        elf::run_init(&dynamic, argc, argv, envp);
        let main: unsafe extern "C" fn(c_int, *const *const c_char, *const *const c_char) -> c_int =
            std::mem::transmute(main);
        let status = main(argc, argv, envp);
        sys::exit(status)
    }
}
