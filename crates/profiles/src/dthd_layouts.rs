//! Channel layouts for a dee_dthd_encoder presentation (`dthd --layout`).
//!
//! The CLI reads a presentation's input positionally: WAV channel k, or wav_list entry k, is
//! slot k, whatever the WAV's channel mask says. It names the slots itself (2ch L R; 6ch L R C
//! LFE Ls Rs; 8ch the same plus Lrs Rrs). A layout renames the slots of one presentation, slot
//! by slot, and `-` leaves a slot out. Names follow ddp's where both have the layout; the
//! eight-slot extras sit in slots 7 and 8. A comma list of slot labels gives any other
//! combination, and the engine judges it against the disc type.

/// The engine's channel names (`<use>` tokens), in the order an `<inputs>` block lists them.
pub const TOKENS: [&str; 25] = [
    "L", "R", "C", "LFE", "Ls", "Rs", "Lvh", "Rvh", "Lc", "Rc", "Lrs", "Rrs", "Cs", "Ts", "Lsd",
    "Rsd", "Lw", "Rw", "Cvh", "LFE2", "Lts", "Rts", "Lrh", "Rrh", "Obj",
];

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub name: &'static str,
    /// One label per slot, `-` for a slot left out.
    pub slots: &'static str,
    pub note: &'static str,
}

pub static LAYOUTS: &[Layout] = &[
    Layout {
        name: "1.0",
        slots: "- - C",
        note: "in 2ch slot 1, coded as Mono (DVD-A: a lone C)",
    },
    Layout {
        name: "2.0",
        slots: "L R",
        note: "",
    },
    Layout {
        name: "2.1",
        slots: "L R - LFE",
        note: "",
    },
    Layout {
        name: "3.0",
        slots: "L R C",
        note: "",
    },
    Layout {
        name: "3.1",
        slots: "L R C LFE",
        note: "",
    },
    Layout {
        name: "3.0-back",
        slots: "L R - - Cs",
        note: "DVD-A, or an 8ch presentation",
    },
    Layout {
        name: "3.1-back",
        slots: "L R - LFE Cs",
        note: "DVD-A, or an 8ch presentation",
    },
    Layout {
        name: "4.0",
        slots: "L R C - Cs",
        note: "DVD-A, or an 8ch presentation",
    },
    Layout {
        name: "4.1",
        slots: "L R C LFE Cs",
        note: "DVD-A, or an 8ch presentation",
    },
    Layout {
        name: "4.0-quad",
        slots: "L R - - Ls Rs",
        note: "",
    },
    Layout {
        name: "4.1-quad",
        slots: "L R - LFE Ls Rs",
        note: "",
    },
    Layout {
        name: "5.0",
        slots: "L R C - Ls Rs",
        note: "",
    },
    Layout {
        name: "5.1",
        slots: "L R C LFE Ls Rs",
        note: "",
    },
    Layout {
        name: "3.1.2",
        slots: "L R C LFE Lvh Rvh",
        note: "front heights in slots 5 and 6",
    },
    Layout {
        name: "4.0.2",
        slots: "L R - - Ls Rs Lvh Rvh",
        note: "",
    },
    Layout {
        name: "5.0.2",
        slots: "L R C - Ls Rs Lvh Rvh",
        note: "",
    },
    Layout {
        name: "5.1.2",
        slots: "L R C LFE Ls Rs Lvh Rvh",
        note: "",
    },
    Layout {
        name: "5.0.2-top",
        slots: "L R C - Ls Rs Lts Rts",
        note: "",
    },
    Layout {
        name: "5.1.2-top",
        slots: "L R C LFE Ls Rs Lts Rts",
        note: "",
    },
    Layout {
        name: "6.0",
        slots: "L R C - Ls Rs Cs -",
        note: "",
    },
    Layout {
        name: "6.1",
        slots: "L R C LFE Ls Rs Cs -",
        note: "",
    },
    Layout {
        name: "7.0",
        slots: "L R C - Ls Rs Lrs Rrs",
        note: "",
    },
    Layout {
        name: "7.1",
        slots: "L R C LFE Ls Rs Lrs Rrs",
        note: "DEE's 8ch",
    },
    Layout {
        name: "7.1-wide",
        slots: "L R C LFE Ls Rs Lw Rw",
        note: "",
    },
    Layout {
        name: "7.1-center",
        slots: "L R C LFE Ls Rs Lc Rc",
        note: "front inner pair",
    },
];

/// A presentation the CLI composes: its option name, its .mll type and its slot count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Two,
    Six,
    Eight,
}

impl Tier {
    pub const ALL: [Tier; 3] = [Tier::Two, Tier::Six, Tier::Eight];

    pub fn parse(s: &str) -> Option<Tier> {
        Tier::ALL.into_iter().find(|t| t.name() == s)
    }

