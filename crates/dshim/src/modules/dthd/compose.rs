//! Capture-and-mutate .mll composer for dee_dthd_encoder.
//!
//! The stock CLI composes its whole .mll job in memory, writes it to its temp-dir/UUID path
//! (fopen "w" + fclose), then the encoder's options file reader reads it back from disk and
//! chdirs to its directory (so relative <file> entries resolve). The reader is called on every
//! encode path. We detour its entry (`dthd.mll.setoptions_entry`), read the composed .mll at the
//! path in its second argument, rewrite it in place, and chain through with the path unchanged.
//! No byte patches: every refusal that would otherwise force a hand-authored .mll (the BluRay
//! 44.1 rejection, the two-independent-presentation limit, the hardcoded BluRay target) is keyed
//! off the .mll content or the front-end, so editing the file the engine reads is enough.
//!
//! The mutation preserves the CLI's envelope. The CLI writes relative <output>/<log> temp names
//! and copies the temp output to the real -o after encoding; authoring our own <output> would
//! break that copy and leave the run reporting failure. So <output>, <log> and <temp-folder> come
//! from the captured file and only the body changes.
//!
//! Levers (DTHD_COMPOSE alone, or DTHD_TARGET and DTHD_LAYOUT; the CLI refuses each with
//! DTHD_MLL):
//!   DTHD_TARGET=<bluray|dvd-a|hddvd-video|non-conformant-fba>
//!                     swap the <disctypes> target in place. dvd-a also reshapes the job into
//!                     FBB (to_fbb). non-conformant-fba writes
//!                     HD DVD-Video and turns the high-rate gates on (highres.rs).
//!   DTHD_LAYOUT=[TIER=]SPEC;..
//!                     relabel presentations (layout.rs), before a DTHD_TARGET swap.
//!   DTHD_COMPOSE=<path>
//!                     a full .mll body template authored by dee_shim with the placeholders
//!                     __DSHIM_OUTPUT__, __DSHIM_LOG__ and __DSHIM_TEMP__, filled from the
//!                     captured envelope. This is the path for the rare channel layouts (the
//!                     <group1> and <use>-order levers live in the authored independent tier),
//!                     the 44.1 kHz family and more than two presentations.
//!
//! The engine encodes the .mll's own <file> inputs, not the CLI feed (the CLI feed is the dummy
//! that passes front-end validation), as the DTHD_MLL redirect does.

use crate::site::Resolver;
use std::io::Write;
use std::os::raw::c_char;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

type FnSet = extern "C" fn(usize, *const c_char) -> bool;

static TRAMP: AtomicUsize = AtomicUsize::new(0);
static DONE: AtomicBool = AtomicBool::new(false);

fn fail(msg: &str, log: &mut dyn Write) -> ! {
    lerr!(log, "dthd", "compose: {msg}, stopping");
    let _ = log.flush();
    std::process::exit(3);
}

/// The value between <tag> and </tag>, trimmed. None if the tag is absent.
pub(super) fn tag_value<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let a = s.find(&open)? + open.len();
    let b = s[a..].find(&close)? + a;
    Some(s[a..b].trim())
}

/// Replace the whole `<disctypes>..</disctypes>` block with one naming `target`. DVD-A is FBB,
/// so the CLI's FBA job is also reshaped for it (`to_fbb`).
fn swap_target(mll: &str, target: &str) -> Result<String, String> {
    let open = mll
        .find("<disctypes>")
        .ok_or("no <disctypes> block in the composed .mll")?;
    let close = mll[open..]
        .find("</disctypes>")
        .ok_or("no </disctypes> in the composed .mll")?
        + open
        + "</disctypes>".len();
    let block = format!("<disctypes>\n    <{target}>true</{target}>\n  </disctypes>");
    let out = format!("{}{}{}", &mll[..open], block, &mll[close..]);
    if target == "DVD-A" {
        to_fbb(&out)
    } else {
        Ok(out)
    }
}

/// Elements an FBB stream rejects in a presentation.
const FBA_ONLY: [&str; 10] = [
    "control-enabled",
    "dialogue-level",
    "dialogue-norm",
    "mix-level",
    "reference-level",
    "SPL-at-reference-level",
    "channel-modifier",
    "attenuate-surrounds",
    "dynamic-range-compression",
    "surround-EX",
];

/// Each `<presentation>..</presentation>` block with its line ending. The Windows CLI ends some
/// lines with CRLF and some with LF, so the blocks are found by their tags.
pub(super) fn presentations(region: &str) -> Result<Vec<&str>, String> {
    const END: &str = "</presentation>";
    let mut blocks = Vec::new();
    let mut at = 0;
    while let Some(start) = region[at..].find("<presentation>") {
        let start = at + start;
        let mut end = region[start..]
            .find(END)
            .ok_or("unclosed <presentation> in the composed .mll")?
            + start
            + END.len();
        for eol in ["\r\n", "\n"] {
            if region[end..].starts_with(eol) {
                end += eol.len();
                break;
            }
        }
        blocks.push(&region[start..end]);
        at = end;
    }
    Ok(blocks)
}

