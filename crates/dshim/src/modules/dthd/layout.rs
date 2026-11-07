//! Presentation layouts in the CLI's composed .mll (DTHD_LAYOUT, `dthd --layout`).
//!
//! The CLI fixes each presentation's channels: an independent tier takes its slots as L R (2ch),
//! L R C LFE Ls Rs (6ch) or those plus Lrs Rrs (8ch), from files p_NN.0k it writes from WAV
//! channel k; a downmix tier mixes the tier above with DEE's coefficients. A layout relabels one
//! tier. In an independent tier each slot gets the layout's channel and `-` slots are dropped; in
//! a downmix tier the layout picks the outputs. Every downmix tier whose source changed is
//! mixed again from its source's channels with DEE's own rule (a surround or back pair folds
//! into the pair in front of it at unity, a centre into the front pair at -3 dB, LFE drops), so
//! the 8ch to 6ch to 2ch chain of DEE's job is reproduced exactly when nothing is relabelled.
//! Without its own layout, a regenerated downmix tier drops outputs nothing feeds.
//!
//! Mono (1.0) in a two-channel tier is FBA's Mono modifier over L and R from the same file. FBB
//! (DVD-A) has no two-channel mono: there a mono tier is a six-channel tier with a lone C and no
//! two-channel tier, which DEE's verifier requires (a two-channel downmix of mono fails M108).

use super::compose::{presentations, tag_value};
use dshim_profiles::dthd_layouts::{self, Tier, TOKENS};

/// DEE's centre and back-centre fold, -3 dB.
const MINUS_3DB: f64 = 0.707946;

#[derive(Clone, Debug, PartialEq)]
enum Source {
    Own,
    Copy(String),
    Downmix(String),
}

#[derive(Clone, Debug)]
struct Chan {
    name: String,
    file: Option<String>,
}

#[derive(Clone, Debug)]
struct Pres {
    kind: String,
    source: Source,
    /// The block up to the line holding <inputs>.
    head: String,
    /// The block from the line after </inputs>.
    tail: String,
    eol: &'static str,
    chans: Vec<Chan>,
    /// Set when the block must be written again.
    changed: bool,
    mono: bool,
    /// An eight-channel tier of an Atmos job: the engine pans the 16 elements into whatever
    /// speakers it names, so it carries no coefficients (<matrices-from-file-name>).
    panned: bool,
    raw: String,
}

fn rank(name: &str) -> usize {
    TOKENS
        .iter()
        .position(|t| *t == name)
        .unwrap_or(TOKENS.len())
}

fn parse(block: &str) -> Result<Pres, String> {
    let kind = tag_value(block, "type")
        .ok_or("a presentation without <type> in the composed .mll")?
        .to_string();
    let source = if let Some(s) = tag_value(block, "copy-of") {
        Source::Copy(s.to_string())
    } else if let Some(s) = tag_value(block, "downmix-of") {
        Source::Downmix(s.to_string())
    } else {
        Source::Own
    };
    let eol = if block.contains("\r\n") { "\r\n" } else { "\n" };
    let mut p = Pres {
        kind,
        source,
        head: String::new(),
        tail: String::new(),
        eol,
        chans: Vec::new(),
        changed: false,
        mono: false,
        panned: block.contains("<matrices-from-file-name>"),
        raw: block.to_string(),
    };
    if matches!(p.source, Source::Copy(_)) {
        return Ok(p);
    }
    let open = block
        .find("<inputs>")
        .ok_or("a presentation without <inputs>")?;
    let open = block[..open].rfind('\n').map_or(0, |n| n + 1);
    let close = block
        .find("</inputs>")
        .ok_or("a presentation without </inputs>")?;
    let close = block[close..]
        .find('\n')
        .map_or(block.len(), |n| close + n + 1);
    p.head = block[..open].to_string();
    p.tail = block[close..].to_string();
    let mut at = open;
    while let Some(a) = block[at..close].find("<channel>") {
        let a = at + a;
        let b = block[a..]
            .find("</channel>")
            .ok_or("an unclosed <channel>")?
            + a;
        let seg = &block[a..b];
        p.chans.push(Chan {
            name: tag_value(seg, "use")
                .ok_or("a <channel> without <use>")?
                .to_string(),
            file: tag_value(seg, "file").map(str::to_string),
        });
        at = b;
    }
    Ok(p)
}