    pub fn name(self) -> &'static str {
        match self {
            Tier::Two => "2ch",
            Tier::Six => "6ch",
            Tier::Eight => "8ch",
        }
    }

    pub fn mll(self) -> &'static str {
        match self {
            Tier::Two => "two-channel",
            Tier::Six => "six-channel",
            Tier::Eight => "eight-channel",
        }
    }

    pub fn slots(self) -> usize {
        match self {
            Tier::Two => 2,
            Tier::Six => 6,
            Tier::Eight => 8,
        }
    }
}

pub fn find(name: &str) -> Option<&'static Layout> {
    LAYOUTS.iter().find(|l| l.name == name)
}

/// The slot labels of `spec` (a layout name or a comma list of labels) for a presentation of
/// `slots` slots, padded with empty slots. In two slots, 1.0 is slot 1.
pub fn slots(spec: &str, slots: usize) -> Result<Vec<Option<&str>>, String> {
    let labels: Vec<&str> = match find(spec) {
        Some(_) if spec == "1.0" && slots == 2 => vec!["C"],
        Some(l) => l.slots.split_whitespace().collect(),
        None if spec.contains(',') || TOKENS.contains(&spec) => {
            spec.split(',').map(str::trim).collect()
        }
        None => {
            return Err(format!(
                "layout {spec:?} unknown; see `dee_shim dthd_encoder --list`"
            ))
        }
    };
    if labels.len() > slots {
        return Err(format!(
            "layout {spec:?} has {} slots, the presentation {slots}",
            labels.len()
        ));
    }
    let mut out = Vec::with_capacity(slots);
    for l in labels {
        if l == "-" {
            out.push(None);
        } else if let Some(t) = TOKENS.iter().find(|t| **t == l) {
            if out.contains(&Some(*t)) {
                return Err(format!("layout {spec:?} names {l} twice"));
            }
            out.push(Some(*t));
        } else {
            return Err(format!(
                "channel {l:?} unknown in layout {spec:?} ({})",
                TOKENS.join(" ")
            ));
        }
    }
    out.resize(slots, None);
    if out.iter().all(Option::is_none) {
        return Err(format!("layout {spec:?} has no channel"));
    }
    Ok(out)
}

/// A DTHD_LAYOUT value: comma-free entries joined by `;`, each `[TIER=]SPEC`. Without a tier an
/// entry is for the job's top independent presentation. At most one entry per tier.
pub fn parse(text: &str) -> Result<Vec<(Option<Tier>, String)>, String> {
    let mut out: Vec<(Option<Tier>, String)> = Vec::new();
    for entry in text.split(';').filter(|e| !e.is_empty()) {
        let (tier, spec) = match entry.split_once('=') {
            Some((t, s)) => (
                Some(
                    Tier::parse(t)
                        .ok_or_else(|| format!("presentation {t:?} unknown (2ch, 6ch or 8ch)"))?,
                ),
                s,
            ),
            None => (None, entry),
        };
        if out.iter().any(|(t, _)| *t == tier) {
            return Err(match tier {
                Some(t) => format!("two layouts for {}", t.name()),
                None => "two layouts without a presentation".to_string(),
            });
        }
        let check = tier.map_or(8, Tier::slots);
        slots(spec, check)?;
        out.push((tier, spec.to_string()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_layout_parses() {
        for l in LAYOUTS {
            let n = l.slots.split_whitespace().count();
            assert!(n <= 8, "{}", l.name);
            slots(l.name, 8).unwrap();
        }
    }

    #[test]
    fn mono_takes_the_first_of_two_slots() {
        assert_eq!(slots("1.0", 2).unwrap(), vec![Some("C"), None]);
        assert_eq!(
            slots("1.0", 6).unwrap(),
            vec![None, None, Some("C"), None, None, None]
        );
    }

    #[test]
    fn lists_and_refusals() {
        assert_eq!(
            slots("L,R,-,-,Cs", 6).unwrap(),
            vec![Some("L"), Some("R"), None, None, Some("Cs"), None]
        );
        assert!(slots("5.1.2", 6).is_err());
        assert!(slots("L,L", 2).is_err());
        assert!(slots("L,Q", 2).is_err());
        assert!(slots("-,-", 2).is_err());
        assert!(slots("9.1", 8).is_err());
    }

    #[test]
    fn entries() {
        let e = parse("8ch=7.1-wide;2ch=1.0").unwrap();
        assert_eq!(e[0], (Some(Tier::Eight), "7.1-wide".to_string()));
        assert_eq!(e[1], (Some(Tier::Two), "1.0".to_string()));
        assert!(parse("6ch=5.1;6ch=5.0").is_err());
        assert!(parse("4ch=5.1").is_err());
        assert!(parse("6ch=5.1.2").is_err());
    }
}
