//! Confirm that a requested optional shim has a matching site profile before launching DEE.

use dshim_profiles::{hex, Os, Profile, ALL};
use std::path::Path;

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_hexdigit())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn file_bytes_at<'a>(
    data: &'a [u8],
    layout: &dshim_image::ImageLayout,
    va: u64,
    len: usize,
) -> Option<&'a [u8]> {
    let end = va.checked_add(len as u64)?;
    let range = layout
        .ranges
        .iter()
        .find(|r| va >= r.va && end <= r.va.checked_add(r.size).unwrap_or(0))?;
    let start = usize::try_from(range.off.checked_add(va - range.va)?).ok()?;
    data.get(start..start.checked_add(len)?)
}

/// Identity is preferred. Linux also accepts the same byte-for-byte non-PIE signature fallback
/// as the engine's resolver, so a rebuilt binary with unchanged site positions remains usable.
pub fn optional_profile(binary: &Path, tool: &str) -> Result<&'static Profile, String> {
    let data = std::fs::read(binary).map_err(|e| format!("{}: {e}", binary.display()))?;
    let layout =
        dshim_image::image_layout(&data).map_err(|e| format!("{}: {e:#}", binary.display()))?;
    let os = match layout.target.format {
        object::BinaryFormat::Elf => Os::Linux,
        object::BinaryFormat::MachO => Os::Macos,
        object::BinaryFormat::Pe => Os::Windows,
        other => return Err(format!("unsupported executable format {other:?}")),
    };
    if let Some(profile) = ALL
        .iter()
        .copied()
        .find(|p| p.os == os && p.tool == tool && p.ident.key() == norm(&layout.ident))
    {
        return Ok(profile);
    }
    if os == Os::Linux {
        let candidates: Vec<_> = ALL
            .iter()
            .copied()
            .filter(|p| p.os == os && p.tool == tool && !p.sites.is_empty())
            .filter(|p| {
                p.sites.iter().all(|s| {
                    file_bytes_at(&data, &layout, s.va, hex(s.expect).len())
                        == Some(hex(s.expect).as_slice())
                })
            })
            .collect();
        if candidates.len() == 1 {
            return Ok(candidates[0]);
        }
        if candidates.len() > 1 {
            return Err("multiple optional shim profiles match by site signature".into());
        }
    }
    Err(format!(
        "no optional shim profile for {tool} ({os:?}, identity {})",
        layout.ident
    ))
}
