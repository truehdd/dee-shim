//! Resolving named sites to runtime addresses for the hosted tool image, with a byte check.
//!
//! The constructor builds one `Resolver`: it tries every profile's identity against the loaded
//! images and keeps the one that matches. Modules then ask for a site by name and get its
//! runtime address, but only after the bytes the profile recorded are confirmed to be there, so
//! a stale profile fails loudly instead of corrupting a different instruction.

use crate::platform::{self, Loaded};
use dshim_profiles::{by_ident, hex, Kind, Os, Profile, ALL};
use std::cell::Cell;
use std::io::Write;

/// The OS this engine was built for, to match a profile's `os` in the signature fallback.
#[cfg(target_os = "linux")]
const THIS_OS: Os = Os::Linux;
#[cfg(target_os = "macos")]
const THIS_OS: Os = Os::Macos;
#[cfg(target_os = "windows")]
const THIS_OS: Os = Os::Windows;

pub struct Resolver {
    pub profile: &'static Profile,
    loaded: Loaded,
    failed_site: Cell<bool>,
}

impl Resolver {
    /// Match a loaded image to a profile by identity. Returns None with a logged reason if no
    /// profile's binary is present.
    pub fn new(log: &mut dyn Write) -> Option<Resolver> {
        for p in ALL {
            if let Some(loaded) = platform::find_image(&p.ident.key()) {
                lok!(log, "", "matched {}, slide {:#x}", p.label(), loaded.slide);
                return Some(Resolver {
                    profile: p,
                    loaded,
                    failed_site: Cell::new(false),
                });
            }
        }
        // Signature fallback. No identity matched, but a build recompiled from the same tree can
        // keep a profiled function at its link-time VA with its prologue bytes intact. When every
        // site of a profile is found byte-for-byte at its VA (slide 0, the non-PIE case), adopt
        // it. Each read is bounds-checked against the main executable's readable mappings first,
        // so probing a wrong-version VA never touches unmapped memory. Only Linux reports those
        // mappings; elsewhere `main_ranges` is None and this is skipped.
        if let Some(ranges) = platform::main_ranges() {
            let contains =
                |addr: usize, len: usize| ranges.iter().any(|&(a, b)| addr >= a && addr + len <= b);
            for p in ALL {
                if p.os != THIS_OS || p.sites.is_empty() {
                    continue;
                }
                let all_match = p.sites.iter().all(|s| {
                    let want = hex(s.expect);
                    let addr = s.va as usize; // slide 0: non-PIE link-time VA is live
                    contains(addr, want.len()) && platform::read(addr, want.len()) == want
                });
                if all_match {
                    lok!(
                        log,
                        "",
                        "adopted {} by signature, slide 0x0 (identity did not match)",
                        p.label()
                    );
                    return Some(Resolver {
                        profile: p,
                        loaded: Loaded { slide: 0 },
                        failed_site: Cell::new(false),
                    });
                }
            }
        }
        ldebug!(
            log,
            "",
            "no profile identity or signature matched a loaded image"
        );
        let _ = by_ident(""); // keep the symbol referenced; no-op
        None
    }

    /// Runtime address of a link-time VA under this image's load slide.
    pub fn at(&self, va: u64) -> usize {
        self.loaded.at(va)
    }

    /// A declared site failed its live byte check. The caller can fail closed after all modules
    /// have reported their diagnostics rather than encoding with a partial requested capability.
    pub fn failed_site(&self) -> bool {
        self.failed_site.get()
    }

    /// Runtime address of a named site after confirming its `expect` bytes. Logs and returns
    /// None if the site is unknown or the bytes differ.
    pub fn site(&self, name: &str, log: &mut dyn Write) -> Option<Resolved> {
        let s = self.profile.site(name).or_else(|| {
            ldebug!(
                log,
                name,
                "not in profile {}, skipped",
                self.profile.label()
            );
            None
        })?;
        let addr = self.loaded.at(s.va);
        let want = hex(s.expect);
        let got = platform::read(addr, want.len());
        if got != want {
            self.failed_site.set(true);
            lerr!(
                log,
                name,
                "byte mismatch @ {addr:#x}, expect {} got {}",
                s.expect,
                got.iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            return None;
        }
        let steal = match s.kind {
            Kind::Hook { steal } | Kind::HookCall { steal, .. } => steal,
            _ => 0,
        };
        Some(Resolved {
            addr,
            steal,
            kind: s.kind,
        })
    }
}

#[derive(Clone, Copy)]
pub struct Resolved {
    pub addr: usize,
    pub steal: usize,
    pub kind: Kind,
}

impl Resolved {
    pub fn detour(&self, hook: usize) -> Option<usize> {
        match self.kind {
            Kind::Hook { steal } => crate::detour::install(self.addr, steal, hook),
            Kind::HookCall { steal, call_offset } => {
                crate::detour::install_call(self.addr, steal, call_offset, hook)
            }
            _ => None,
        }
    }
}
