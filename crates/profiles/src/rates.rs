//! Data rates each encoder mode accepts, per input channel count, as the binary enforces them.
//!
//! The binary's own help (`--morehelp data-rate`) covers only the public modes (dd, ddp) and is
//! wrong in one place: it lists 272 for dd, which the encoder rejects and which appears in none
//! of dd's lists in the code. The hidden parents (bluray, bluray_secondary) are undocumented.
//!
//! Two independent sources, which agree exactly:
//!
//! - **Execution.** A probe encoded every multiple of 8 from 8 to 1664 plus a tail up to 6144,
//!   for every mode and for 1, 2, 6 and 8 channel WAV input, and parsed the coded rate out of
//!   each accepted stream's syncframes. Every accepted rate coded exactly what was asked (for
//!   bluray, as the sum of the AC-3 core and the E-AC-3 dependent substream). For bluray every
//!   integer 1..2000 was also tried; only the five below pass, and the parser's ceiling is 1664
//!   (1665 is "Invalid option value"; 1024 for the other modes). The result, for DEE 6.7, is
//!   testsets/rates/dee67.tsv, which the tests below check the tables against.
//! - **Code.** The rate check tests membership in a list chosen from a static map of codec id
//!   to list builder: 0 dd, 1 ddp (separate mono and stereo tables, and it honours
//!   allow_hybrid_downmix), 2 bluray (the five values are instruction immediates, so no byte
//!   search finds them as an array), 3 bluray_secondary.
//!
//! The check is on the INPUT channel count and the mode, before any lever runs, so a shimmed
//! layout (say 2.1 from a 6-channel input) is validated against the 6-channel row. The bluray
//! core/dependent split also depends on the layout: native 5.1 at 768 codes 448+320, the shimmed
//! 6.1 codes 512+256; only the total is fixed by `--data-rate`.

#[derive(Clone, Copy, Debug)]
pub struct RateRow {
    /// `-e` value. A row with sub-options (`ddp:allow_hybrid_downmix=1`) applies when the `-e`
    /// value carries all of them; a bare mode row applies to every `-e` value of that mode.
    pub mode: &'static str,
    /// Input channel counts this row covers.
    pub channels: &'static [u8],
    /// Accepted `--data-rate` values, kbps.
    pub kbps: &'static [u16],
    /// What `auto` (no `--data-rate`) gives, kbps.
    pub default: u16,
    /// Shape of the output and anything surprising.
    pub note: &'static str,
}

const DD_ALL: &[u16] = &[
    96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 576, 640,
];
const DD_51: &[u16] = &[224, 256, 320, 384, 448, 512, 576, 640];
const DDP_TAIL: &[u16] = &[
    192, 200, 208, 216, 224, 232, 240, 248, 256, 272, 288, 304, 320, 336, 352, 368, 384, 400, 448,
    512, 576, 640, 704, 768, 832, 896, 960, 1008, 1024,
];
const DDP_HYBRID: &[u16] = &[
    64, 96, 128, 160, 192, 200, 208, 216, 224, 232, 240, 248, 256, 272, 288, 304, 320, 336, 352,
    368, 384, 400, 448, 512, 576, 640, 704, 768, 832, 896, 960, 1008, 1024,
];
const DDP_1: &[u16] = &[
    32, 40, 48, 56, 64, 72, 80, 88, 96, 104, 112, 120, 128, 144, 160, 176, 192, 200, 208, 216, 224,
    232, 240, 248, 256, 272, 288, 304, 320, 336, 352, 368, 384, 400, 448, 512, 576, 640, 704, 768,
    832, 896, 960, 1008, 1024,
];
const DDP_2: &[u16] = &[
    96, 104, 112, 120, 128, 144, 160, 176, 192, 200, 208, 216, 224, 232, 240, 248, 256, 272, 288,
    304, 320, 336, 352, 368, 384, 400, 448, 512, 576, 640, 704, 768, 832, 896, 960, 1008, 1024,
];