/// FBB peak data rate limit, bits per second.
const FBB_PEAK_RATE: u32 = 9_600_000;

/// The longest restart gap FBB allows (8 to 32); the CLI writes FBA's 128.
const FBB_RESTART_GAP: u32 = 32;

/// Reshape the CLI's FBA job into an FBB one. The CLI always writes the eight- and six-channel
/// tiers, as copies of the input's tier where the input has fewer channels; FBB carries at most
/// six channels, so the eight-channel copy goes and a real eight- or sixteen-channel tier is
/// refused; FBB needs the six-channel tier defined, so its copy of a two-channel input stays.
/// The FBA-only elements are dropped, and the peak data rate and restart gap come down to FBB's
/// limits.
/// Where the job's presentation blocks end: after the last </presentation> and its line. What
/// follows varies (<restart-points> when the 8ch is the Atmos render, <extra-data> for Atmos).
pub fn presentations_end(mll: &str) -> Result<usize, String> {
    let close = "</presentation>";
    let at = mll
        .rfind(close)
        .ok_or("no </presentation> in the composed .mll")?
        + close.len();
    Ok(mll[at..].find('\n').map_or(mll.len(), |n| at + n + 1))
}

fn to_fbb(mll: &str) -> Result<String, String> {
    let first = mll
        .find("<presentation>")
        .ok_or("no <presentation> in the composed .mll")?;
    let rest = presentations_end(mll)?;
    let mut body = String::new();
    for block in presentations(&mll[first..rest])? {
        let kind = tag_value(block, "type").unwrap_or_default();
        if block.contains("<copy-of>") {
            // FBB defines the six-channel tier even for a two-channel input, as a copy.
            if kind == "six-channel" {
                body.push_str(block);
            }
            continue;
        }
        if matches!(kind, "eight-channel" | "sixteen-channel") {
            return Err(format!(
                "DVD-A (FBB) carries at most six channels, and this job has a {kind} tier; use \
                 a 2ch or 6ch presentation"
            ));
        }
        for line in block.split_inclusive('\n') {
            let fba = FBA_ONLY
                .iter()
                .any(|t| line.trim_start().starts_with(&format!("<{t}>")));
            if !fba {
                body.push_str(line);
            }
        }
    }
    let head = &mll[..first];
    let rate =
        tag_value(head, "peak-data-rate").ok_or("no <peak-data-rate> in the composed .mll")?;
    let head = head.replace(
        &format!("<peak-data-rate>{rate}</peak-data-rate>"),
        &format!("<peak-data-rate>{FBB_PEAK_RATE}</peak-data-rate>"),
    );
    let tail = &mll[rest..];
    let gap = tag_value(tail, "restart-gap").ok_or("no <restart-gap> in the composed .mll")?;
    let tail = tail.replace(
        &format!("<restart-gap>{gap}</restart-gap>"),
        &format!("<restart-gap>{FBB_RESTART_GAP}</restart-gap>"),
    );
    Ok(format!("{head}{body}{tail}"))
}

/// Fill a dee_shim-authored template with the captured envelope's output/log/temp-folder.
fn apply_template(captured: &str, template: &str) -> Result<String, String> {
    let out = tag_value(captured, "output").ok_or("captured .mll has no <output>")?;
    let log = tag_value(captured, "log").ok_or("captured .mll has no <log>")?;
    let temp = tag_value(captured, "temp-folder").ok_or("captured .mll has no <temp-folder>")?;
    let filled = template
        .replace("__DSHIM_OUTPUT__", out)
        .replace("__DSHIM_LOG__", log)
        .replace("__DSHIM_TEMP__", temp);
    let filled = fill_slots(captured, filled)?;
    if filled.contains("__DSHIM_") {
        return Err("template still has an unfilled __DSHIM_* placeholder".to_string());
    }
    Ok(filled)
}