/// Where a channel goes when the tier below lacks it: its parents and their weights.
fn parents(name: &str) -> &'static [(&'static str, f64)] {
    match name {
        "C" => &[("L", MINUS_3DB), ("R", MINUS_3DB)],
        "Ls" | "Lvh" | "Lw" | "Lc" => &[("L", 1.0)],
        "Rs" | "Rvh" | "Rw" | "Rc" => &[("R", 1.0)],
        "Lrs" | "Lts" | "Lsd" => &[("Ls", 1.0)],
        "Rrs" | "Rts" | "Rsd" => &[("Rs", 1.0)],
        "Lrh" => &[("Lrs", 1.0)],
        "Rrh" => &[("Rrs", 1.0)],
        "Cs" => &[("Ls", MINUS_3DB), ("Rs", MINUS_3DB)],
        "Ts" | "Cvh" => &[("C", 1.0)],
        "LFE2" => &[("LFE", 1.0)],
        _ => &[],
    }
}

/// The gains of source channel `name` into the outputs `outs`.
fn fold(name: &str, outs: &[&str]) -> Vec<f64> {
    let mut g = vec![0.0; outs.len()];
    if let Some(i) = outs.iter().position(|o| *o == name) {
        g[i] = 1.0;
        return g;
    }
    for (p, w) in parents(name) {
        for (gi, pi) in g.iter_mut().zip(fold(p, outs)) {
            *gi += w * pi;
        }
    }
    g
}

fn write_own(p: &Pres) -> String {
    let e = p.eol;
    let mut s = format!("{}  <inputs>{e}", p.head);
    for c in &p.chans {
        s.push_str(&format!(
            "    <channel><use>{}</use><file>{}</file></channel>{e}",
            c.name,
            c.file.as_deref().unwrap_or_default()
        ));
    }
    format!("{s}  </inputs>{e}{}", p.tail)
}

/// The speakers the engine pans an Atmos job's 8ch tier into: 5.1 and one of these.
const PANNED_EXTRAS: [&[&str]; 6] = [
    &["Lrs", "Rrs"],
    &["Lvh", "Rvh"],
    &["Lts", "Rts"],
    &["Lw", "Rw"],
    &["Lc", "Rc"],
    &["Cs"],
];

fn check_panned(chans: &[Chan]) -> Result<(), String> {
    let names: Vec<&str> = chans.iter().map(|c| c.name.as_str()).collect();
    let ok = PANNED_EXTRAS.iter().any(|x| {
        let mut want: Vec<&str> = ["L", "R", "C", "LFE", "Ls", "Rs"].to_vec();
        want.extend_from_slice(x);
        want.sort_by_key(|n| rank(n));
        want == names
    });
    if ok {
        return Ok(());
    }
    Err(format!(
        "--layout 8ch={}: the engine renders an Atmos job's 8ch presentation only as 5.1 plus \
         Lrs Rrs, Lvh Rvh, Lts Rts, Lw Rw, Lc Rc or Cs (7.1, 5.1.2, 5.1.2-top, 7.1-wide, \
         7.1-center, 6.1)",
        names.join(",")
    ))
}

fn write_panned(p: &Pres) -> String {
    let e = p.eol;
    let mut s = format!("{}<inputs>{e}", p.head);
    for c in &p.chans {
        s.push_str(&format!("  <channel><use>{}</use></channel>{e}", c.name));
    }
    format!("{s}  </inputs>{e}{}", p.tail)
}

fn write_downmix(p: &Pres, rows: &[(String, Vec<(String, f64)>)]) -> String {
    let e = p.eol;
    let mut s = format!("{}<inputs>{e}", p.head);
    for (out, coefs) in rows {
        s.push_str(&format!("  <channel>{e}  <use>{out}</use>{e}"));
        for (r, c) in coefs {
            s.push_str(&format!(
                "    <coefficient ref=\"{r}\">{c:.6}</coefficient>{e}"
            ));
        }
        s.push_str(&format!("  </channel>{e}"));
    }
    format!("{s}</inputs>{e}{}", p.tail)
}

