//! The tool's own static TLS block.
//!
//! A non-PIE executable reaches its thread-local variables with local-exec code: fixed
//! negative offsets from the thread pointer, inside `[tp - N, tp)` where N is its block's
//! offset as the main program (16 bytes for ddp and ddpjoc, 0x530 for the TrueHD port). In the
//! host the main program is `dee_shim`, whose own block normally ends right at `tp`, so the tool
//! would overwrite dee_shim's (Rust std's) thread-locals on every thread.
//!
//! Patching the access sites is not practical: the TrueHD port alone has about 100 of them in a
//! dozen shapes (direct `%fs:disp32`, `%fs:0` loads followed by displacements, offsets kept in
//! registers), so a patcher would need a decoder and data flow, per build. Instead glibc is
//! made to lay the block out exactly where the code expects it:
//!
//! 1. `dee_shim`'s TLS segment gets an 8 KiB alignment (`GAP_MARKER` below). glibc places a
//!    main program's block at `roundup(size, align)` below `tp`, so this leaves a free gap of
//!    nearly 8 KiB directly under `tp`.
//! 2. glibc fills that gap with the next startup modules' TLS blocks, in module order, the
//!    first at `roundup(size, align)` below `tp`: the same formula that placed the tool's block
//!    when it was the main program. So the host child starts itself again through ld.so with
//!    `--preload` of a stub shared object built in memory (memfd, nothing on disk) whose only
//!    content is a PT_TLS with the tool's size, alignment and `.tdata` image. As the first
//!    preloaded module it lands at exactly `[tp - N, tp)`.
//! 3. glibc then owns the block: it is initialised from the tool's template on every thread,
//!    whoever creates it and whatever stack it reuses. Stage two checks the placement through
//!    `dl_iterate_phdr` before mapping the tool and refuses to run if it differs.
//!
//! The marker costs every dee_shim thread about 16 KiB of static TLS, in every mode.

use super::elf::Tls;
use super::sys;
use dshim_levers::Levers;
use std::ffi::{c_int, c_void, OsString};
use std::io::Write;
use std::os::fd::FromRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::Path;

/// Names the stub's file descriptor in the second-stage host. Its presence means the stub is
/// already loaded.
const STAGE_ENV: &str = "DEE_SHIM_HOST_TLS";

/// The largest tool TLS block the gap can take, leaving room for dee_shim's own variables after
/// the marker.
const GAP: u64 = 8192;

std::arch::global_asm!(
    ".pushsection .tbss.dee_shim_tls_gap,\"awT\",@nobits",
    ".p2align 13",
    "dee_shim_tls_gap:",
    ".zero 8",
    ".popsection",
    ".pushsection .text.dee_shim_tls_gap_tpoff,\"ax\",@progbits",
    ".globl dee_shim_tls_gap_tpoff",
    ".hidden dee_shim_tls_gap_tpoff",
    "dee_shim_tls_gap_tpoff:",
    "movq $dee_shim_tls_gap@tpoff, %rax",
    "ret",
    ".popsection",
    options(att_syntax)
);

extern "C" {
    /// The marker's offset from the thread pointer. Calling it also keeps the marker linked.
    fn dee_shim_tls_gap_tpoff() -> isize;
}

/// Where glibc put the tool's TLS block: its static TLS module id and its offset below `tp`.
#[derive(Clone, Copy)]
pub struct Block {
    pub modid: usize,
    pub offset: u64,
}

/// Stage one starts the host again with the stub preloaded and does not return unless that
/// fails. Stage two checks the stub's block and returns it.
pub fn ensure(
    binary: &Path,
    args: &[OsString],
    levers: &Levers,
    tls: &Tls,
) -> Result<Block, String> {
    if tls.tp_offset() + 256 > GAP {
        return Err(format!(
            "the tool's TLS block ({:#x} bytes) does not fit dee_shim's {GAP:#x}-byte gap",
            tls.tp_offset()
        ));
    }
    match std::env::var(STAGE_ENV) {
        Ok(fd) => {
            std::env::remove_var(STAGE_ENV);
            let fd: c_int = fd.parse().map_err(|_| format!("bad {STAGE_ENV}"))?;
            let block = verify(fd, tls);
            unsafe { sys::close(fd) };
            block
        }
        Err(_) => Err(restart(binary, args, levers, tls)),
    }
}

