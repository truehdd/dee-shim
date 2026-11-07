//! Map, link and start a non-PIE x86_64 executable (`ET_EXEC`) inside this process.
//!
//! The image is mapped at its link-time addresses (slide 0), so once the segments are in place
//! every table the dynamic section names can be read straight from memory.

use super::hooks;
use super::sys::{self, Phdr};
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_PHDR: u32 = 6;
const PT_TLS: u32 = 7;
const PT_GNU_EH_FRAME: u32 = 0x6474_e550;
const PT_GNU_RELRO: u32 = 0x6474_e552;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;

const DT_NULL: i64 = 0;
const DT_NEEDED: i64 = 1;
const DT_PLTRELSZ: i64 = 2;
const DT_STRTAB: i64 = 5;
const DT_SYMTAB: i64 = 6;
const DT_RELA: i64 = 7;
const DT_RELASZ: i64 = 8;
const DT_INIT: i64 = 12;
const DT_FINI: i64 = 13;
const DT_RPATH: i64 = 15;
const DT_REL: i64 = 17;
const DT_PLTREL: i64 = 20;
const DT_TEXTREL: i64 = 22;
const DT_JMPREL: i64 = 23;
const DT_INIT_ARRAY: i64 = 25;
const DT_FINI_ARRAY: i64 = 26;
const DT_INIT_ARRAYSZ: i64 = 27;
const DT_FINI_ARRAYSZ: i64 = 28;
const DT_RUNPATH: i64 = 29;
const DT_PREINIT_ARRAY: i64 = 32;
const DT_PREINIT_ARRAYSZ: i64 = 33;
const DT_VERSYM: i64 = 0x6fff_fff0;
const DT_VERNEED: i64 = 0x6fff_fffe;
const DT_VERNEEDNUM: i64 = 0x6fff_ffff;

const R_X86_64_NONE: u32 = 0;
const R_X86_64_64: u32 = 1;
const R_X86_64_COPY: u32 = 5;
const R_X86_64_GLOB_DAT: u32 = 6;
const R_X86_64_JUMP_SLOT: u32 = 7;
const R_X86_64_RELATIVE: u32 = 8;
const R_X86_64_TPOFF64: u32 = 18;
const R_X86_64_IRELATIVE: u32 = 37;

const STB_WEAK: u8 = 2;

fn u16_at(d: &[u8], o: usize) -> Result<u16, String> {
    d.get(o..o + 2)
        .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| "truncated ELF".to_string())
}

fn u32_at(d: &[u8], o: usize) -> Result<u32, String> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| "truncated ELF".to_string())
}

fn u64_at(d: &[u8], o: usize) -> Result<u64, String> {
    d.get(o..o + 8)
        .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| "truncated ELF".to_string())
}

/// The image's static TLS template (PT_TLS).
#[derive(Clone)]
pub struct Tls {
    pub vaddr: u64,
    pub memsz: u64,
    pub align: u64,
    /// The initialised part (`.tdata`), `p_filesz` bytes.
    pub init: Vec<u8>,
}

impl Tls {
    /// Distance below the thread pointer at which glibc puts this block when the image is the
    /// main program (module 1, TLS variant II): the offset the image's local-exec code uses.
    pub fn tp_offset(&self) -> u64 {
        let align = self.align.max(1);
        let first = self.vaddr.wrapping_neg() & (align - 1);
        self.memsz.wrapping_sub(first).div_ceil(align) * align + first
    }
}

/// What the program headers say, read from the file before anything is mapped.
pub struct Image {
    pub entry: u64,
    pub phdrs: Vec<Phdr>,
    /// Runtime address of the program header table.
    pub phdr_addr: u64,
    pub tls: Option<Tls>,
}