/// Native frontend rates; defaults are from DEE 6.7.
pub static DDP: &[RateRow] = &[
    RateRow { mode: "dd", channels: &[1], kbps: DD_ALL, default: 96, note: "AC-3; help also lists 272, which is rejected" },
    RateRow { mode: "dd", channels: &[2], kbps: DD_ALL, default: 128, note: "AC-3" },
    RateRow { mode: "dd", channels: &[6, 8], kbps: DD_51, default: 224, note: "AC-3" },
    RateRow { mode: "ddp", channels: &[1], kbps: DDP_1, default: 64, note: "E-AC-3, one independent substream" },
    RateRow { mode: "ddp", channels: &[2], kbps: DDP_2, default: 128, note: "E-AC-3" },
    RateRow { mode: "ddp", channels: &[6, 8], kbps: DDP_TAIL, default: 192, note: "E-AC-3; 8-ch input codes 5.1 without the shim" },
    RateRow { mode: "ddp:allow_hybrid_downmix=1", channels: &[6, 8], kbps: DDP_HYBRID, default: 192,
              note: "hybrid downmix adds 64 96 128 160 to the 5.1 list (the help says: energy in C, LR, LRC, all)" },
    RateRow { mode: "bluray", channels: &[6, 8], kbps: &[768, 1024, 1280, 1536, 1664], default: 768,
              note: "the total of AC-3 core plus E-AC-3 dependent: 768=448+320 1024=576+448 1280=640+640 1536=640+896 1664=640+1024; rejects 1 and 2 ch input" },
    RateRow { mode: "bluray_secondary", channels: &[1], kbps: &[32, 64, 96, 128, 160, 192, 224, 256], default: 192, note: "E-AC-3, steps of 32" },
    RateRow { mode: "bluray_secondary", channels: &[2], kbps: &[96, 128, 160, 192, 224, 256], default: 192, note: "E-AC-3" },
    RateRow { mode: "bluray_secondary", channels: &[6, 8], kbps: &[160, 192, 224, 256], default: 192, note: "E-AC-3" },
];

/// AC-3 rates supported by the 5.1 core in an extended layout, at 48 kHz.
pub const AC3_CORE: &[u16] = &[224, 256, 320, 384, 448, 512, 576, 640];

pub const JOC_BLURAY: &[u16] = &[1152, 1280, 1408, 1512, 1536, 1664];

/// Default Blu-ray JOC allocation without the AC-3 rate ladder.
pub fn joc_eac3_split(total: u32) -> Result<(u32, u32), &'static str> {
    if !(64..=2048).contains(&total) {
        return Err("an E-AC-3 total must be 64..2048 kbps");
    }

    // The native default reserves ceil(68500/1000) kbps plus allowances of 6 and 17.
    let available = total
        .checked_sub(69 + 6 + 17)
        .ok_or("total is too small for the JOC reservation")?;
    let weight = if total <= 640 { 19661 } else { 18350 };
    let target = (available * weight + 16384) >> 15;
    let first = target.min(1024).max(total.saturating_sub(1024));
    let dependent = total - first;
    if first < 32 || dependent < 32 {
        return Err("total leaves less than 32 kbps for a substream after the JOC reservation");
    }
    Ok((first, dependent))
}

pub static DDPJOC: &[RateRow] = &[
    RateRow {
        mode: "ddpjoc",
        channels: &[],
        kbps: &[384, 448, 576, 640, 768, 1024],
        default: 448,
        note: "E-AC-3 independent with JOC; streaming",
    },
    RateRow {
        mode: "bluray",
        channels: &[],
        kbps: JOC_BLURAY,
        default: 1152,
        note: "AC-3 core + E-AC-3 dependent with JOC",
    },
];

const DDP_EXTENDED: &[u16] = &[
    384, 448, 504, 576, 640, 704, 768, 832, 896, 960, 1008, 1024, 1152, 1280, 1408, 1512, 1536,
    1664, 1792,
];

/// Coded totals for a plain DD+ E-AC-3-core layout, rather than frontend input rates.
pub fn layout_rates(tool: &str, layout: &str) -> Option<&'static [u16]> {
    if tool != "dee_ddp_encoder" {
        return None;
    }
    let recipe = crate::recipes::find(tool, layout)?;
    let base = recipe.name.strip_suffix("/eac3")?;
    Some(if matches!(base, "6.0" | "6.1") {
        &DDP_EXTENDED[..17]
    } else {
        DDP_EXTENDED
    })
}

pub fn contains(values: &[u16], rate: u32) -> bool {
    values.iter().any(|&value| u32::from(value) == rate)
}

/// Rows for a tool, empty if not probed.
pub fn for_tool(tool: &str) -> &'static [RateRow] {
    match tool {
        "dee_ddp_encoder" => DDP,
        "dee_ddpjoc_encoder" => DDPJOC,
        _ => &[],
    }
}

/// The mode part of an `-e` value (`ddp:lfe_on=0` -> `ddp`).
pub fn base_mode(mode: &str) -> &str {
    mode.split(':').next().unwrap_or(mode)
}

