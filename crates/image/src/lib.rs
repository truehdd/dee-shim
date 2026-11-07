//! Executable format and section address mapping.

use anyhow::{Context, Result};
use object::{Architecture, BinaryFormat, Object, ObjectSegment};

/// Executable format, architecture, and byte order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinaryTarget {
    pub format: BinaryFormat,
    pub architecture: Architecture,
    pub little_endian: bool,
}

/// A checked file-backed load range, used for offline site inspection.
#[derive(Clone, Copy, Debug)]
pub struct MappedRange {
    pub va: u64,
    pub off: u64,
    pub size: u64,
}

pub struct ImageLayout {
    pub target: BinaryTarget,
    pub ident: String,
    pub ranges: Vec<MappedRange>,
}

/// Parse the image identity and load ranges once with the object reader. Bounds are checked
/// before a range is returned, so callers can safely inspect arbitrary candidate files.
pub fn image_layout(data: &[u8]) -> Result<ImageLayout> {
    let file = object::File::parse(data).context("parsing executable")?;
    let target = BinaryTarget {
        format: file.format(),
        architecture: file.architecture(),
        little_endian: file.is_little_endian(),
    };
    let ident = match file.format() {
        BinaryFormat::Elf => file.build_id()?.map(hex).unwrap_or_default(),
        BinaryFormat::MachO => file.mach_uuid()?.map(|uuid| hex(&uuid)).unwrap_or_default(),
        BinaryFormat::Pe => {
            let pe_off = u32::from_le_bytes(
                data.get(0x3c..0x40)
                    .context("truncated PE DOS header")?
                    .try_into()?,
            ) as usize;
            let coff = pe_off.checked_add(4).context("PE offset overflow")?;
            let timestamp_off = coff.checked_add(4).context("PE offset overflow")?;
            let timestamp = u32::from_le_bytes(
                data.get(timestamp_off..timestamp_off + 4)
                    .context("truncated PE COFF header")?
                    .try_into()?,
            );
            let optional = coff.checked_add(20).context("PE offset overflow")?;
            let size_off = optional.checked_add(56).context("PE offset overflow")?;
            let image_size = u32::from_le_bytes(
                data.get(size_off..size_off + 4)
                    .context("truncated PE optional header")?
                    .try_into()?,
            );
            format!("{timestamp:08X}-{image_size:08X}")
        }
        _ => String::new(),
    };
    let mut ranges = Vec::new();
    for segment in file.segments() {
        let (off, size) = segment.file_range();
        if size == 0 {
            continue;
        }
        let end = off
            .checked_add(size)
            .context("segment file range overflow")?;
        if end > data.len() as u64 {
            anyhow::bail!("segment file range exceeds image");
        }
        ranges.push(MappedRange {
            va: segment.address(),
            off,
            size,
        });
    }
    Ok(ImageLayout {
        target,
        ident,
        ranges,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
