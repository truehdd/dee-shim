//! Site profile types: a profile records where each named hook point, byte patch and data
//! table sits in one specific DEE build, and the bytes that must be there before anything is
//! touched.
//!
//! A profile describes exactly one binary, identified by the identity recorded in the image
//! (GNU build-id on Linux ELF, LC_UUID on macOS Mach-O, the CodeView record or module id on
//! Windows PE). Modules never use an address directly; they ask for a site by name, so
//! supporting a new build or OS means adding one profile file, not editing modules.
//!
//! Addresses are link-time virtual addresses as a disassembler shows them for that file. The
//! runtime address is `va + slide`, where the slide is 0 for a non-PIE ELF and the ASLR slide
//! for Mach-O and PE.

/// How the loader identifies a binary.
#[derive(Clone, Copy, Debug)]
pub enum Ident {
    /// ELF NT_GNU_BUILD_ID note, lowercase hex.
    GnuBuildId(&'static str),
    /// Mach-O LC_UUID, uppercase hex with dashes as `otool -l` prints it.
    MachoUuid(&'static str),
    /// PE CodeView RSDS GUID and age, as `GUID-age`, for a Windows build that ships a PDB
    /// signature.
    PeCodeView(&'static str),
    /// PE module id: `TimeDateStamp-SizeOfImage`, each 8 hex digits (the pair Windows debuggers
    /// and symbol servers key a module on). Used where the PE carries no CodeView record.
    PeModuleId(&'static str),
}

impl Ident {
    /// Normalised form for comparison: hex digits only, lowercase.
    pub fn key(&self) -> String {
        let s = match self {
            Ident::GnuBuildId(s)
            | Ident::MachoUuid(s)
            | Ident::PeCodeView(s)
            | Ident::PeModuleId(s) => s,
        };
        s.chars()
            .filter(|c| c.is_ascii_hexdigit())
            .map(|c| c.to_ascii_lowercase())
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Linux,
    Macos,
    Windows,
}

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    /// Function entry to detour. `steal` bytes are moved into a trampoline, so they must end
    /// on an instruction boundary, be position independent (no RIP-relative operand, no
    /// branch) and cover at least the 12-byte entry jump. `expect` is exactly those bytes.
    Hook { steal: usize },
    /// Entry containing one rel32 call; relocate that instruction in the trampoline.
    HookCall { steal: usize, call_offset: usize },
    /// Fixed byte replacement: `expect` is the original, `new` the replacement (same length,
    /// space-separated hex). Modules may also write their own bytes of that length.
    Patch { new: &'static str },
    /// A table or other data the modules read or edit. `expect` is a prefix that pins it.
    Data,
}

#[derive(Clone, Copy, Debug)]
pub struct Site {
    /// Stable semantic name, `tool.component.thing`, the same in every profile that has it.
    pub name: &'static str,
    pub va: u64,
    pub kind: Kind,
    /// Space-separated hex of the bytes that must be at `va`.
    pub expect: &'static str,
    /// What the site is and what the lever does, in plain words.
    pub note: &'static str,
}

#[derive(Debug)]
pub struct Profile {
    /// Executable name as shipped.
    pub tool: &'static str,
    pub os: Os,
    pub version: &'static str,
    pub ident: Ident,
    pub sites: &'static [Site],
}

impl Profile {
    pub fn site(&self, name: &str) -> Option<&'static Site> {
        self.sites.iter().find(|s| s.name == name)
    }

    pub fn label(&self) -> String {
        format!("{} {:?} {}", self.tool, self.os, self.version)
    }
}

/// Parse space-separated hex ("48 8b 06 c3").
pub fn hex(s: &str) -> Vec<u8> {
    s.split_whitespace()
        .map(|b| {
            u8::from_str_radix(b, 16).unwrap_or_else(|_| panic!("bad hex byte {b:?} in profile"))
        })
        .collect()
}

/// Little-endian i32 bytes, for modules that write an immediate.
pub fn le32(v: i32) -> [u8; 4] {
    v.to_le_bytes()
}