/// Whether a row applies to an `-e` value: same mode, and every sub-option the row names is set.
pub fn row_applies(row: &RateRow, mode: &str) -> bool {
    let given: Vec<&str> = mode.split(':').skip(1).collect();
    base_mode(row.mode) == base_mode(mode)
        && row.mode.split(':').skip(1).all(|o| given.contains(&o))
}

/// The row that governs an `-e` value at one input channel count: among the rows that apply and
/// cover that count, the one naming the most sub-options.
pub fn row_for(tool: &str, mode: &str, channels: u8) -> Option<&'static RateRow> {
    for_tool(tool)
        .iter()
        .filter(|r| {
            row_applies(r, mode) && (r.channels.is_empty() || r.channels.contains(&channels))
        })
        .max_by_key(|r| r.mode.split(':').count())
}

/// Every rate the `-e` value accepts for at least one input channel count, sorted, deduplicated.
pub fn union_for_mode(tool: &str, mode: &str) -> Vec<u16> {
    let mut v: Vec<u16> = for_tool(tool)
        .iter()
        .filter(|r| row_applies(r, mode))
        .flat_map(|r| r.kbps.iter().copied())
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// The DD+ rate ladder: every E-AC-3 list in `DDP` is a run of it, after at most a few values
/// of its own.
const LADDER: &[u16] = DDP_1;

/// A rate list in short form: values off the ladder as they are, then a run of eight or more
/// ladder values as `first..last on the ladder`.
fn short(values: &[u16]) -> Vec<String> {
    let run = (0..values.len()).find(|&i| {
        let tail = &values[i..];
        tail.len() >= 8 && LADDER.windows(tail.len()).any(|w| w == tail)
    });
    let (own, tail) = values.split_at(run.unwrap_or(values.len()));
    let mut words: Vec<String> = own.iter().map(u16::to_string).collect();
    if let (Some(first), Some(last)) = (tail.first(), tail.last()) {
        if let Some(word) = words.last_mut() {
            word.push(',');
        }
        words.extend([
            format!("{first}..{last}"),
            "on".into(),
            "the".into(),
            "ladder".into(),
        ]);
    }
    words
}

/// The accepted data rates of a tool, one row per mode and input, for `--list`.
pub fn render(tool: &str, indent: usize, width: usize) -> String {
    let rows = for_tool(tool);
    if rows.is_empty() {
        return String::new();
    }
    let pad = " ".repeat(indent);
    // Rows that differ only in input channels and default share a line.
    let mut lines: Vec<(String, String, String, Vec<String>)> = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let r = &rows[i];
        let mut same = vec![r];
        while let Some(next) = rows.get(i + same.len()) {
            if next.mode != r.mode || next.kbps != r.kbps {
                break;
            }
            same.push(next);
        }
        i += same.len();
        let channels: Vec<String> = same
            .iter()
            .flat_map(|r| r.channels.iter().map(u8::to_string))
            .collect();
        let input = if channels.is_empty() {
            "Atmos".into()
        } else {
            format!("{} ch", channels.join("/"))
        };
        let mut defaults: Vec<String> = same.iter().map(|r| r.default.to_string()).collect();
        defaults.dedup();
        let mode = if r.mode.contains("allow_hybrid_downmix=1") {
            "ddp hybrid"
        } else {
            r.mode
        };
        lines.push((mode.into(), input, defaults.join("/"), short(r.kbps)));
    }
    let extended = |text: &str| {
        text.split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>()
    };
    if tool == "dee_ddp_encoder" {
        let mut words = short(DDP_EXTENDED);
        words.extend(extended("(6.0 and 6.1 stop at 1536)"));
        lines.push(("/eac3".into(), "8 ch".into(), "768".into(), words));
    } else {
        lines.push((
            "/eac3".into(),
            "Atmos".into(),
            "1152".into(),
            extended("any whole kbps 64..2048"),
        ));
    }

    let mode_w = lines.iter().map(|l| l.0.len()).max().unwrap_or(0);
    let input_w = lines.iter().map(|l| l.1.len()).max().unwrap_or(0).max(5);
    let lead = format!("{pad}{:mode_w$}  {:input_w$}  {:>7}  ", "", "", "");
    let mut out = "data rates (kbps, DEE 6.7):\n".to_string();
    if tool == "dee_ddp_encoder" {
        let ladder: Vec<String> = LADDER.iter().map(u16::to_string).collect();
        let head = format!("{pad}ladder ");
        out += &wrap(&ladder, &" ".repeat(head.len()), width).replacen(
            &" ".repeat(head.len()),
            &head,
            1,
        );
    }
    out += &format!(
        "{pad}{:mode_w$}  {:input_w$}  {:>7}  rates\n",
        "mode", "input", "default"
    );
    for (mode, input, default, words) in &lines {
        let first = format!("{pad}{mode:mode_w$}  {input:input_w$}  {default:>7}  ");
        out += &wrap(words, &lead, width).replacen(&lead, &first, 1);
    }
    let mut notes = vec![
        "R0:R1 (extended layouts): R0 an AC-3 core rate (224 256 320 384 448 512 576 640) or an \
         E-AC-3 rate 32..1024, R1 32..1024; the total is up to 1664 with an AC-3 core, 2048 with \
         E-AC-3.",
    ];
    if tool == "dee_ddp_encoder" {
        notes.push(
            "ddp hybrid is -e ddp:allow_hybrid_downmix=1. A rate is checked against the \
                    input channel count, before any layout change.",
        );
    } else {
        notes.push(
            "/eac3 splits a total by weighted allocation after 92 kbps for JOC and framing; \
                    each substream caps at 1024.",
        );
    }
    out.push('\n');
    for note in notes {
        let words: Vec<String> = note.split_whitespace().map(String::from).collect();
        out += &wrap(&words, &pad, width);
    }
    out
}