fn restart(binary: &Path, args: &[OsString], levers: &Levers, tls: &Tls) -> String {
    let me = match std::env::current_exe() {
        Ok(me) => me,
        Err(e) => return format!("locate dee_shim: {e}"),
    };
    let interpreter = match interpreter(&me) {
        Ok(path) => path,
        Err(e) => return e,
    };
    let fd = unsafe { sys::memfd_create(c"dee-shim-tool-tls".as_ptr(), 0) };
    if fd < 0 {
        return format!("memfd_create: {}", std::io::Error::last_os_error());
    }
    // The descriptor stays open (no close-on-exec) so the restarted host can load it.
    let mut file = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(fd) });
    if let Err(e) = file.write_all(&stub(tls)) {
        return format!("write the TLS stub: {e}");
    }
    let mut command = std::process::Command::new(&interpreter);
    command
        .arg("--argv0")
        .arg(&me)
        .arg("--preload")
        .arg(format!("/proc/self/fd/{fd}"))
        .arg(&me)
        .args(args)
        .env(super::super::HOST_ENV, binary)
        .env(STAGE_ENV, fd.to_string());
    // The first stage took the levers out of its environment; the second needs them again.
    super::super::pass_levers(&mut command, levers);
    let error = command.exec();
    format!("restart through {}: {error}", interpreter.display())
}

/// dee_shim's own PT_INTERP.
fn interpreter(me: &Path) -> Result<std::path::PathBuf, String> {
    let data = std::fs::read(me).map_err(|e| format!("{}: {e}", me.display()))?;
    let field = |o: usize, n: usize| -> Option<u64> {
        let mut v = [0u8; 8];
        v[..n].copy_from_slice(data.get(o..o + n)?);
        Some(u64::from_le_bytes(v))
    };
    let phoff = field(32, 8).ok_or("truncated dee_shim")? as usize;
    let phnum = field(56, 2).ok_or("truncated dee_shim")? as usize;
    for i in 0..phnum {
        let o = phoff + i * 56;
        if field(o, 4) == Some(3) {
            let off = field(o + 8, 8).ok_or("truncated dee_shim")? as usize;
            let len = field(o + 32, 8).ok_or("truncated dee_shim")? as usize;
            let name = data.get(off..off + len).ok_or("truncated dee_shim")?;
            let name = name.split(|b| *b == 0).next().unwrap_or_default();
            return Ok(std::path::PathBuf::from(std::ffi::OsStr::from_bytes(name)));
        }
    }
    Err("dee_shim has no PT_INTERP".to_string())
}

fn verify(fd: c_int, tls: &Tls) -> Result<Block, String> {
    struct Found {
        name: String,
        modid: usize,
        data: usize,
        seen: bool,
    }
    unsafe extern "C" fn find(info: *mut sys::DlPhdrInfo, _: usize, data: *mut c_void) -> c_int {
        let found = &mut *(data as *mut Found);
        let info = &*info;
        if !info.name.is_null()
            && std::ffi::CStr::from_ptr(info.name).to_bytes() == found.name.as_bytes()
        {
            found.modid = info.tls_modid;
            found.data = info.tls_data as usize;
            found.seen = true;
            return 1;
        }
        0
    }
    let mut found = Found {
        name: format!("/proc/self/fd/{fd}"),
        modid: 0,
        data: 0,
        seen: false,
    };
    unsafe { sys::dl_iterate_phdr(find, &mut found as *mut Found as *mut c_void) };
    if !found.seen {
        return Err("the TLS stub is not loaded".into());
    }
    let offset = tls.tp_offset();
    let tp = sys::thread_pointer();
    let expected = tp - offset as usize;
    if found.data != expected {
        let marker = unsafe { dee_shim_tls_gap_tpoff() };
        return Err(format!(
            "the tool's TLS block landed at tp-{:#x}, not tp-{offset:#x} (dee_shim's gap marker \
             is at tp{marker:+#x})",
            tp.wrapping_sub(found.data)
        ));
    }
    log::debug!(
        "in-process host: tool TLS at tp-{offset:#x}, module {}",
        found.modid
    );
    Ok(Block {
        modid: found.modid,
        offset,
    })
}

