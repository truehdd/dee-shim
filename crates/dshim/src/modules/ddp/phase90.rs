//! The 90 degree surround phase shift of the DD+ core, for dee_ddp_encoder: the plain-ddp twin of
//! the ddpjoc DDPJOC_PHASE90 feature. DEE always applies it; this turns it off.
//!
//! Lever:
//!   DDP_PHASE90=off   do not apply the 90 degree phase shift to the coded 5.1's surrounds
//!                     (on is the DEE default).
//!
//! Linux: the phase-90 flag lives at substream 0's config +0x7C, the same field the ddpjoc core
//! uses. The per-frame engine driver writes it right before it calls the engine
//! (ddp.core.engine_check) and the coded surrounds follow that write, so zeroing the value
//! register leaves the surrounds unshifted.
//!
//! macOS and Windows: engine config +0x7C has no effect (0 or 1 gives a byte-identical .ec3). The
//! live consumer is the preprocessor's phase-90 filter (three Hilbert passes over the surround
//! buffer), called from the preprocessing step behind two gates that read preproc-state +0x50
//! with opposite sense, so exactly one call site fires per encode. Two patch sites disable both
//! calls for every config: ddp.core.phase90_param_write (gate 1, forced to skip; jz to jmp on
//! Windows) and ddp.core.phase90_param_write2 (gate 2, forced to fall through; jnz to jmp on
//! Windows).
//!
//! Verified by encode on Linux, and by decode on macOS and Windows: fronts, C and LFE unchanged
//! (diff rms about 5e-5), both surrounds fully changed (diff rms about 0.17), the phase-90
//! signature.

use crate::site::Resolver;
use crate::write_code;
use dshim_profiles::Kind;
use std::io::Write;

/// Apply DDP_PHASE90=off if set. All or nothing: a missing site ends the process rather than
/// encoding a stream that is not what was asked.
pub fn install(res: &Resolver, log: &mut dyn Write) {
    if crate::levers().ddp.phase90 != Some(false) {
        return; // on is the DEE default; nothing to do
    }
    // Required: the engine-register site on Linux, preproc gate 1 on macOS and Windows.
    let Some(r) = res.site("ddp.core.phase90_param_write", log) else {
        lerr!(log, "phase90", "DDP_PHASE90=off is not available on this build (no ddp.core.phase90_param_write site in its profile)");
        let _ = log.flush();
        std::process::exit(3);
    };
    patch_or_die("ddp.core.phase90_param_write", r, log);

    // Optional: preproc gate 2 on macOS and Windows. Linux has a single site and lacks it, which
    // `res.site` logs as "not in profile, skipped".
    if let Some(r2) = res.site("ddp.core.phase90_param_write2", log) {
        patch_or_die("ddp.core.phase90_param_write2", r2, log);
    }
}

/// Apply one phase-90 patch site, or end the process.
fn patch_or_die(name: &str, r: crate::site::Resolved, log: &mut dyn Write) {
    let Kind::Patch { new } = r.kind else {
        lerr!(log, "phase90", "{name} is not a patch site");
        let _ = log.flush();
        std::process::exit(3);
    };
    let bytes = dshim_profiles::hex(new);
    if write_code(r.addr, &bytes) {
        lok!(log, "phase90", "off: patch {name} @ {:#x} ({new})", r.addr);
    } else {
        lerr!(log, "phase90", "{name} patch write failed, stopping");
        let _ = log.flush();
        std::process::exit(3);
    }
}
