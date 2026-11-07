//! The Win32 calls the Windows host uses, and small readers for the mapped PE image.

use std::ffi::c_void;

pub type Handle = *mut c_void;

pub const PAGE_READWRITE: u32 = 0x04;
pub const PAGE_EXECUTE_READWRITE: u32 = 0x40;
pub const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
pub const HEAP_ZERO_MEMORY: u32 = 0x08;
pub const STACK_SIZE_PARAM_IS_A_RESERVATION: u32 = 0x0001_0000;
pub const INFINITE: u32 = 0xFFFF_FFFF;
pub const DLL_PROCESS_DETACH: u32 = 0;
pub const DLL_PROCESS_ATTACH: u32 = 1;
pub const DLL_THREAD_ATTACH: u32 = 2;
pub const DLL_THREAD_DETACH: u32 = 3;
pub const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
pub const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS: u32 = 0x4;
pub const CP_ACP: u32 = 0;

#[allow(non_snake_case)]
#[link(name = "kernel32")]
extern "system" {
    pub fn LoadLibraryExW(name: *const u16, file: Handle, flags: u32) -> Handle;
    pub fn LoadLibraryW(name: *const u16) -> Handle;
    pub fn GetProcAddress(module: Handle, name: *const u8) -> *mut c_void;
    pub fn GetModuleHandleW(name: *const u16) -> Handle;
    pub fn GetLastError() -> u32;
    pub fn SetLastError(code: u32);
    pub fn VirtualProtect(addr: *mut c_void, size: usize, new: u32, old: *mut u32) -> i32;
    pub fn CreateThread(
        attrs: *mut c_void,
        stack: usize,
        start: unsafe extern "system" fn(*mut c_void) -> u32,
        param: *mut c_void,
        flags: u32,
        tid: *mut u32,
    ) -> Handle;
    pub fn WaitForSingleObject(h: Handle, ms: u32) -> u32;
    pub fn GetExitCodeThread(h: Handle, code: *mut u32) -> i32;
    pub fn ExitProcess(code: u32) -> !;
    pub fn GetProcessHeap() -> Handle;
    pub fn HeapAlloc(heap: Handle, flags: u32, size: usize) -> *mut c_void;
    pub fn HeapFree(heap: Handle, flags: u32, mem: *mut c_void) -> i32;
    pub fn GetCurrentProcess() -> Handle;
    pub fn FlushInstructionCache(process: Handle, addr: *const c_void, size: usize) -> i32;
    pub fn TlsAlloc() -> u32;
    pub fn TlsGetValue(index: u32) -> *mut c_void;
    pub fn TlsSetValue(index: u32, value: *mut c_void) -> i32;
    pub fn WideCharToMultiByte(
        page: u32,
        flags: u32,
        wide: *const u16,
        wide_len: i32,
        out: *mut u8,
        out_len: i32,
        default: *const u8,
        used_default: *mut i32,
    ) -> i32;
    pub fn RtlLookupFunctionEntry(pc: u64, base: *mut u64, history: *mut c_void) -> *mut c_void;
    pub fn RtlAddFunctionTable(table: *mut c_void, count: u32, base: u64) -> u8;
}

/// Read a little-endian u16/u32/u64 at an absolute address of the mapped image.
pub unsafe fn rd16(addr: usize) -> u16 {
    (addr as *const u16).read_unaligned()
}
pub unsafe fn rd32(addr: usize) -> u32 {
    (addr as *const u32).read_unaligned()
}
pub unsafe fn rd64(addr: usize) -> u64 {
    (addr as *const u64).read_unaligned()
}

/// Data directory indices used here (IMAGE_DIRECTORY_ENTRY_*).
pub const DIR_IMPORT: usize = 1;
pub const DIR_EXCEPTION: usize = 3;
pub const DIR_TLS: usize = 9;

/// The parts of a mapped PE32+ image's headers the host reads.
pub struct Headers {
    pub base: usize,
    pub entry: usize,
    pub stack_reserve: usize,
    /// The PE (NT) header and the optional header, for the directories and sections.
    nt: usize,
    opt: usize,
}

/// A section of the mapped image: runtime address, virtual size, characteristics.
pub struct Section {
    pub start: usize,
    pub size: usize,
    pub characteristics: u32,
}

impl Headers {
    /// Headers of the image mapped at `base`, or an error if it is not a PE32+ image.
    pub unsafe fn read(base: usize) -> Result<Headers, String> {
        if rd16(base) != 0x5A4D {
            return Err("no MZ header".into());
        }
        let nt = base + rd32(base + 0x3C) as usize;
        if rd32(nt) != 0x4550 {
            return Err("no PE header".into());
        }
        let opt = nt + 24;
        if rd16(opt) != 0x20B {
            return Err("not a PE32+ image".into());
        }
        Ok(Headers {
            base,
            entry: base + rd32(opt + 16) as usize,
            stack_reserve: rd64(opt + 72) as usize,
            nt,
            opt,
        })
    }

    /// The section table (IMAGE_SECTION_HEADER, 40 bytes each, after the optional header).
    pub unsafe fn sections(&self) -> Vec<Section> {
        let count = rd16(self.nt + 6) as usize;
        let table = self.opt + rd16(self.nt + 20) as usize;
        (0..count)
            .map(|i| {
                let header = table + i * 40;
                Section {
                    start: self.base + rd32(header + 12) as usize,
                    size: rd32(header + 8) as usize,
                    characteristics: rd32(header + 36),
                }
            })
            .collect()
    }

    /// Runtime address and size of data directory `index`, or None if it is empty.
    pub unsafe fn dir(&self, index: usize) -> Option<(usize, usize)> {
        let count = rd32(self.opt + 108) as usize;
        if index >= count {
            return None;
        }
        let at = self.opt + 112 + index * 8;
        let (rva, size) = (rd32(at) as usize, rd32(at + 4) as usize);
        (rva != 0).then_some((self.base + rva, size))
    }
}

/// The link-time ImageBase of the PE32+ file at `path`, read from disk because the loader may
/// rewrite the mapped header's copy.
pub fn file_image_base(path: &std::path::Path) -> Result<u64, String> {
    use std::io::Read;
    let mut header = vec![0u8; 0x1000];
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let got = file.read(&mut header).map_err(|e| e.to_string())?;
    header.truncate(got);
    let word = |at: usize, n: usize| -> Option<u64> {
        let bytes = header.get(at..at + n)?;
        Some(bytes.iter().rev().fold(0u64, |v, &b| (v << 8) | b as u64))
    };
    let nt = word(0x3C, 4).ok_or("short header")? as usize;
    if word(nt, 4) != Some(0x4550) || word(nt + 24, 2) != Some(0x20B) {
        return Err("not a PE32+ file".into());
    }
    word(nt + 24 + 24, 8).ok_or_else(|| "short header".into())
}
