//! `dthd --compose spec.json`: author a `.mll` job template from a high-level JSON spec.
//!
//! The template is a complete `<encode>` job, with the element order and names the engine's
//! parser accepts. `<output>`, `<log>` and `<temp-folder>` hold the placeholders
//! `__DSHIM_OUTPUT__`, `__DSHIM_LOG__` and `__DSHIM_TEMP__`, which the engine fills from the
//! CLI's own composed `.mll` at encode time (`crates/dshim/src/modules/dthd/compose.rs`), so the
//! CLI still copies its output into place and exits 0.
//!
//! The spec therefore has no output or temp fields. The engine encodes the presentations' own
//! inputs: a channel's `file`, an absolute path, or its `slot`, a slot of the CLI input after
//! `--` (`K` for slot K of the CLI's top presentation, `2ch:K`, `6ch:K` or `8ch:K` for another
//! independent presentation, counted from 1). A slot is written as the placeholder
//! `__DSHIM_SLOT_<TIER>_<K-1>__`, `top` for the top presentation, which the engine fills with
//! the per-channel file the CLI wrote for it. Without slots the CLI input is only a dummy that
//! passes front-end validation.

use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
pub struct Spec {
    /// BluRay | DVD-A | HDDVD-Video (default BluRay).
    #[serde(default)]
    disctype: Option<String>,
    #[serde(default)]
    peak_data_rate: Option<u64>,
    #[serde(default)]
    filter_type: Option<String>,
    #[serde(default)]
    restart_gap: Option<u32>,
    #[serde(default)]
    subsequent_restarts: Option<u32>,
    #[serde(default)]
    verify: Option<bool>,
    #[serde(default)]
    report: Option<bool>,
    #[serde(default)]
    decode_check: Option<bool>,
    #[serde(default)]
    downmix_coef_check: Option<bool>,
    presentations: Vec<Presentation>,
}

#[derive(Deserialize)]
struct Presentation {
    #[serde(rename = "type")]
    kind: String,
    /// "independent" (default), "downmix-of:<type>", or "copy-of:<type>".
    #[serde(default)]
    mode: Option<String>,
    /// Group 1 channel count, which tells apart FBB layouts that are otherwise ambiguous.
    #[serde(default)]
    group1: Option<u32>,
    #[serde(default)]
    dialogue_level: Option<i32>,
    #[serde(default)]
    dialogue_norm: Option<i32>,
    #[serde(default)]
    mix_level: Option<i32>,
    #[serde(default)]
    reference_level: Option<i32>,
    #[serde(default)]
    spl_at_reference_level: Option<i32>,
    #[serde(default)]
    drc: Option<String>,
    #[serde(default)]
    channel_modifier: Option<String>,
    #[serde(default)]
    attenuate_surrounds: Option<bool>,
    #[serde(default)]
    apply_noise_shaping: Option<bool>,
    #[serde(default)]
    apodise: Option<bool>,
    #[serde(default)]
    upsample: Option<bool>,
    #[serde(default)]
    surround_ex: Option<bool>,
    #[serde(default)]
    gain: Option<i32>,
    #[serde(default)]
    gain_check: Option<bool>,
    #[serde(default)]
    channels: Vec<Channel>,
}

#[derive(Deserialize)]
struct Channel {
    #[serde(rename = "use")]
    name: String,
    #[serde(default)]
    file: Option<String>,
    /// A slot of the CLI's input, `K` or `TIER:K`, in place of `file`.
    #[serde(default)]
    slot: Option<serde_json::Value>,
    /// Downmix coefficients by ref, emitted sorted by ref; the engine keys on ref, not position.
    #[serde(default)]
    coef: Option<BTreeMap<String, f64>>,
}

/// The channel tokens the engine's `<use>` parser accepts (its channel name table, 25 entries).
/// FBA uses the whole set (height, top, wide, object); FBB uses L, R, C, LFE, Ls, Rs and Cs. This
/// only catches typos, naming the vocabulary early; a token the engine rejects for the stream
/// type (a top channel in an FBB job) still passes here.
const USES: &[&str] = &dshim_profiles::dthd_layouts::TOKENS;