impl Image {
    pub fn parse(data: &[u8]) -> Result<Image, String> {
        if data.get(..4) != Some(b"\x7fELF") || data.get(4..6) != Some(&[2, 1]) {
            return Err("not a 64-bit little-endian ELF file".into());
        }
        let kind = u16_at(data, 16)?;
        let machine = u16_at(data, 18)?;
        if machine != 62 {
            return Err(format!("ELF machine {machine} is not x86_64"));
        }
        if kind != 2 {
            return Err(format!(
                "ELF type {kind} is not ET_EXEC; this loader maps fixed-address executables"
            ));
        }
        let entry = u64_at(data, 24)?;
        let phoff = u64_at(data, 32)? as usize;
        let phentsize = u16_at(data, 54)? as usize;
        let phnum = u16_at(data, 56)? as usize;
        if phentsize != std::mem::size_of::<Phdr>() {
            return Err(format!("unexpected program header size {phentsize}"));
        }
        let mut phdrs = Vec::with_capacity(phnum);
        for i in 0..phnum {
            let o = phoff + i * phentsize;
            phdrs.push(Phdr {
                p_type: u32_at(data, o)?,
                p_flags: u32_at(data, o + 4)?,
                p_offset: u64_at(data, o + 8)?,
                p_vaddr: u64_at(data, o + 16)?,
                p_paddr: u64_at(data, o + 24)?,
                p_filesz: u64_at(data, o + 32)?,
                p_memsz: u64_at(data, o + 40)?,
                p_align: u64_at(data, o + 48)?,
            });
        }
        let phdr_addr = match phdrs.iter().find(|p| p.p_type == PT_PHDR) {
            Some(p) => p.p_vaddr,
            None => phdrs
                .iter()
                .find(|p| {
                    p.p_type == PT_LOAD
                        && p.p_offset <= phoff as u64
                        && (phoff + phnum * phentsize) as u64 <= p.p_offset + p.p_filesz
                })
                .map(|p| p.p_vaddr + phoff as u64 - p.p_offset)
                .ok_or("program headers are not in a loaded segment")?,
        };
        let tls = match phdrs.iter().find(|p| p.p_type == PT_TLS) {
            Some(p) if p.p_memsz > 0 => {
                let start = p.p_offset as usize;
                let init = data
                    .get(start..start + p.p_filesz as usize)
                    .ok_or("truncated TLS template")?
                    .to_vec();
                Some(Tls {
                    vaddr: p.p_vaddr,
                    memsz: p.p_memsz,
                    align: p.p_align.max(1),
                    init,
                })
            }
            _ => None,
        };
        if let Some(tls) = &tls {
            if !tls.align.is_power_of_two() {
                return Err(format!("TLS alignment {} is not a power of two", tls.align));
            }
        }
        Ok(Image {
            entry,
            phdrs,
            phdr_addr,
            tls,
        })
    }

    fn loads(&self) -> impl Iterator<Item = &Phdr> {
        self.phdrs.iter().filter(|p| p.p_type == PT_LOAD)
    }

    /// The loaded ranges, `[start, end)`, at their runtime (= link-time) addresses.
    pub fn ranges(&self) -> Vec<(usize, usize)> {
        self.loads()
            .map(|p| (p.p_vaddr as usize, (p.p_vaddr + p.p_memsz) as usize))
            .collect()
    }

    /// Lowest mapped address (what `dladdr` reports as the base of a non-PIE executable).
    pub fn base(&self) -> usize {
        self.loads()
            .map(|p| sys::page_down(p.p_vaddr as usize))
            .min()
            .unwrap_or(0)
    }

    /// Map every PT_LOAD at its link address, privately (copy-on-write) from `path`. Segments
    /// stay writable until `protect`, so relocations can be applied.
    pub fn map(&self, path: &Path) -> Result<(), String> {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let fd = file.as_raw_fd();
        let mut mapped_end = 0usize;
        let mut loads: Vec<&Phdr> = self.loads().collect();
        loads.sort_by_key(|p| p.p_vaddr);
        for p in loads {
            let vaddr = p.p_vaddr as usize;
            if p.p_offset % sys::PAGE as u64 != p.p_vaddr % sys::PAGE as u64 {
                return Err(format!("segment at {vaddr:#x} is not page-congruent"));
            }
            let start = sys::page_down(vaddr);
            if start < mapped_end {
                return Err(format!(
                    "segment at {vaddr:#x} shares a page with the previous one"
                ));
            }
            let file_end = vaddr + p.p_filesz as usize;
            let mem_end = sys::page_up(vaddr + p.p_memsz as usize);
            let prot = sys::PROT_READ | sys::PROT_WRITE;
            if p.p_filesz > 0 {
                let len = sys::page_up(file_end) - start;
                let off = sys::page_down(p.p_offset as usize) as i64;
                map_fixed(start, len, prot, sys::MAP_PRIVATE, fd, off)?;
                // The rest of the last file page belongs to .bss.
                let tail = sys::page_up(file_end);
                if p.p_memsz > p.p_filesz && tail > file_end {
                    unsafe { std::ptr::write_bytes(file_end as *mut u8, 0, tail - file_end) };
                }
            }
            let anon = sys::page_up(file_end).max(start);
            if mem_end > anon {
                map_fixed(
                    anon,
                    mem_end - anon,
                    prot,
                    sys::MAP_PRIVATE | sys::MAP_ANONYMOUS,
                    -1,
                    0,
                )?;
            }
            mapped_end = mem_end;
        }
        Ok(())
    }

