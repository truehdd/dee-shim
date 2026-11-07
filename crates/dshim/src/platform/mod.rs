//! Platform layer: the three things that differ by OS. Everything above this (the detour
//! engine, the modules) is OS-independent.
//!
//!   1. make a code page writable (and keep the edit private to this process),
//!   2. flush the instruction cache after writing code,
//!   3. allocate executable memory for trampolines.
//!
//! The real work of each is in `linux`, `macos`, `windows`; this module re-exports the one
//! for the target OS. The tool image itself is mapped by the in-process host, which records it
//! here with `set_hosted`.

#[cfg(target_os = "linux")]
pub(crate) mod linux;
#[cfg(target_os = "linux")]
pub(crate) use linux as os;

#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos as os;

#[cfg(target_os = "windows")]
pub(crate) mod windows;
#[cfg(target_os = "windows")]
pub(crate) use windows as os;

/// A loaded image and how its link-time VAs map to runtime.
pub struct Loaded {
    /// Runtime address minus link-time address; 0 for a non-PIE ELF.
    pub slide: i64,
}

impl Loaded {
    /// Runtime address of a link-time VA from the profile.
    pub fn at(&self, va: u64) -> usize {
        (va as i64 + self.slide) as usize
    }
}

/// The tool image the in-process host mapped: its identity, read from the file, and the slide
/// the host chose.
pub struct Hosted {
    /// Normalised identity: lowercase hex digits only, as `Ident::key` produces.
    key: String,
    slide: i64,
    /// File-backed load ranges at their runtime addresses, `[start, end)`.
    ranges: Vec<(usize, usize)>,
}

impl Hosted {
    pub fn new(image: &std::path::Path, slide: i64) -> Result<Hosted, String> {
        let data = std::fs::read(image).map_err(|e| e.to_string())?;
        let layout = dshim_image::image_layout(&data).map_err(|e| e.to_string())?;
        let key = layout
            .ident
            .chars()
            .filter(|c| c.is_ascii_hexdigit())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        let ranges = layout
            .ranges
            .iter()
            .map(|r| {
                let start = (r.va as i64 + slide) as usize;
                (start, start + r.size as usize)
            })
            .collect();
        Ok(Hosted { key, slide, ranges })
    }
}

static HOSTED: std::sync::OnceLock<Hosted> = std::sync::OnceLock::new();

/// Record the hosted image. Called once by `init_hosted`, before the resolver runs.
pub fn set_hosted(hosted: Hosted) {
    let _ = HOSTED.set(hosted);
}

/// The hosted image's slide if its identity (build-id / UUID / CodeView) normalises to
/// `ident_key`. `None` if no image is hosted or it is a different build or tool.
pub fn find_image(ident_key: &str) -> Option<Loaded> {
    let hosted = HOSTED.get()?;
    (hosted.key == ident_key).then_some(Loaded {
        slide: hosted.slide,
    })
}

/// Overwrite `bytes.len()` bytes of executable code at runtime address `addr`. Makes the page
/// writable first and keeps the change private to this process (copy-on-write), then flushes
/// the instruction cache. Returns false if the page could not be made writable.
pub fn write_code(addr: usize, bytes: &[u8]) -> bool {
    if !os::make_writable(addr, bytes.len()) {
        return false;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), addr as *mut u8, bytes.len());
    }
    os::flush_icache(addr, bytes.len());
    true
}

/// Allocate `len` bytes of RWX memory for a trampoline. Never freed (shims live for the
/// process lifetime).
pub fn alloc_rwx(len: usize) -> *mut u8 {
    os::alloc_rwx(len)
}

/// Flush the instruction cache for freshly written code.
pub fn flush_icache(addr: usize, len: usize) {
    os::flush_icache(addr, len)
}

/// The file-backed mappings of the hosted image, for the signature fallback. `None` disables it.
pub fn main_ranges() -> Option<Vec<(usize, usize)>> {
    HOSTED.get().map(|hosted| hosted.ranges.clone())
}

/// Read `len` bytes of the live image at `addr` (e.g. to confirm a profile's `expect`).
pub fn read(addr: usize, len: usize) -> Vec<u8> {
    unsafe { std::slice::from_raw_parts(addr as *const u8, len).to_vec() }
}