/// Fill `__DSHIM_SLOT_<TIER>_<K>__` with the file the CLI wrote for slot K of that independent
/// presentation of its job (`top` for the first one).
fn fill_slots(captured: &str, mut filled: String) -> Result<String, String> {
    const MARK: &str = "__DSHIM_SLOT_";
    if !filled.contains(MARK) {
        return Ok(filled);
    }
    let first = captured.find("<presentation>").unwrap_or(captured.len());
    let mut tiers: Vec<(String, Vec<String>)> = Vec::new();
    for block in presentations(&captured[first..])? {
        if block.contains("<copy-of>") || block.contains("<downmix-of>") {
            continue;
        }
        let kind = tag_value(block, "type").unwrap_or_default();
        let files = block
            .split("<file>")
            .skip(1)
            .filter_map(|f| f.split_once("</file>").map(|(f, _)| f.to_string()))
            .collect();
        tiers.push((kind.to_string(), files));
    }
    while let Some(a) = filled.find(MARK) {
        let b = filled[a + MARK.len()..]
            .find("__")
            .ok_or("an unclosed slot placeholder")?
            + a
            + MARK.len();
        let key = &filled[a + MARK.len()..b];
        let (tier, k) = key
            .rsplit_once('_')
            .ok_or("a slot placeholder without a slot")?;
        let k: usize = k.parse().map_err(|_| format!("slot {key:?}"))?;
        let kind = match tier {
            "2ch" => Some("two-channel"),
            "6ch" => Some("six-channel"),
            "8ch" => Some("eight-channel"),
            _ => None,
        };
        let files = match kind {
            Some(kind) => tiers.iter().find(|(t, _)| t == kind),
            None => tiers.first(),
        }
        .map(|(_, f)| f)
        .ok_or_else(|| {
            format!(
                "slot {}: the CLI job has no such independent presentation",
                k + 1
            )
        })?;
        let file = files.get(k).ok_or_else(|| {
            format!(
                "slot {}: the CLI's presentation has {} slots",
                k + 1,
                files.len()
            )
        })?;
        filled.replace_range(a..b + 2, file);
    }
    Ok(filled)
}

/// Read the composed .mll at `path`, apply the spec, write it back in place.
fn mutate(path: *const c_char) -> Result<String, String> {
    let p = unsafe { std::ffi::CStr::from_ptr(path) }
        .to_str()
        .map_err(|_| "options file path is not UTF-8".to_string())?;
    let captured = std::fs::read_to_string(p).map_err(|e| format!("read {p}: {e}"))?;
    let dthd = &crate::levers().dthd;

    let mut what = Vec::new();
    let out = if let Some(tmpl) = &dthd.compose {
        let t = std::fs::read_to_string(tmpl).map_err(|e| format!("read template {tmpl}: {e}"))?;
        what.push("compose".to_string());
        apply_template(&captured, &t)?
    } else {
        let mut cur = captured.clone();
        if let Some(l) = &dthd.layout {
            let fbb = dthd.target.is_some_and(|t| t.mll_name() == "DVD-A");
            cur = super::layout::apply(&cur, l, fbb)?;
            what.push(format!("layout={l}"));
        }
        if let Some(t) = dthd.target.map(|t| t.mll_name()) {
            cur = swap_target(&cur, t)?;
            what.push(format!("target={t}"));
        }
        cur
    };
    std::fs::write(p, out).map_err(|e| format!("write {p}: {e}"))?;
    Ok(format!("{} -> {p}", what.join(" ")))
}

extern "C" fn hook(this: usize, path: *const c_char) -> bool {
    if !DONE.swap(true, Ordering::Relaxed) {
        match mutate(path) {
            Ok(what) => log::debug!(target: "dshim::dthd", "compose: mutated .mll ({what})"),
            Err(e) => {
                log::error!(target: "dshim::dthd", "compose: {e}, stopping");
                std::process::exit(3);
            }
        }
    }
    let tr: FnSet = unsafe { std::mem::transmute(TRAMP.load(Ordering::Relaxed)) };
    tr(this, path)
}