/// The placeholder for a channel's `slot`: `K` or `TIER:K`, K counted from 1.
fn slot_placeholder(v: &serde_json::Value) -> Result<String, String> {
    let text = match v {
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        _ => return Err(format!("slot {v} is not K or TIER:K")),
    };
    let (tier, k) = match text.split_once(':') {
        Some((t, k)) => match t {
            "2ch" | "6ch" | "8ch" => (t, k),
            _ => {
                return Err(format!(
                    "slot {text:?}: presentation {t:?} unknown (2ch, 6ch or 8ch)"
                ))
            }
        },
        None => ("top", text.as_str()),
    };
    match k.parse::<u32>() {
        Ok(k @ 1..=8) => Ok(format!("__DSHIM_SLOT_{tier}_{}__", k - 1)),
        _ => Err(format!("slot {text:?}: K is a slot from 1 to 8")),
    }
}

fn check_use(tok: &str) -> Result<(), String> {
    if USES.contains(&tok) {
        Ok(())
    } else {
        Err(format!(
            "channel <use> {tok:?} is not a known token (one of {})",
            USES.join(" ")
        ))
    }
}

fn disctype_canon(v: &str) -> Result<&'static str, String> {
    match v {
        "BluRay" => Ok("BluRay"),
        "DVD-A" => Ok("DVD-A"),
        "HDDVD-Video" => Ok("HDDVD-Video"),
        _ => Err(format!(
            "disctype {v:?} unknown (BluRay, DVD-A or HDDVD-Video)"
        )),
    }
}

