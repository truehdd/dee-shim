//! Audio switches for dee_dthd_encoder. By default dthd's TrueHD is lossless only on what DEE
//! hands the TrueHD core, which is not the master's PCM. With noise shaping and apodising off and
//! the other two on (and the cluster pass-through, passthrough.rs), a master whose objects fit
//! decodes sample for sample.
//!
//! Properties DEE chooses per job, set either way (unset keeps DEE's choice):
//!   DTHD_NOISE_SHAPING=on|off
//!                     the TrueHD core requantises every channel to 20 bits with a shaped dither
//!                     (rms about 8 LSB of 24). DEE asks for it on Atmos, not on channel input.
//!   DTHD_APODISE=on|off
//!                     the apodising filter, which changes the coded samples. DEE asks for it on
//!                     96 kHz input only, and ignores it at 44.1 and 48 kHz with a warning, so on
//!                     takes effect from 88.2 kHz up. The Atmos presentation's writer passes a
//!                     constant false and is left alone (Atmos is 48 kHz).
//!
//! Levers (on or off, default off; on is the as-is direction):
//!   DTHD_AUDIO_UNIT_SCALE=on
//!                     hand the core the PCM at the scale it was read at; by default it is
//!                     written back at 8388607 / 8388608, so samples at or above 0.75 of full
//!                     scale lose 1 LSB.
//!   DTHD_AUDIO_EXACT_OVERLAP=on
//!                     a cluster that carries one object alone at gain 1 gets that object's own
//!                     samples where the clusterer's overlapping blocks meet; by default the
//!                     windowed halves are summed in float, so samples at or above a quarter of
//!                     full scale can be 1 LSB off (overlap.rs).
//!   DTHD_AUDIO_AS_IS=on
//!                     both levers on, noise shaping and apodising off. A switch set on its own
//!                     wins.

use crate::modules::common::require_all;
use crate::site::Resolver;
use crate::write_code;
use dshim_profiles::{hex, Kind};
use std::io::Write;

/// The setz that computes each property's flag in DEE's presentation writers. The sites are
/// named `<property>_off_N` and `<property>_on_N`.
const NOISE_SHAPING: (&str, usize) = ("dthd.mll.noise_shaping", 4);
const APODISE: (&str, usize) = ("dthd.mll.apodise", 3);

// Unit scale: the required core and the per-build extras. The macOS build carries a second scale
// load (scale_1); the Windows build loads the scale once and stores the clip bound separately
// (clip_store). The extras are patched when present and skipped otherwise.
const UNIT_SCALE_REQ: &[&str] = &["dthd.pcm.scale_0", "dthd.pcm.clip_max"];
const UNIT_SCALE_OPT: &[&str] = &["dthd.pcm.clip_store", "dthd.pcm.scale_1"];

fn write_patch(name: &str, r: &crate::site::Resolved, feature: &str, log: &mut dyn Write) {
    let Kind::Patch { new } = r.kind else { return };
    if !write_code(r.addr, &hex(new)) {
        lerr!(log, "dthd", "{name} write failed, stopping");
        let _ = log.flush();
        std::process::exit(3);
    }
    lok!(
        log,
        "dthd",
        "{feature}: patch {name} @ {:#x} ({new})",
        r.addr
    );
}

fn apply(res: &Resolver, feature: &str, names: &[&str], log: &mut dyn Write) {
    // Resolve and byte-check every site of the feature before writing any.
    let sites = require_all(res, feature, names, log);
    for (name, r) in names.iter().zip(&sites) {
        write_patch(name, r, feature, log);
    }
}

/// Apply a feature whose required sites must all resolve, plus optional sites patched only when
/// the build carries them (e.g. per-channel loads a compiler collapsed into one). Nothing is
/// written unless the required set resolves first.
fn apply_opt(res: &Resolver, feature: &str, req: &[&str], opt: &[&str], log: &mut dyn Write) {
    let sites = require_all(res, feature, req, log);
    for (name, r) in req.iter().zip(&sites) {
        write_patch(name, r, feature, log);
    }
    for name in opt {
        if let Some(r) = res.site(name, log) {
            write_patch(name, &r, feature, log);
        }
    }
}

/// `keep_quiet`: the pass-through's keep quiet is on, which reads the element blocks (overlap.rs).
pub fn install(res: &Resolver, keep_quiet: bool, log: &mut dyn Write) {
    let dthd = &crate::levers().dthd;
    for (feature, (base, count), value) in [
        ("noise shaping", NOISE_SHAPING, dthd.noise_shaping()),
        ("apodise", APODISE, dthd.apodise()),
    ] {
        let Some(on) = value else { continue };
        let side = if on { "on" } else { "off" };
        let names: Vec<String> = (0..count).map(|n| format!("{base}_{side}_{n}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        apply(res, &format!("{feature} {side}"), &names, log);
    }
    if dthd.audio_unit_scale() {
        apply_opt(res, "unit scale", UNIT_SCALE_REQ, UNIT_SCALE_OPT, log);
    }
    super::overlap::install(res, keep_quiet, log);
}