pub fn install(res: &Resolver, log: &mut dyn Write) {
    // The CLI refuses DTHD_MLL with either, and DTHD_COMPOSE with DTHD_TARGET.
    let dthd = &crate::levers().dthd;
    if dthd.target.is_none() && dthd.compose.is_none() && dthd.layout.is_none() {
        return;
    }

    let Some(r) = res.site("dthd.mll.setoptions_entry", log) else {
        fail(
            "compose switch set but dthd.mll.setoptions_entry is not in this profile",
            log,
        );
    };
    match r.detour(hook as *const () as usize) {
        Some(t) => {
            TRAMP.store(t, Ordering::Relaxed);
            let mode = dthd
                .compose
                .as_deref()
                .map(|p| format!("compose {p}"))
                .or_else(|| dthd.target.map(|t| format!("target {}", t.mll_name())))
                .or_else(|| dthd.layout.as_deref().map(|l| format!("layout {l}")))
                .unwrap_or_default();
            lok!(
                log,
                "dthd",
                "compose: hook the options file reader @ {:#x} ({mode})",
                r.addr
            );
        }
        None => fail("could not detour the options file reader", log),
    }

    // An Atmos job's 8ch tier is panned from the 16 elements, and the panner takes 5.1 plus a
    // height, top, wide, centre pair or Cs; the job parser admits only the 7.1 speakers there.
    if dthd.layout.is_some() {
        let Some(r) = res.site("dthd.mll.downmix8_speakers", log) else {
            fail(
                "--layout set but dthd.mll.downmix8_speakers is not in this profile",
                log,
            );
        };
        let dshim_profiles::Kind::Patch { new } = r.kind else {
            fail("dthd.mll.downmix8_speakers is not a patch site", log);
        };
        if !crate::write_code(r.addr, &dshim_profiles::hex(new)) {
            fail("could not lift the 8ch downmix speaker rule", log);
        }
        lok!(
            log,
            "dthd",
            "layout: 8ch downmix speaker rule lifted @ {:#x}",
            r.addr
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "<encode>\n  <disctypes>\n    <BluRay>true</BluRay>\n  </disctypes>\n  \
        <data-rate>\n    <peak-data-rate>18000000</peak-data-rate>\n  </data-rate>\n";
    const TAIL: &str = "<restart-points>\n  <restart-gap>128</restart-gap>\n</restart-points>\n\
        </encode>\n";

    fn job(body: &str) -> String {
        format!("{HEAD}{body}{TAIL}")
    }

    #[test]
    fn dvd_a_reshapes_a_two_channel_job() {
        let mll = job(
            "<presentation>\n<type>eight-channel</type>\n<copy-of>two-channel</copy-of>\n\
            </presentation>\n<presentation>\n<type>six-channel</type>\n\
            <copy-of>two-channel</copy-of>\n</presentation>\n<presentation>\n\
            <type>two-channel</type>\n<control-enabled>true</control-enabled>\n\
            <dialogue-level>-29</dialogue-level>\n\
            <dynamic-range-compression>Film-Light</dynamic-range-compression>\n\
            <apodise>false</apodise>\n  <inputs>\n  </inputs>\n</presentation>\n",
        );
        let out = swap_target(&mll, "DVD-A").unwrap();
        assert!(out.contains("<DVD-A>true</DVD-A>"));
        assert!(!out.contains("eight-channel"));
        assert!(out.contains("<type>six-channel</type>\n<copy-of>two-channel</copy-of>"));
        for gone in [
            "control-enabled",
            "dialogue-level",
            "dynamic-range-compression",
        ] {
            assert!(!out.contains(gone), "{gone} kept");
        }
        assert!(out.contains("<apodise>false</apodise>"));
        assert!(out.contains("<peak-data-rate>9600000</peak-data-rate>"));
        assert!(out.contains("<restart-gap>32</restart-gap>"));
    }

    #[test]
    fn dvd_a_reads_crlf_jobs() {
        let mll = job("<presentation>\r\n<type>eight-channel</type>\r\n\
            <copy-of>six-channel</copy-of>\r\n</presentation>\r\n<presentation>\r\n\
            <type>six-channel</type>\n<control-enabled>false</control-enabled>\r\n\
            </presentation>\r\n");
        let out = swap_target(&mll, "DVD-A").unwrap();
        assert!(!out.contains("eight-channel"));
        assert!(out.contains("<type>six-channel</type>"));
        assert!(!out.contains("control-enabled"));
    }

    #[test]
    fn slots_take_the_cli_files() {
        let cap = job(
            "<presentation>\n<type>eight-channel</type>\n<copy-of>six-channel</copy-of>\n\
            </presentation>\n<presentation>\n<type>six-channel</type>\n  <inputs>\n\
            <channel><use>L</use><file>p_06.00.wav</file></channel>\n\
            <channel><use>R</use><file>p_06.01.wav</file></channel>\n  </inputs>\n\
            </presentation>\n",
        );
        let cap = cap.replace(
            "<encode>\n",
            "<encode>\n<output>o</output><log>l</log>\
            <temp-folder>t</temp-folder>\n",
        );
        let t = "<output>__DSHIM_OUTPUT__</output><log>__DSHIM_LOG__</log>\
            <temp-folder>__DSHIM_TEMP__</temp-folder><file>__DSHIM_SLOT_top_1__</file>\
            <file>__DSHIM_SLOT_6ch_0__</file>";
        let out = apply_template(&cap, t).unwrap();
        assert!(out.ends_with("<file>p_06.01.wav</file><file>p_06.00.wav</file>"));
        assert!(apply_template(&cap, "__DSHIM_SLOT_2ch_0__").is_err());
        assert!(apply_template(&cap, "__DSHIM_SLOT_top_5__").is_err());
    }

    #[test]
    fn dvd_a_refuses_an_eight_channel_tier() {
        let mll = job(
            "<presentation>\n<type>eight-channel</type>\n<apodise>false</apodise>\n\
            </presentation>\n",
        );
        assert!(swap_target(&mll, "DVD-A").is_err());
        assert!(swap_target(&mll, "HDDVD-Video").is_ok());
    }
}