fn b(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

fn emit_presentation(p: &Presentation, fbb: bool, out: &mut String) -> Result<(), String> {
    out.push_str("<presentation>\n");
    out.push_str(&format!("<type>{}</type>\n", p.kind));
    let mode = p.mode.as_deref().unwrap_or("independent");
    let independent = if let Some(src) = mode.strip_prefix("downmix-of:") {
        out.push_str(&format!("<downmix-of>{src}</downmix-of>\n"));
        false
    } else if let Some(src) = mode.strip_prefix("copy-of:") {
        out.push_str(&format!("<copy-of>{src}</copy-of>\n</presentation>\n"));
        return Ok(());
    } else if mode == "independent" {
        true
    } else {
        return Err(format!(
            "presentation mode {mode:?} unknown (independent, downmix-of:<type>, copy-of:<type>)"
        ));
    };

    // The 6ch and 8ch modifier fields have no Mono: their 11b is reserved (FBA reference,
    // Annex E), and DEE writes it unchecked. Mono is a 2ch format only.
    if p.channel_modifier
        .as_deref()
        .is_some_and(|m| m.eq_ignore_ascii_case("mono"))
        && matches!(p.kind.as_str(), "six-channel" | "eight-channel")
    {
        return Err(format!(
            "{}: channel_modifier Mono is for a two-channel presentation only; the 6ch and 8ch \
             field has no Mono (its value would be reserved). Use Stereo, LtRt or LBinRBin, or \
             a lone C for a mono channel",
            p.kind
        ));
    }

    // Elements in the order the parser accepts, per scope (independent and downmix). Several
    // are FBA-only: an FBB (DVD-A) stream rejects control-enabled, channel-modifier,
    // attenuate-surrounds, dynamic-range-compression, surround-EX and the dialogue/mix/reference
    // levels, so they are emitted only for an FBA target.
    let el = |tag: &str, val: &str, o: &mut String| o.push_str(&format!("<{tag}>{val}</{tag}>\n"));
    if !fbb {
        el(
            "control-enabled",
            if independent { "false" } else { "true" },
            out,
        );
    }
    if independent {
        if !fbb {
            if let Some(v) = p.dialogue_level {
                el("dialogue-level", &v.to_string(), out);
            }
            if let Some(v) = p.dialogue_norm {
                el("dialogue-norm", &v.to_string(), out);
            }
            if let Some(v) = p.mix_level {
                el("mix-level", &v.to_string(), out);
            }
            if let Some(v) = p.reference_level {
                el("reference-level", &v.to_string(), out);
            }
            if let Some(v) = p.spl_at_reference_level {
                el("SPL-at-reference-level", &v.to_string(), out);
            }
        }
        if !fbb {
            if let Some(v) = &p.drc {
                el("dynamic-range-compression", v, out);
            }
        }
        if !fbb {
            if let Some(v) = &p.channel_modifier {
                el("channel-modifier", v, out);
            }
            if let Some(v) = p.attenuate_surrounds {
                el("attenuate-surrounds", b(v), out);
            }
        }
        if let Some(v) = p.apply_noise_shaping {
            el("apply-noise-shaping", b(v), out);
        }
        if let Some(v) = p.apodise {
            el("apodise", b(v), out);
        }
        if let Some(v) = p.upsample {
            el("upsample", b(v), out);
        }
        if !fbb {
            if let Some(v) = p.surround_ex {
                el("surround-EX", b(v), out);
            }
        }
    } else {
        if !fbb {
            if let Some(v) = &p.drc {
                el("dynamic-range-compression", v, out);
            }
        }
        if !fbb {
            if let Some(v) = &p.channel_modifier {
                el("channel-modifier", v, out);
            }
        }
        if let Some(v) = p.gain {
            el("gain", &v.to_string(), out);
        }
        if let Some(v) = p.gain_check {
            el("gain-check", b(v), out);
        }
        if !fbb {
            if let Some(v) = p.surround_ex {
                el("surround-EX", b(v), out);
            }
        }
    }

    if !p.channels.is_empty() {
        out.push_str("  <inputs>\n");
        // group1 belongs to <inputs> (it sets the FBB channel assignment) and must come first.
        if let Some(g) = p.group1 {
            out.push_str(&format!("    <group1>{g}</group1>\n"));
        }
        for c in &p.channels {
            check_use(&c.name)?;
            if let Some(coef) = &c.coef {
                out.push_str(&format!("  <channel>\n  <use>{}</use>\n", c.name));
                for (r, v) in coef {
                    check_use(r)?;
                    out.push_str(&format!(
                        "    <coefficient ref=\"{r}\">{v:.6}</coefficient>\n"
                    ));
                }
                out.push_str("  </channel>\n");
            } else {
                let file = match (&c.file, &c.slot) {
                    (Some(f), None) => f.clone(),
                    (None, Some(s)) => slot_placeholder(s)?,
                    (Some(_), Some(_)) => {
                        return Err(format!("channel {:?} has both a file and a slot", c.name))
                    }
                    (None, None) => {
                        return Err(format!(
                            "channel {:?} needs a file or slot (independent) or coef (downmix)",
                            c.name
                        ))
                    }
                };
                out.push_str(&format!(
                    "    <channel><use>{}</use><file>{}</file></channel>\n",
                    c.name, file
                ));
            }
        }
        out.push_str("  </inputs>\n");
    }
    out.push_str("</presentation>\n");
    Ok(())
}

/// Author the whole `<encode>` job from the spec, with the placeholders described above.
pub fn build_mll(spec: &Spec) -> Result<String, String> {
    if spec.presentations.is_empty() {
        return Err("spec has no presentations".to_string());
    }
    let dt = disctype_canon(spec.disctype.as_deref().unwrap_or("BluRay"))?;
    let fbb = dt == "DVD-A"; // BluRay and HDDVD-Video are FBA; DVD-A is FBB
                             // FBB's own limits: 9.6 Mbps peak and a restart gap of 8 to 32.
    let (peak, gap) = if fbb {
        (9_600_000, 32)
    } else {
        (18_000_000, 128)
    };
    let mut g = String::new();
    g.push_str("<encode>\n");
    g.push_str(&format!(
        "  <disctypes>\n    <{dt}>true</{dt}>\n  </disctypes>\n"
    ));
    g.push_str("  <progress-report>on</progress-report>\n");
    g.push_str("  <output>__DSHIM_OUTPUT__</output>\n");
    g.push_str("  <log>__DSHIM_LOG__</log>\n");
    g.push_str("  <data-rate-table-file>false</data-rate-table-file>\n");
    g.push_str("  <temp-folder>\"__DSHIM_TEMP__\"</temp-folder>\n");
    g.push_str("  <movie-downmix-restrictions>false</movie-downmix-restrictions>\n");
    g.push_str("  <use-parallel>true</use-parallel>\n");
    g.push_str(&format!(
        "  <data-rate>\n    <peak-data-rate>{}</peak-data-rate>\n  </data-rate>\n",
        spec.peak_data_rate.unwrap_or(peak)
    ));
    if let Some(ft) = &spec.filter_type {
        g.push_str(&format!("  <filter-type>{ft}</filter-type>\n"));
    }
    g.push_str("  <verification>\n");
    g.push_str(&format!(
        "    <verify>{}</verify><report>{}</report>\n",
        b(spec.verify.unwrap_or(true)),
        b(spec.report.unwrap_or(true))
    ));
    g.push_str(&format!(
        "    <decode-check>{}</decode-check><downmix-coef-check>{}</downmix-coef-check>\n",
        b(spec.decode_check.unwrap_or(false)),
        b(spec.downmix_coef_check.unwrap_or(true))
    ));
    g.push_str("  </verification>\n");

    // Tiers largest first, as the CLI emits them.
    let rank = |t: &str| match t {
        "sixteen-channel" => 3,
        "eight-channel" => 2,
        "six-channel" => 1,
        "two-channel" => 0,
        _ => 9,
    };
    let mut order: Vec<&Presentation> = spec.presentations.iter().collect();
    order.sort_by_key(|p| std::cmp::Reverse(rank(&p.kind)));
    for p in order {
        emit_presentation(p, fbb, &mut g)?;
    }

    g.push_str(&format!(
        "<restart-points><restart-gap>{}</restart-gap><subsequent-restarts>{}</subsequent-restarts></restart-points>\n",
        spec.restart_gap.unwrap_or(gap), spec.subsequent_restarts.unwrap_or(1)));
    g.push_str("</encode>\n");
    Ok(g)
}

/// Parse a spec file and author its `.mll` template. `noise_shaping` and `apodise` (from
/// --noise-shaping, --apodise and --as-is, which act on the CLI's own job and so not on
/// this one) fill a presentation that leaves the field out.
pub fn author(
    spec_path: &str,
    noise_shaping: Option<bool>,
    apodise: Option<bool>,
) -> Result<String, String> {
    let text = std::fs::read_to_string(spec_path).map_err(|e| format!("read {spec_path}: {e}"))?;
    let mut spec: Spec =
        serde_json::from_str(&text).map_err(|e| format!("parse {spec_path}: {e}"))?;
    for p in &mut spec.presentations {
        p.apply_noise_shaping = p.apply_noise_shaping.or(noise_shaping);
        p.apodise = p.apodise.or(apodise);
    }
    build_mll(&spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(kind: &str, modifier: &str) -> Spec {
        serde_json::from_value(serde_json::json!({"presentations": [{
            "type": kind,
            "channel_modifier": modifier,
            "channels": [{"use": "L", "file": "/l.wav"}, {"use": "R", "file": "/r.wav"}],
        }]}))
        .unwrap()
    }

    #[test]
    fn mono_is_for_two_channel_presentations_only() {
        assert!(build_mll(&spec("two-channel", "Mono")).is_ok());
        assert!(build_mll(&spec("six-channel", "Mono")).is_err());
        assert!(build_mll(&spec("eight-channel", "mono")).is_err());
        assert!(build_mll(&spec("six-channel", "LtRt"))
            .unwrap()
            .contains("<channel-modifier>LtRt</channel-modifier>"));
    }
}