    /// Give each segment its final protection, then make the RELRO range read-only, as ld.so
    /// does after relocating.
    pub fn protect(&self) -> Result<(), String> {
        for p in self.loads() {
            let start = sys::page_down(p.p_vaddr as usize);
            let end = sys::page_up((p.p_vaddr + p.p_memsz) as usize);
            let mut prot = 0;
            if p.p_flags & PF_R != 0 {
                prot |= sys::PROT_READ;
            }
            if p.p_flags & PF_W != 0 {
                prot |= sys::PROT_WRITE;
            }
            if p.p_flags & PF_X != 0 {
                prot |= sys::PROT_EXEC;
            }
            protect(start, end - start, prot)?;
        }
        if let Some(p) = self.phdrs.iter().find(|p| p.p_type == PT_GNU_RELRO) {
            let start = sys::page_down(p.p_vaddr as usize);
            let end = sys::page_down((p.p_vaddr + p.p_memsz) as usize);
            if end > start {
                protect(start, end - start, sys::PROT_READ)?;
            }
        }
        Ok(())
    }

    /// Hand the image's `.eh_frame` to libgcc. Its unwinder finds FDEs through
    /// `dl_iterate_phdr` (or `_dl_find_object`), which never lists a hand-mapped image, but it
    /// searches registered frames first.
    pub fn register_eh_frame(&self) -> Result<(), String> {
        let Some(hdr) = self.phdrs.iter().find(|p| p.p_type == PT_GNU_EH_FRAME) else {
            return Ok(());
        };
        let hdr = hdr.p_vaddr as usize;
        let eh_frame = unsafe {
            let version = *(hdr as *const u8);
            let enc = *((hdr + 1) as *const u8);
            if version != 1 {
                return Err(format!(".eh_frame_hdr version {version}"));
            }
            let field = hdr + 4;
            read_encoded(field, enc)?
        };
        let register = unsafe { sys::dlsym(std::ptr::null_mut(), c"__register_frame".as_ptr()) };
        if register.is_null() {
            return Err("libgcc's __register_frame is not loaded".into());
        }
        let register: unsafe extern "C" fn(*const c_void) =
            unsafe { std::mem::transmute(register) };
        unsafe { register(eh_frame as *const c_void) };
        Ok(())
    }

    /// `main`, read from `_start`: glibc's `_start` passes it to `__libc_start_main` with
    /// `mov $main, %rdi` (48 c7 c7 imm32).
    pub fn main(&self) -> Result<usize, String> {
        let code = unsafe { std::slice::from_raw_parts(self.entry as *const u8, 64) };
        let at = code
            .windows(3)
            .position(|w| w == [0x48, 0xc7, 0xc7])
            .ok_or("no `mov $main, %rdi` in _start")?;
        let main = i32::from_le_bytes(code[at + 3..at + 7].try_into().unwrap()) as i64 as usize;
        let executable = self.loads().any(|p| {
            p.p_flags & PF_X != 0
                && (p.p_vaddr as usize..(p.p_vaddr + p.p_memsz) as usize).contains(&main)
        });
        if !executable {
            return Err(format!("_start names main at {main:#x}, outside the code"));
        }
        Ok(main)
    }
}

fn map_fixed(
    addr: usize,
    len: usize,
    prot: c_int,
    flags: c_int,
    fd: c_int,
    off: i64,
) -> Result<(), String> {
    let got = unsafe {
        sys::mmap(
            addr as *mut c_void,
            len,
            prot,
            flags | sys::MAP_FIXED_NOREPLACE,
            fd,
            off,
        )
    };
    if got == sys::MAP_FAILED {
        return Err(format!(
            "map {addr:#x}+{len:#x}: {}",
            std::io::Error::last_os_error()
        ));
    }
    if got as usize != addr {
        // A kernel older than 4.17 treats MAP_FIXED_NOREPLACE as a hint.
        unsafe { sys::munmap(got, len) };
        return Err(format!("map {addr:#x}: the address range is taken"));
    }
    Ok(())
}