/// DEE takes <surround-EX> only for a tier whose surrounds are exactly Ls and Rs.
fn fit_surround_ex(head: &str, chans: &[Chan]) -> String {
    const SURROUNDS: [&str; 7] = ["Ls", "Rs", "Cs", "Lrs", "Rrs", "Lsd", "Rsd"];
    let mut s: Vec<&str> = chans
        .iter()
        .map(|c| c.name.as_str())
        .filter(|n| SURROUNDS.contains(n))
        .collect();
    s.sort_unstable();
    if s == ["Ls", "Rs"] {
        return head.to_string();
    }
    head.split_inclusive('\n')
        .filter(|l| !l.trim_start().starts_with("<surround-EX>"))
        .collect()
}

/// Set the presentation's channel modifier (FBA two-channel) to `m`.
fn set_modifier(head: &str, m: &str, eol: &str) -> String {
    match tag_value(head, "channel-modifier") {
        Some(old) => head.replace(
            &format!("<channel-modifier>{old}</channel-modifier>"),
            &format!("<channel-modifier>{m}</channel-modifier>"),
        ),
        None => format!("{head}<channel-modifier>{m}</channel-modifier>{eol}"),
    }
}

/// Apply the DTHD_LAYOUT entries in `spec` to the composed job. `fbb` is set when the job is
/// going to DVD-A, whose mono differs.
pub fn apply(mll: &str, spec: &str, fbb: bool) -> Result<String, String> {
    let first = mll
        .find("<presentation>")
        .ok_or("no <presentation> in the composed .mll")?;
    let rest = super::compose::presentations_end(mll)?;
    let mut ps = presentations(&mll[first..rest])?
        .into_iter()
        .map(parse)
        .collect::<Result<Vec<_>, _>>()?;

    let mut explicit: Vec<(usize, String)> = Vec::new();
    for (tier, s) in dthd_layouts::parse(spec)? {
        let i = match tier {
            Some(t) => ps.iter().position(|p| p.kind == t.mll()).ok_or_else(|| {
                format!(
                    "--layout {}: the job has no {} presentation",
                    t.name(),
                    t.name()
                )
            })?,
            None => ps
                .iter()
                .position(|p| p.source == Source::Own)
                .ok_or("--layout: the job has no independent presentation")?,
        };
        let tier = Tier::ALL
            .into_iter()
            .find(|t| t.mll() == ps[i].kind)
            .ok_or_else(|| format!("--layout: a {} presentation takes no layout", ps[i].kind))?;
        if let Source::Copy(src) = &ps[i].source {
            return Err(format!(
                "--layout {}: this job's {} presentation is a copy of its {}; set the layout \
                 there",
                tier.name(),
                tier.name(),
                src
            ));
        }
        let slots = dthd_layouts::slots(&s, tier.slots())?;
        let mono = tier == Tier::Two && slots == [Some("C"), None];
        let p = &mut ps[i];
        p.changed = true;
        p.mono = mono;
        if p.source == Source::Own {
            if p.chans.len() != slots.len() {
                return Err(format!(
                    "--layout {}: the composed tier has {} channels, not {}",
                    tier.name(),
                    p.chans.len(),
                    slots.len()
                ));
            }
            let files: Vec<Option<String>> = p.chans.iter().map(|c| c.file.clone()).collect();
            p.chans = if mono {
                ["L", "R"]
                    .iter()
                    .map(|n| Chan {
                        name: n.to_string(),
                        file: files[0].clone(),
                    })
                    .collect()
            } else {
                let mut v: Vec<Chan> = slots
                    .iter()
                    .zip(files)
                    .filter_map(|(n, f)| {
                        n.map(|n| Chan {
                            name: n.to_string(),
                            file: f,
                        })
                    })
                    .collect();
                v.sort_by_key(|c| rank(&c.name));
                v
            };
        } else {
            let mut outs: Vec<&str> = if mono {
                vec!["L", "R"]
            } else {
                slots.iter().flatten().copied().collect()
            };
            outs.sort_by_key(|n| rank(n));
            p.chans = outs
                .into_iter()
                .map(|n| Chan {
                    name: n.to_string(),
                    file: None,
                })
                .collect();
            explicit.push((i, s));
        }
    }

    // FBB mono: a six-channel tier with a lone C and no two-channel tier.
    if fbb {
        if let Some(t) = ps
            .iter()
            .position(|p| p.mono && p.source == Source::Own && p.kind == Tier::Two.mll())
        {
            if ps
                .iter()
                .any(|p| p.kind == Tier::Six.mll() && !matches!(p.source, Source::Copy(_)))
            {
                return Err(
                    "--layout 2ch=1.0 with --target dvd-a codes the mono as the six-channel \
                     presentation, and this job has its own 6ch; use --layout 6ch=1.0 there"
                        .into(),
                );
            }
            ps.retain(|p| p.kind != Tier::Six.mll());
            let t = ps
                .iter()
                .position(|p| p.kind == Tier::Two.mll())
                .unwrap_or(t);
            let p = &mut ps[t];
            p.head = p.head.replace(
                &format!("<type>{}</type>", Tier::Two.mll()),
                &format!("<type>{}</type>", Tier::Six.mll()),
            );
            p.kind = Tier::Six.mll().into();
            p.chans.truncate(1);
            p.chans[0].name = "C".into();
            p.mono = false;
        }
        let lone_c = |p: &Pres| {
            p.kind == Tier::Six.mll()
                && p.source == Source::Own
                && p.chans.len() == 1
                && p.chans[0].name == "C"
        };
        if ps.iter().any(lone_c) {
            ps.retain(|p| {
                !(p.kind == Tier::Two.mll() && p.source == Source::Downmix(Tier::Six.mll().into()))
            });
        }
    }

    // Mix every downmix tier again whose source or own layout changed, in job order (the CLI
    // writes a downmix's source before it).
    for i in 0..ps.len() {
        let Source::Downmix(src) = ps[i].source.clone() else {
            continue;
        };
        let j = ps
            .iter()
            .position(|p| p.kind == src)
            .ok_or_else(|| format!("downmix of {src}, which the job lacks"))?;
        if !ps[i].changed && !ps[j].changed {
            continue;
        }
        if j > i {
            return Err(format!("{} mixes {src}, written after it", ps[i].kind));
        }
        if ps[i].panned {
            if ps[i].changed {
                check_panned(&ps[i].chans)?;
                ps[i].raw = write_panned(&ps[i]);
            }
            continue;
        }
        let ins: Vec<String> = match &ps[j].source {
            Source::Copy(c) => ps
                .iter()
                .find(|p| &p.kind == c)
                .map(|p| p.chans.iter().map(|c| c.name.clone()).collect())
                .unwrap_or_default(),
            _ => ps[j].chans.iter().map(|c| c.name.clone()).collect(),
        };
        let has_layout = explicit.iter().any(|(k, _)| *k == i);
        let outs: Vec<String> = ps[i].chans.iter().map(|c| c.name.clone()).collect();
        let out_refs: Vec<&str> = outs.iter().map(String::as_str).collect();
        let gains: Vec<Vec<f64>> = ins.iter().map(|n| fold(n, &out_refs)).collect();
        let mut rows: Vec<(String, Vec<(String, f64)>)> = outs
            .iter()
            .enumerate()
            .map(|(o, name)| {
                (
                    name.clone(),
                    ins.iter()
                        .zip(&gains)
                        .map(|(n, g)| (n.clone(), g[o]))
                        .collect(),
                )
            })
            .collect();
        if ps[i].mono {
            if fbb {
                return Err(
                    "--layout 2ch=1.0: DVD-A has no two-channel mono downmix; give the 2ch \
                     presentation its own mono input, or use --layout 6ch=1.0"
                        .into(),
                );
            }
            let avg: Vec<(String, f64)> = rows[0]
                .1
                .iter()
                .zip(&rows[1].1)
                .map(|((n, a), (_, b))| (n.clone(), (a + b) / 2.0))
                .collect();
            rows = vec![("L".into(), avg.clone()), ("R".into(), avg)];
        } else if !has_layout {
            let two = ps[i].kind == Tier::Two.mll();
            rows.retain(|(n, c)| {
                (two && (n == "L" || n == "R")) || c.iter().any(|(_, v)| *v != 0.0)
            });
        }
        if rows.is_empty() {
            return Err(format!("{} is left with no channel", ps[i].kind));
        }
        if rows.len() > ins.len() {
            let t = Tier::ALL
                .into_iter()
                .find(|t| t.mll() == ps[i].kind)
                .map_or("", Tier::name);
            if t == "2ch" {
                return Err(format!(
                    "the 2ch downmix needs at least two channels in the {src}, which has {}",
                    ins.len()
                ));
            }
            return Err(format!(
                "the {t} downmix would have {} channels from {}, and a downmix cannot have more \
                 than its source; give it a layout with at most {}, e.g. --layout {t}=2.0",
                rows.len(),
                ins.len(),
                ins.len()
            ));
        }
        ps[i].chans = rows
            .iter()
            .map(|(n, _)| Chan {
                name: n.clone(),
                file: None,
            })
            .collect();
        ps[i].changed = true;
        let head = if ps[i].mono {
            set_modifier(&ps[i].head, "Mono", ps[i].eol)
        } else {
            ps[i].head.clone()
        };
        let head = fit_surround_ex(&head, &ps[i].chans);
        let mut p = ps[i].clone();
        p.head = head;
        ps[i].raw = write_downmix(&p, &rows);
    }

    let mut body = String::new();
    for p in &ps {
        if p.changed && p.source == Source::Own {
            let mut p = p.clone();
            if p.mono {
                p.head = set_modifier(&p.head, "Mono", p.eol);
            }
            p.head = fit_surround_ex(&p.head, &p.chans);
            body.push_str(&write_own(&p));
        } else {
            body.push_str(&p.raw);
        }
    }
    Ok(format!("{}{}{}", &mll[..first], body, &mll[rest..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "<encode>\n  <disctypes>\n    <BluRay>true</BluRay>\n  </disctypes>\n";
    const TAIL: &str = "<restart-points>\n  <restart-gap>128</restart-gap>\n</restart-points>\n\
        </encode>\n";

    fn own(kind: &str, names: &[&str], nn: &str) -> String {
        let mut s = format!(
            "<presentation>\n<type>{kind}</type>\n<channel-modifier>Stereo</channel-modifier>\n\
             <apodise>false</apodise>\n  <inputs>\n"
        );
        for (k, n) in names.iter().enumerate() {
            s.push_str(&format!(
                "    <channel><use>{n}</use><file>p_{nn}.0{k}.wav</file></channel>\n"
            ));
        }
        s + "  </inputs>\n</presentation>\n"
    }

    fn dee_downmix(kind: &str, src: &str, ins: &[&str], outs: &[&str]) -> String {
        let mut s = format!(
            "<presentation>\n<type>{kind}</type>\n<downmix-of>{src}</downmix-of>\n<inputs>\n"
        );
        for o in outs {
            s.push_str(&format!("  <channel>\n  <use>{o}</use>\n"));
            for (i, g) in ins.iter().zip(fold_dee(ins, outs, o)) {
                s.push_str(&format!(
                    "    <coefficient ref=\"{i}\">{g:.6}</coefficient>\n"
                ));
            }
            s.push_str("  </channel>\n");
        }
        s + "</inputs>\n</presentation>\n"
    }

    /// DEE's coefficients as captured from its 7.1 job (8ch to 6ch and 6ch to 2ch).
    fn fold_dee(ins: &[&str], outs: &[&str], out: &str) -> Vec<f64> {
        ins.iter()
            .map(|i| match (out, *i) {
                (o, i) if o == i => 1.0,
                (_, i) if outs.contains(&i) => 0.0,
                ("Ls", "Lrs") | ("Rs", "Rrs") => 1.0,
                ("L", "Ls") | ("R", "Rs") => 1.0,
                ("L", "C") | ("R", "C") => MINUS_3DB,
                _ => 0.0,
            })
            .collect()
    }

    const S6: [&str; 6] = ["L", "R", "C", "LFE", "Ls", "Rs"];
    const S8: [&str; 8] = ["L", "R", "C", "LFE", "Ls", "Rs", "Lrs", "Rrs"];

    fn job71() -> String {
        format!(
            "{HEAD}{}{}{}{TAIL}",
            own("eight-channel", &S8, "08"),
            dee_downmix("six-channel", "eight-channel", &S8, &S6),
            dee_downmix("two-channel", "six-channel", &S6, &["L", "R"])
        )
    }

    #[test]
    fn the_fold_reproduces_dee() {
        let job = job71();
        let ps: Vec<Pres> = presentations(&job[job.find("<presentation>").unwrap()..])
            .unwrap()
            .into_iter()
            .map(|b| parse(b).unwrap())
            .collect();
        assert_eq!(ps.len(), 3);
        for o in S6 {
            let g: Vec<f64> = S8
                .iter()
                .map(|i| fold(i, &S6)[S6.iter().position(|x| *x == o).unwrap()])
                .collect();
            assert_eq!(g, fold_dee(&S8, &S6, o), "{o}");
        }
        // 7.1 on DEE's own job rewrites it to the same text.
        assert_eq!(apply(&job, "8ch=7.1", false).unwrap(), job);
    }

    /// DEE's Atmos job: a 16-element tier, an 8ch tier panned from it, then 6ch and 2ch.
    fn job_atmos() -> String {
        let mut s16 = String::from(
            "<presentation>\n<type>sixteen-channel</type>\n  <inputs>\n    \
             <channel><use>LFE</use><file>dthdsc.00.wav</file></channel>\n",
        );
        for k in 1..12 {
            s16.push_str(&format!(
                "    <channel><use>Obj</use><file>dthdsc.{k:02}.wav</file></channel>\n"
            ));
        }
        s16.push_str("  </inputs>\n</presentation>\n");
        let mut s8 = String::from(
            "<presentation>\n<type>eight-channel</type>\n<downmix-of>sixteen-channel</downmix-of>\n\
             <matrices-from-file-name>matrix_info_mx.mx</matrices-from-file-name>\n<inputs>\n",
        );
        for n in S8 {
            s8.push_str(&format!("  <channel><use>{n}</use></channel>\n"));
        }
        s8.push_str("  </inputs>\n</presentation>\n");
        format!(
            "{HEAD}{s16}{s8}{}{}{TAIL}",
            dee_downmix("six-channel", "eight-channel", &S8, &S6),
            dee_downmix("two-channel", "six-channel", &S6, &["L", "R"])
        )
    }

    #[test]
    fn atmos_8ch_is_renamed_not_mixed() {
        let out = apply(&job_atmos(), "8ch=5.1.2", false).unwrap();
        let eight =
            &out[out.find("<type>eight-channel").unwrap()..out.find("<type>six-channel").unwrap()];
        assert!(eight.contains("<channel><use>Lvh</use></channel>"));
        assert!(eight.contains("<matrices-from-file-name>"));
        assert!(!eight.contains("coefficient"));
        // The 6ch downmix folds the heights into the front pair.
        let six = &out[out.find("<type>six-channel").unwrap()..];
        let l = &six[six.find("<use>L</use>").unwrap()..];
        assert!(l.contains("<coefficient ref=\"Lvh\">1.000000</coefficient>"));
        assert!(apply(&job_atmos(), "8ch=L,R,C,LFE,Ls,Rs,Lvh,Lts", false).is_err());
        assert!(apply(&job_atmos(), "8ch=7.0", false).is_err());
    }

    #[test]
    fn a_job_without_restart_points() {
        // Atmos with an independent 8ch: the CLI writes <extra-data> and no <restart-points>.
        let job = job71().replace(TAIL, "<extra-data>\n</extra-data>\n</encode>\n");
        let out = apply(&job, "6ch=2.0", false).unwrap();
        assert!(out.ends_with("<extra-data>\n</extra-data>\n</encode>\n"));
        let six = &out[out.find("<type>six-channel").unwrap()..];
        assert!(!six[..six.find("</presentation>").unwrap()].contains("<use>C</use>"));
    }

    #[test]
    fn wide_relabels_slots_and_refolds() {
        let out = apply(&job71(), "8ch=7.1-wide", false).unwrap();
        assert!(out.contains("<use>Lw</use><file>p_08.06.wav</file>"));
        assert!(!out.contains("Lrs"));
        // Lw folds into L in the six-channel tier.
        let six = &out[out.find("<type>six-channel").unwrap()..];
        let l = &six[six.find("<use>L</use>").unwrap()..];
        assert!(l.contains("<coefficient ref=\"Lw\">1.000000</coefficient>"));
    }

    #[test]
    fn dropped_lfe_drops_the_downmix_lfe() {
        let out = apply(&job71(), "8ch=7.0", false).unwrap();
        let six =
            &out[out.find("<type>six-channel").unwrap()..out.find("<type>two-channel").unwrap()];
        assert!(!six.contains("<use>LFE</use>"));
        assert!(six.contains("<use>Ls</use>"));
    }

    #[test]
    fn back_centre_is_minus_3db_in_each_surround() {
        let out = apply(&job71(), "8ch=6.1", false).unwrap();
        assert!(out.contains("<use>Cs</use><file>p_08.06.wav</file>"));
        assert!(!out.contains("p_08.07.wav"));
        let six = &out[out.find("<type>six-channel").unwrap()..];
        let ls = &six[six.find("<use>Ls</use>").unwrap()..];
        assert!(ls.contains("<coefficient ref=\"Cs\">0.707946</coefficient>"));
    }

    #[test]
    fn fba_mono() {
        let job = format!(
            "{HEAD}<presentation>\n<type>eight-channel</type>\n<copy-of>two-channel</copy-of>\n\
             </presentation>\n{}{TAIL}",
            own("two-channel", &["L", "R"], "02")
        );
        let out = apply(&job, "1.0", false).unwrap();
        assert!(out.contains("<channel-modifier>Mono</channel-modifier>"));
        assert!(out.contains("<use>L</use><file>p_02.00.wav</file>"));
        assert!(out.contains("<use>R</use><file>p_02.00.wav</file>"));
        assert!(out.contains("<copy-of>two-channel</copy-of>"));
    }

    #[test]
    fn fba_mono_downmix_averages() {
        let out = apply(&job71(), "2ch=1.0", false).unwrap();
        let two = &out[out.find("<type>two-channel").unwrap()..];
        assert!(two.contains("<channel-modifier>Mono</channel-modifier>"));
        assert!(two.contains("<coefficient ref=\"L\">0.500000</coefficient>"));
        assert!(two.contains("<coefficient ref=\"C\">0.707946</coefficient>"));
    }

    #[test]
    fn fbb_mono_is_a_lone_c() {
        let job = format!(
            "{HEAD}<presentation>\n<type>eight-channel</type>\n<copy-of>two-channel</copy-of>\n\
             </presentation>\n<presentation>\n<type>six-channel</type>\n\
             <copy-of>two-channel</copy-of>\n</presentation>\n{}{TAIL}",
            own("two-channel", &["L", "R"], "02")
        );
        let out = apply(&job, "2ch=1.0", true).unwrap();
        assert!(!out.contains("<type>two-channel</type>"));
        let six = &out[out.find("<type>six-channel</type>").unwrap()..];
        assert!(six.contains("<use>C</use><file>p_02.00.wav</file>"));
        assert!(!six.contains("copy-of"));
        let job6 = format!(
            "{HEAD}{}{}{TAIL}",
            own("six-channel", &S6, "06"),
            dee_downmix("two-channel", "six-channel", &S6, &["L", "R"])
        );
        let out = apply(&job6, "6ch=1.0", true).unwrap();
        assert!(!out.contains("two-channel"));
        assert!(out.contains("<use>C</use><file>p_06.02.wav</file>"));
        assert!(apply(&job71(), "2ch=1.0", true).is_err());
    }

    #[test]
    fn surround_ex_only_with_ls_rs() {
        let six = own("six-channel", &S6, "06")
            .replace("<apodise>", "<surround-EX>false</surround-EX>\n<apodise>");
        let job = format!("{HEAD}{six}{TAIL}");
        assert!(apply(&job, "5.0", false).unwrap().contains("surround-EX"));
        assert!(!apply(&job, "3.1.2", false).unwrap().contains("surround-EX"));
    }

    #[test]
    fn refusals() {
        assert!(apply(&job71(), "6ch=5.1.2", false).is_err());
        assert!(apply(&job71(), "8ch=L,R,-,-,-,-,Cs", false)
            .unwrap_err()
            .contains("more"));
        assert!(apply(&job71(), "8ch=L,R,-,-,-,-,Cs;6ch=2.0", false).is_ok());
        let job = format!(
            "{HEAD}<presentation>\n<type>eight-channel</type>\n<copy-of>two-channel</copy-of>\n\
             </presentation>\n{}{TAIL}",
            own("two-channel", &["L", "R"], "02")
        );
        assert!(apply(&job, "8ch=7.1", false).unwrap_err().contains("copy"));
        assert!(apply(&job, "6ch=5.1", false)
            .unwrap_err()
            .contains("no 6ch"));
    }
}
