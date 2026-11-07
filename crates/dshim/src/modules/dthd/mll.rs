//! .mll job substitution for dee_dthd_encoder: make the engine read a supplied options (.mll)
//! file instead of the one the driver builds from the CLI --presentation arguments. The CLI and
//! the .mll expose disjoint parameter sets, so this reaches job constructs the CLI cannot express.
//!
//! Lever:
//!   DTHD_MLL=<path>    read this .mll as the engine job.
//!
//! Hook the encoder's options file reader and replace the CLI-generated job in place, retaining
//! its output, log and temp-folder names so the CLI can copy the encoded result. Relative input
//! and restart info paths in the supplied job are resolved against that job's directory. The
//! source job is not changed.
//!
//! The runtime switches (DTHD_OAMD_*, DTHD_AUDIO_*, DTHD_DRC, DTHD_HIGHRES) patch the encoder,
//! not the job file, so they apply on top of whatever job is read.

use crate::site::Resolver;
use std::ffi::{CStr, CString};
use std::io::Write;
use std::os::raw::c_char;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

type FnSet = extern "C" fn(usize, *const c_char) -> bool;

static ENTRY_TRAMP: AtomicUsize = AtomicUsize::new(0);
static REDIRECT_PATH: AtomicUsize = AtomicUsize::new(0);

fn tag_value<'a>(s: &'a str, tag: &str) -> Result<&'a str, String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = s.find(&open).ok_or_else(|| format!("missing <{tag}>"))? + open.len();
    let end = s[start..]
        .find(&close)
        .ok_or_else(|| format!("missing {close}"))?
        + start;
    Ok(&s[start..end])
}

fn replace_tag(s: &str, tag: &str, value: &str) -> Result<String, String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = s.find(&open).ok_or_else(|| format!("missing <{tag}>"))? + open.len();
    let end = s[start..]
        .find(&close)
        .ok_or_else(|| format!("missing {close}"))?
        + start;
    Ok(format!("{}{}{}", &s[..start], value, &s[end..]))
}

fn absolutize_files(mut s: String, tag: &str, origin: &Path) -> Result<String, String> {
    let mut scan = 0;
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    while let Some(found) = s[scan..].find(&open) {
        let start = scan + found + open.len();
        let end = s[start..]
            .find(&close)
            .ok_or_else(|| format!("missing {close}"))?
            + start;
        let name = s[start..end].trim();
        if !name.is_empty() && !Path::new(name).is_absolute() {
            let abs = origin.join(name).to_string_lossy().into_owned();
            s.replace_range(start..end, &abs);
            scan = start + abs.len() + close.len();
        } else {
            scan = end + close.len();
        }
    }
    Ok(s)
}

fn substitute_job(path: *const c_char) -> Result<String, String> {
    let generated = unsafe { CStr::from_ptr(path) }
        .to_str()
        .map_err(|_| "generated .mll path is not UTF-8".to_string())?;
    let source = unsafe { CStr::from_ptr(REDIRECT_PATH.load(Ordering::Relaxed) as *const c_char) }
        .to_str()
        .map_err(|_| "supplied .mll path is not UTF-8".to_string())?;
    let envelope = std::fs::read_to_string(generated)
        .map_err(|e| format!("read generated {generated}: {e}"))?;
    let mut job =
        std::fs::read_to_string(source).map_err(|e| format!("read supplied {source}: {e}"))?;
    let origin = Path::new(source)
        .parent()
        .ok_or("supplied .mll has no parent directory")?;
    job = absolutize_files(job, "file", origin)?;
    job = absolutize_files(job, "restart-info-file", origin)?;
    for tag in ["output", "log", "temp-folder"] {
        job = replace_tag(&job, tag, tag_value(&envelope, tag)?)?;
    }
    std::fs::write(generated, job).map_err(|e| format!("write generated {generated}: {e}"))?;
    Ok(format!("{source} -> {generated}"))
}

extern "C" fn redirect_entry(this: usize, _path: *const c_char) -> bool {
    match substitute_job(_path) {
        Ok(what) => log::debug!(target: "dshim::mll", "substituted job ({what})"),
        Err(e) => {
            log::error!(target: "dshim::mll", "{e}, stopping");
            std::process::exit(3);
        }
    }
    let tr: FnSet = unsafe { std::mem::transmute(ENTRY_TRAMP.load(Ordering::Relaxed)) };
    tr(this, _path)
}

pub fn install(res: &Resolver, log: &mut dyn Write) {
    let Some(path) = crate::levers().dthd.mll.clone() else {
        return;
    };
    // An absolute path so the engine resolves it regardless of its working directory; fall back
    // to the given path if it cannot be canonicalised (e.g. it does not exist yet), letting the
    // engine report the missing file rather than silently encoding the CLI job.
    let abs = std::fs::canonicalize(&path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.clone());
    let Ok(cpath) = CString::new(abs.clone()) else {
        lerr!(log, "mll", "DTHD_MLL path has an interior NUL, stopping");
        let _ = log.flush();
        std::process::exit(3);
    };
    // Leak the buffer so its pointer stays valid for the process (one .mll per run).
    let ptr = cpath.into_raw() as u64;

    // Replace the generated job in place, retaining the CLI's output envelope.
    let Some(r) = res.site("dthd.mll.setoptions_entry", log) else {
        lerr!(
            log,
            "mll",
            "DTHD_MLL needs the options file reader entry, stopping"
        );
        let _ = log.flush();
        std::process::exit(3);
    };
    REDIRECT_PATH.store(ptr as usize, Ordering::Relaxed);
    match r.detour(redirect_entry as *const () as usize) {
        Some(t) => {
            ENTRY_TRAMP.store(t, Ordering::Relaxed);
            lok!(
                log,
                "mll",
                "substitute generated job from {abs} @ {:#x}",
                r.addr
            );
        }
        None => {
            lerr!(
                log,
                "mll",
                "could not detour the options file reader, stopping"
            );
            let _ = log.flush();
            std::process::exit(3);
        }
    }
}