fn protect(addr: usize, len: usize, prot: c_int) -> Result<(), String> {
    if unsafe { sys::mprotect(addr as *mut c_void, len, prot) } != 0 {
        return Err(format!(
            "mprotect {addr:#x}+{len:#x}: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Read a DWARF exception-header pointer of encoding `enc` stored at `at`.
unsafe fn read_encoded(at: usize, enc: u8) -> Result<usize, String> {
    let value = match enc & 0x0f {
        0x00 | 0x04 | 0x0c => *(at as *const u64) as usize,
        0x03 => *(at as *const u32) as usize,
        0x0b => *(at as *const i32) as isize as usize,
        _ => return Err(format!(".eh_frame_hdr pointer encoding {enc:#x}")),
    };
    match enc & 0x70 {
        0x00 => Ok(value),
        0x10 => Ok(at.wrapping_add(value)),
        _ => Err(format!(".eh_frame_hdr pointer encoding {enc:#x}")),
    }
}

/// The dynamic section, read from the mapped image.
pub struct Dynamic {
    pub needed: Vec<String>,
    /// DT_RPATH and DT_RUNPATH entries, `$ORIGIN` already expanded.
    pub search: Vec<PathBuf>,
    strtab: usize,
    symtab: usize,
    versym: usize,
    verneed: usize,
    verneednum: usize,
    rela: (usize, usize),
    jmprel: (usize, usize),
    init: usize,
    fini: usize,
    preinit_array: (usize, usize),
    init_array: (usize, usize),
    fini_array: (usize, usize),
}

impl Dynamic {
    pub fn read(image: &Image, path: &Path) -> Result<Dynamic, String> {
        let p = image
            .phdrs
            .iter()
            .find(|p| p.p_type == PT_DYNAMIC)
            .ok_or("no dynamic section")?;
        let mut tags: Vec<(i64, u64)> = Vec::new();
        let mut at = p.p_vaddr as usize;
        loop {
            let (tag, value) = unsafe { (*(at as *const i64), *((at + 8) as *const u64)) };
            if tag == DT_NULL {
                break;
            }
            tags.push((tag, value));
            at += 16;
        }
        let one = |t: i64| {
            tags.iter()
                .find(|(tag, _)| *tag == t)
                .map(|(_, v)| *v as usize)
                .unwrap_or(0)
        };
        for unsupported in [DT_REL, DT_TEXTREL] {
            if one(unsupported) != 0 {
                return Err(format!("dynamic tag {unsupported} is not supported"));
            }
        }
        if one(DT_PLTREL) != 0 && one(DT_PLTREL) != DT_RELA as usize {
            return Err("PLT relocations are not RELA".into());
        }
        let strtab = one(DT_STRTAB);
        if strtab == 0 {
            return Err("no DT_STRTAB".into());
        }
        let string = |off: u64| unsafe {
            CStr::from_ptr((strtab + off as usize) as *const c_char)
                .to_string_lossy()
                .into_owned()
        };
        let origin = path.parent().unwrap_or(Path::new("/"));
        let mut search = Vec::new();
        // Like ld.so: DT_RPATH only counts when there is no DT_RUNPATH; both are searched before
        // the system directories, which plain dlopen covers.
        let has_runpath = tags.iter().any(|(t, _)| *t == DT_RUNPATH);
        for (tag, value) in &tags {
            if *tag == DT_RUNPATH || (*tag == DT_RPATH && !has_runpath) {
                for dir in string(*value).split(':').filter(|d| !d.is_empty()) {
                    let dir = dir
                        .replace("${ORIGIN}", &origin.to_string_lossy())
                        .replace("$ORIGIN", &origin.to_string_lossy());
                    search.push(PathBuf::from(dir));
                }
            }
        }
        Ok(Dynamic {
            needed: tags
                .iter()
                .filter(|(t, _)| *t == DT_NEEDED)
                .map(|(_, v)| string(*v))
                .collect(),
            search,
            strtab,
            symtab: one(DT_SYMTAB),
            versym: one(DT_VERSYM),
            verneed: one(DT_VERNEED),
            verneednum: one(DT_VERNEEDNUM),
            rela: (one(DT_RELA), one(DT_RELASZ)),
            jmprel: (one(DT_JMPREL), one(DT_PLTRELSZ)),
            init: one(DT_INIT),
            fini: one(DT_FINI),
            preinit_array: (one(DT_PREINIT_ARRAY), one(DT_PREINIT_ARRAYSZ)),
            init_array: (one(DT_INIT_ARRAY), one(DT_INIT_ARRAYSZ)),
            fini_array: (one(DT_FINI_ARRAY), one(DT_FINI_ARRAYSZ)),
        })
    }

    /// Version names by version index, from DT_VERNEED.
    fn versions(&self) -> HashMap<u16, CString> {
        let mut out = HashMap::new();
        let mut need = self.verneed;
        for _ in 0..self.verneednum {
            if need == 0 {
                break;
            }
            unsafe {
                let cnt = *((need + 2) as *const u16);
                let mut aux = need + *((need + 8) as *const u32) as usize;
                for _ in 0..cnt {
                    let other = *((aux + 6) as *const u16);
                    let name = *((aux + 8) as *const u32) as usize;
                    out.insert(
                        other,
                        CStr::from_ptr((self.strtab + name) as *const c_char).to_owned(),
                    );
                    let next = *((aux + 12) as *const u32) as usize;
                    if next == 0 {
                        break;
                    }
                    aux += next;
                }
                let next = *((need + 12) as *const u32) as usize;
                if next == 0 {
                    break;
                }
                need += next;
            }
        }
        out
    }
}

/// `dlopen` the image's DT_NEEDED libraries in order, globally, as ld.so would put them in the
/// main program's scope. Returns their handles in the same order.
pub fn load_needed(dynamic: &Dynamic) -> Result<Vec<*mut c_void>, String> {
    let mut handles = Vec::new();
    for name in &dynamic.needed {
        let handle = hooks::open_library(name, &dynamic.search, sys::RTLD_NOW | sys::RTLD_GLOBAL);
        if handle.is_null() {
            return Err(format!("load {name}: {}", sys::last_dl_error()));
        }
        handles.push(handle);
    }
    Ok(handles)
}

/// A symbol the image's relocations name.
struct Symbol {
    name: CString,
    version: Option<CString>,
    value: u64,
    size: u64,
    defined: bool,
    weak: bool,
}

/// One COPY relocation: the library's object (`from`) and the image's copy of it (`to`).
pub struct Copy {
    pub from: usize,
    pub to: usize,
}

/// Apply every RELA relocation of the image. Returns its COPY relocations, which the caller
/// must make the libraries use (see `hooks::interpose`).
pub fn relocate(
    dynamic: &Dynamic,
    libs: &[*mut c_void],
    tls_offset: Option<u64>,
) -> Result<Vec<Copy>, String> {
    let versions = dynamic.versions();
    let symbol = |index: u32| -> Symbol {
        let at = dynamic.symtab + index as usize * 24;
        unsafe {
            let name = *(at as *const u32) as usize;
            let info = *((at + 4) as *const u8);
            let shndx = *((at + 6) as *const u16);
            let version = if dynamic.versym != 0 {
                versions
                    .get(&(*((dynamic.versym + index as usize * 2) as *const u16) & 0x7fff))
                    .cloned()
            } else {
                None
            };
            Symbol {
                name: CStr::from_ptr((dynamic.strtab + name) as *const c_char).to_owned(),
                version,
                value: *((at + 8) as *const u64),
                size: *((at + 16) as *const u64),
                defined: shndx != 0,
                weak: info >> 4 == STB_WEAK,
            }
        }
    };
    let mut resolved: HashMap<u32, usize> = HashMap::new();
    let mut copies = Vec::new();
    let mut missing = Vec::new();
    let tp = sys::thread_pointer();
    let tables = [dynamic.rela, dynamic.jmprel];
    for (start, size) in tables {
        for i in 0..size / 24 {
            let at = start + i * 24;
            let (offset, info, addend) = unsafe {
                (
                    *(at as *const u64) as usize,
                    *((at + 8) as *const u64),
                    *((at + 16) as *const i64),
                )
            };
            let kind = info as u32;
            let index = (info >> 32) as u32;
            let slot = offset as *mut u64;
            match kind {
                R_X86_64_NONE => continue,
                R_X86_64_RELATIVE => {
                    unsafe { *slot = addend as u64 };
                    continue;
                }
                R_X86_64_IRELATIVE => {
                    let resolver: unsafe extern "C" fn() -> u64 =
                        unsafe { std::mem::transmute(addend as usize) };
                    unsafe { *slot = resolver() };
                    continue;
                }
                _ => {}
            }
            let sym = symbol(index);
            match kind {
                R_X86_64_COPY => {
                    let from = lookup(libs, &sym);
                    if from == 0 {
                        return Err(format!("COPY source {:?} not found", sym.name));
                    }
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            from as *const u8,
                            offset as *mut u8,
                            sym.size as usize,
                        )
                    };
                    copies.push(Copy { from, to: offset });
                }
                R_X86_64_TPOFF64 => {
                    let value = if sym.defined || index == 0 {
                        // The image's own TLS: its block sits `tls_offset` below the thread
                        // pointer, as the local-exec code assumes.
                        let off = tls_offset.ok_or("TPOFF64 against an image without TLS")?;
                        (sym.value as i64).wrapping_sub(off as i64)
                    } else {
                        let addr = lookup(libs, &sym);
                        if addr == 0 {
                            return Err(format!("TLS symbol {:?} not found", sym.name));
                        }
                        (addr as i64).wrapping_sub(tp as i64)
                    };
                    unsafe { *slot = value.wrapping_add(addend) as u64 };
                }
                R_X86_64_64 | R_X86_64_GLOB_DAT | R_X86_64_JUMP_SLOT => {
                    let value = match resolved.get(&index) {
                        Some(v) => *v,
                        None => {
                            let v = if sym.defined {
                                sym.value as usize
                            } else if let Some(hook) = hooks::replacement(&sym.name) {
                                hook
                            } else {
                                lookup(libs, &sym)
                            };
                            resolved.insert(index, v);
                            v
                        }
                    };
                    if value == 0 && !sym.defined && !sym.weak {
                        if kind != R_X86_64_JUMP_SLOT {
                            return Err(format!(
                                "undefined symbol {:?} ({:?})",
                                sym.name, sym.version
                            ));
                        }
                        missing.push(sym.name.to_string_lossy().into_owned());
                    }
                    unsafe { *slot = (value as i64).wrapping_add(addend) as u64 };
                }
                other => {
                    return Err(format!(
                        "relocation type {other} ({:?}) is not supported",
                        sym.name
                    ))
                }
            }
        }
    }
    if !missing.is_empty() {
        missing.dedup();
        eprintln!(
            "dee_shim: in-process host: unresolved functions (they fail only if called): {}",
            missing.join(" ")
        );
    }
    Ok(copies)
}

/// Find `sym` in the image's libraries, in DT_NEEDED order (each handle's scope includes its
/// own dependencies), honouring the symbol version the image asked for.
fn lookup(libs: &[*mut c_void], sym: &Symbol) -> usize {
    for &handle in libs {
        let found = unsafe {
            match &sym.version {
                Some(version) => sys::dlvsym(handle, sym.name.as_ptr(), version.as_ptr()),
                None => sys::dlsym(handle, sym.name.as_ptr()),
            }
        };
        if !found.is_null() {
            return found as usize;
        }
    }
    0
}

type InitFn = unsafe extern "C" fn(c_int, *const *const c_char, *const *const c_char);

static FINI: std::sync::OnceLock<(Vec<usize>, usize)> = std::sync::OnceLock::new();

extern "C" fn run_fini() {
    if let Some((array, fini)) = FINI.get() {
        for &f in array.iter().rev() {
            if f != 0 && f != usize::MAX {
                let f: extern "C" fn() = unsafe { std::mem::transmute(f) };
                f();
            }
        }
        if *fini != 0 {
            let f: extern "C" fn() = unsafe { std::mem::transmute(*fini) };
            f();
        }
    }
}

fn array(table: (usize, usize)) -> Vec<usize> {
    let (start, size) = table;
    (0..size / 8)
        .map(|i| unsafe { *((start + i * 8) as *const usize) })
        .collect()
}

/// Run the image's constructors the way glibc runs a main program's: DT_PREINIT_ARRAY, DT_INIT,
/// DT_INIT_ARRAY, each array entry with (argc, argv, envp). Its destructors (DT_FINI_ARRAY in
/// reverse, then DT_FINI) are registered first, so they run after the static destructors the
/// constructors register, and before the libraries' own, as under ld.so.
///
/// # Safety
/// The image must be mapped, relocated and protected.
pub unsafe fn run_init(
    dynamic: &Dynamic,
    argc: c_int,
    argv: *const *const c_char,
    envp: *const *const c_char,
) {
    let _ = FINI.set((array(dynamic.fini_array), dynamic.fini));
    sys::atexit(run_fini);
    let call = |f: usize| {
        if f != 0 && f != usize::MAX {
            let f: InitFn = std::mem::transmute(f);
            f(argc, argv, envp);
        }
    };
    for f in array(dynamic.preinit_array) {
        call(f);
    }
    if dynamic.init != 0 {
        let f: extern "C" fn() = std::mem::transmute(dynamic.init);
        f();
    }
    for f in array(dynamic.init_array) {
        call(f);
    }
}