/// A shared object with no code and no symbols, only a PT_TLS that reproduces the tool's TLS
/// template: same size, alignment, `.tdata` bytes, and the same address congruence modulo the
/// alignment, so glibc computes the same offset for it as it did for the tool.
fn stub(tls: &Tls) -> Vec<u8> {
    const PHNUM: u64 = 4;
    const DT_NULL: u64 = 0;
    const DT_STRTAB: u64 = 5;
    const DT_SYMTAB: u64 = 6;
    const DT_STRSZ: u64 = 10;
    const DT_SYMENT: u64 = 11;
    const DT_FLAGS: u64 = 30;
    const DF_STATIC_TLS: u64 = 0x10;
    let align = tls.align.max(1);
    let dynamic = 64 + 56 * PHNUM;
    let strtab = dynamic + 16 * 6;
    let symtab = strtab + 8;
    let end = symtab + 24;
    let tdata = end.div_ceil(align) * align + tls.vaddr % align;
    let size = tdata + tls.init.len() as u64;

    let mut out = Vec::with_capacity(size as usize);
    let put16 = |o: &mut Vec<u8>, v: u16| o.extend_from_slice(&v.to_le_bytes());
    let put32 = |o: &mut Vec<u8>, v: u32| o.extend_from_slice(&v.to_le_bytes());
    let put64 = |o: &mut Vec<u8>, v: u64| o.extend_from_slice(&v.to_le_bytes());
    // ELF header: ET_DYN, x86_64, program headers right after it, no sections.
    out.extend_from_slice(b"\x7fELF\x02\x01\x01\0\0\0\0\0\0\0\0\0");
    put16(&mut out, 3);
    put16(&mut out, 62);
    put32(&mut out, 1);
    put64(&mut out, 0);
    put64(&mut out, 64);
    put64(&mut out, 0);
    put32(&mut out, 0);
    put16(&mut out, 64);
    put16(&mut out, 56);
    put16(&mut out, PHNUM as u16);
    put16(&mut out, 64);
    put16(&mut out, 0);
    put16(&mut out, 0);
    let phdr = |o: &mut Vec<u8>, kind: u32, flags: u32, off: u64, filesz: u64, memsz: u64, al| {
        put32(o, kind);
        put32(o, flags);
        put64(o, off);
        put64(o, off);
        put64(o, off);
        put64(o, filesz);
        put64(o, memsz);
        put64(o, al);
    };
    // The dynamic section is writable: glibc relocates its pointers in place.
    phdr(&mut out, 1, 6, 0, size, size, 0x1000);
    phdr(&mut out, 2, 6, dynamic, 16 * 6, 16 * 6, 8);
    phdr(
        &mut out,
        7,
        4,
        tdata,
        tls.init.len() as u64,
        tls.memsz,
        align,
    );
    phdr(&mut out, 0x6474_e551, 6, 0, 0, 0, 16);
    for (tag, value) in [
        (DT_STRTAB, strtab),
        (DT_SYMTAB, symtab),
        (DT_STRSZ, 1),
        (DT_SYMENT, 24),
        (DT_FLAGS, DF_STATIC_TLS),
        (DT_NULL, 0),
    ] {
        put64(&mut out, tag);
        put64(&mut out, value);
    }
    out.resize(symtab as usize, 0);
    out.resize(end as usize, 0);
    out.resize(tdata as usize, 0);
    out.extend_from_slice(&tls.init);
    out
}
