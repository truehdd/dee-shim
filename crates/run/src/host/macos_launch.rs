//! How the macOS parent starts the host child (see mod.rs).
//!
//! The host child must run x86_64 code: the DEE tools and the engine's detours are x86_64. What
//! that takes depends on the slices of dee_shim itself:
//!
//! - thin x86_64: start it directly; on Apple silicon it runs under Rosetta.
//! - universal with an x86_64 slice: the kernel would pick arm64 on Apple silicon, so start it
//!   through `/usr/bin/arch -x86_64`, which sets the binary preference for the exec
//!   (`posix_spawnattr_setbinpref_np`). `Command` cannot carry a spawn attribute itself, and the
//!   environment and arguments pass through `arch` unchanged.
//! - no x86_64 slice: refuse with a clear message.

use std::io::Read;
use std::path::Path;
use std::process::Command;

const MH_MAGIC_64: u32 = 0xfeed_facf;
const FAT_MAGIC: u32 = 0xcafe_babe;
const FAT_MAGIC_64: u32 = 0xcafe_babf;
const CPU_TYPE_X86_64: u32 = 0x0100_0007;

/// The CPU types of the slices in a Mach-O file, and whether it is universal.
fn slices(me: &Path) -> Result<(bool, Vec<u32>), String> {
    let mut data = Vec::new();
    std::fs::File::open(me)
        .and_then(|f| f.take(4096).read_to_end(&mut data))
        .map_err(|e| format!("read {}: {e}", me.display()))?;
    let be = |at: usize| -> Option<u32> {
        Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
    };
    let le = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
    };
    let malformed = || format!("{}: malformed Mach-O header", me.display());
    if le(0) == Some(MH_MAGIC_64) {
        return Ok((false, vec![le(4).ok_or_else(malformed)?]));
    }
    let magic = be(0).ok_or_else(malformed)?;
    let stride = match magic {
        FAT_MAGIC => 20,
        FAT_MAGIC_64 => 32,
        _ => return Err(format!("{}: not a 64-bit Mach-O executable", me.display())),
    };
    let count = be(4).ok_or_else(malformed)? as usize;
    let cpus = (0..count)
        .map(|i| be(8 + i * stride).ok_or_else(malformed))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((true, cpus))
}

/// The command that starts `me` as the host child, on its x86_64 slice.
pub(super) fn command(me: &Path) -> Result<Command, String> {
    let (universal, cpus) = slices(me)?;
    if !cpus.contains(&CPU_TYPE_X86_64) {
        return Err(format!(
            "{} has no x86_64 slice; the in-process host needs the x86_64 or universal build \
             of dee_shim",
            me.display()
        ));
    }
    if !universal {
        return Ok(Command::new(me));
    }
    let mut command = Command::new("/usr/bin/arch");
    command.arg("-x86_64").arg(me);
    Ok(command)
}

#[cfg(test)]
mod tests {
    #[test]
    fn this_build_has_its_own_slice() {
        let me = std::env::current_exe().unwrap();
        let (_, cpus) = super::slices(&me).unwrap();
        assert!(!cpus.is_empty());
    }
}