/// Words joined by spaces, broken into lines no wider than `width`, each starting with `lead`.
fn wrap(words: &[String], lead: &str, width: usize) -> String {
    let mut out = String::new();
    let mut line = lead.to_string();
    for w in words {
        if line.len() > lead.len() && line.len() + 1 + w.len() > width {
            out += &line;
            out += "\n";
            line = lead.to_string();
        }
        if line.len() > lead.len() {
            line.push(' ');
        }
        line += w;
    }
    out += &line;
    out += "\n";
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joc_eac3_allocation() {
        for (total, first, dependent) in [
            (768, 379, 389),
            (1024, 522, 502),
            (1152, 594, 558),
            (1280, 665, 615),
            (1408, 737, 671),
            (1512, 795, 717),
            (1536, 809, 727),
            (1664, 880, 784),
            (1665, 881, 784),
            (1792, 952, 840),
            (2048, 1024, 1024),
        ] {
            assert_eq!(joc_eac3_split(total), Ok((first, dependent)));
        }
        for total in [0, 64, 92, 2049, u32::MAX] {
            assert!(joc_eac3_split(total).is_err());
        }
        for total in 704..=2048 {
            let (first, dependent) = joc_eac3_split(total).unwrap();
            assert_eq!(first + dependent, total);
            assert!((32..=1024).contains(&first) && (32..=1024).contains(&dependent));
            if total > 704 {
                let previous = joc_eac3_split(total - 1).unwrap().0;
                assert!((previous..=previous + 1).contains(&first));
            }
        }
    }

    #[test]
    fn rates_match_dee67_probe() {
        let tsv = include_str!("../../../testsets/rates/dee67.tsv");
        for line in tsv.lines().skip(1) {
            let fields: Vec<_> = line.split('\t').collect();
            let expected: Vec<u16> = fields[4]
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.parse().unwrap())
                .collect();
            if fields[0] == "layout" {
                assert_eq!(
                    layout_rates(fields[1], fields[2]),
                    Some(expected.as_slice()),
                    "{}",
                    fields[2]
                );
            } else {
                let row = row_for(fields[1], fields[2], fields[3].parse().unwrap());
                assert_eq!(
                    row.map(|r| r.kbps).unwrap_or(&[]),
                    expected,
                    "{} {}",
                    fields[2],
                    fields[3]
                );
                assert_eq!(
                    row.map(|r| r.default).unwrap_or(0),
                    fields[5].parse::<u16>().unwrap()
                );
            }
        }
        assert!(!contains(DDP_EXTENDED, 65536 + 384));
    }

    #[test]
    fn sub_option_rows() {
        let plain = union_for_mode("dee_ddp_encoder", "ddp:lfe_on=0");
        let hybrid = union_for_mode("dee_ddp_encoder", "ddp:lfe_on=0:allow_hybrid_downmix=1");
        assert_eq!(plain, union_for_mode("dee_ddp_encoder", "ddp"));
        assert!(plain.contains(&64)); // mono ddp row
        assert_eq!(row_for("dee_ddp_encoder", "ddp", 6).unwrap().kbps[0], 192);
        assert_eq!(
            row_for("dee_ddp_encoder", "ddp:allow_hybrid_downmix=1", 6)
                .unwrap()
                .kbps[0],
            64
        );
        assert_eq!(
            row_for("dee_ddp_encoder", "ddp:allow_hybrid_downmix=1", 2)
                .unwrap()
                .kbps,
            DDP_2
        );
        assert!(hybrid.len() >= plain.len());
    }
}
