//! Named layouts and their lever settings. `mode` selects the native `-e` pipeline; it does not
//! necessarily name the output codec. `--mode` overrides it, and `--set` overrides recipe
//! settings. `--list` prints each layout's WAV input slots.

#[derive(Clone, Copy, Debug)]
pub struct Recipe {
    /// What the operator types, e.g. "6.1".
    pub name: &'static str,
    /// Executable this applies to.
    pub tool: &'static str,
    /// Coded backbone channel set; not a decoder output-order contract.
    pub channels: &'static str,
    /// The input slots in order, one label per slot and `-` for an empty slot. Empty for a tool
    /// whose input is an Atmos master (ddpjoc).
    pub inputs: &'static str,
    /// The levers to set, as KEY=VAL pairs.
    pub env: &'static [(&'static str, &'static str)],
    /// The encoder mode this layout needs, i.e. the `-e` value (e.g. "bluray", "ddp",
    /// "ddp:lfe_on=0"). Empty means the recipe does not pin a mode. A `--mode` flag overrides it;
    /// both override any `-e` the operator passes natively.
    pub mode: &'static str,
    /// One line on how it works and any caveat.
    pub note: &'static str,
}

/// Every recipe across tools, one table per tool. The driver filters by tool subcommand.
pub static ALL: &[&[Recipe]] = &[DDP_LAYOUTS, DDPJOC_LAYOUTS];

/// JOC streaming and extended backbones. Extended recipes use the Blu-ray pipeline;
/// `/eac3` replaces its AC-3 core with E-AC-3 and supports per-substream rates.
pub static DDPJOC_LAYOUTS: &[Recipe] = &[
    Recipe {
        name: "5.1",
        tool: "dee_ddpjoc_encoder",
        channels: "L R C LFE Ls Rs + JOC",
        inputs: "",
        env: &[],
        mode: "ddpjoc",
        note: "E-AC-3 independent; streaming",
    },
    Recipe {
        name: "5.1.2",
        tool: "dee_ddpjoc_encoder",
        channels: "L R C LFE Ls Rs Lvh Rvh + JOC",
        inputs: "",
        env: &[],
        mode: "bluray",
        note: "AC-3 core + E-AC-3 dependent",
    },
    Recipe {
        name: "5.1.2-top",
        tool: "dee_ddpjoc_encoder",
        channels: "L R C LFE Ls Rs Lts Rts + JOC",
        inputs: "",
        env: &[("DDPJOC_LAYER", "5.1.2-top")],
        mode: "bluray",
        note: "AC-3 core + E-AC-3 dependent",
    },
    Recipe {
        name: "7.1",
        tool: "dee_ddpjoc_encoder",
        channels: "L R C LFE Ls Rs Lrs Rrs + JOC",
        inputs: "",
        env: &[("DDPJOC_LAYER", "7.1")],
        mode: "bluray",
        note: "AC-3 core + E-AC-3 dependent",
    },
    Recipe {
        name: "5.1.2/eac3",
        tool: "dee_ddpjoc_encoder",
        channels: "L R C LFE Ls Rs Lvh Rvh + JOC",
        inputs: "",
        env: &[("DDPJOC_CORE", "eac3")],
        mode: "bluray",
        note: "E-AC-3 independent + dependent",
    },
    Recipe {
        name: "5.1.2-top/eac3",
        tool: "dee_ddpjoc_encoder",
        channels: "L R C LFE Ls Rs Lts Rts + JOC",
        inputs: "",
        env: &[("DDPJOC_LAYER", "5.1.2-top"), ("DDPJOC_CORE", "eac3")],
        mode: "bluray",
        note: "E-AC-3 independent + dependent",
    },
    Recipe {
        name: "7.1/eac3",
        tool: "dee_ddpjoc_encoder",
        channels: "L R C LFE Ls Rs Lrs Rrs + JOC",
        inputs: "",
        env: &[("DDPJOC_LAYER", "7.1"), ("DDPJOC_CORE", "eac3")],
        mode: "bluray",
        note: "E-AC-3 independent + dependent",
    },
];

/// Channel layouts without JOC. Extended layouts have an independent (or AC-3 core) and a
/// dependent substream.
pub static DDP_LAYOUTS: &[Recipe] = &[
    // Single E-AC-3 independent stream.
    Recipe {
        name: "1+1",
        tool: "dee_ddp_encoder",
        channels: "Ch1 Ch2",
        inputs: "Ch1 Ch2",
        env: &[("DDP_ACMOD", "0")],
        mode: "ddp",
        note: "E-AC-3 independent; dual mono",
    },
    Recipe {
        name: "1.0",
        tool: "dee_ddp_encoder",
        channels: "C",
        inputs: "C",
        env: &[],
        mode: "ddp",
        note: "E-AC-3 independent; mono input",
    },
    Recipe {
        name: "1.1",
        tool: "dee_ddp_encoder",
        channels: "C LFE",
        inputs: "- - C LFE - -",
        env: &[
            ("DDP_ACMOD", "1"),
            ("DDP_SET_PARAMS", "5=1"),
            ("DDP_LFE_ROUTE", "1"),
        ],
        mode: "ddp",
        note: "E-AC-3 independent",
    },
    Recipe {
        name: "2.0",
        tool: "dee_ddp_encoder",
        channels: "L R",
        inputs: "L R",
        env: &[("DDP_ACMOD", "2")],
        mode: "ddp",
        note: "E-AC-3 independent",
    },
    Recipe {
        name: "2.1",
        tool: "dee_ddp_encoder",
        channels: "L R LFE",
        inputs: "L R - LFE - -",
        env: &[
            ("DDP_ACMOD", "2"),
            ("DDP_SET_PARAMS", "5=1"),
            ("DDP_LFE_ROUTE", "1"),
        ],
        mode: "ddp",
        note: "E-AC-3 independent",
    },
    Recipe {
        name: "3.0",
        tool: "dee_ddp_encoder",
        channels: "L R C",
        inputs: "L R C - - -",
        env: &[("DDP_ACMOD", "3")],
        mode: "ddp",
        note: "E-AC-3 independent",
    },
    Recipe {
        name: "3.1",
        tool: "dee_ddp_encoder",
        channels: "L R C LFE",
        inputs: "L R C LFE - -",
        env: &[("DDP_ACMOD", "3"), ("DDP_SET_PARAMS", "5=1")],
        mode: "ddp",
        note: "E-AC-3 independent",
    },
    Recipe {
        name: "3.0-back",
        tool: "dee_ddp_encoder",
        channels: "L R S",
        inputs: "L R - - S -",
        env: &[("DDP_ACMOD", "4")],
        mode: "ddp",
        note: "E-AC-3 independent",
    },
    Recipe {
        name: "3.1-back",
        tool: "dee_ddp_encoder",
        channels: "L R LFE S",
        inputs: "L R - LFE S -",
        env: &[("DDP_ACMOD", "4"), ("DDP_SET_PARAMS", "5=1")],
        mode: "ddp",
        note: "E-AC-3 independent",
    },
    Recipe {
        name: "4.0",
        tool: "dee_ddp_encoder",
        channels: "L R C S",
        inputs: "L R C - S -",
        env: &[("DDP_ACMOD", "5")],
        mode: "ddp",
        note: "E-AC-3 independent",
    },
    Recipe {
        name: "4.1",
        tool: "dee_ddp_encoder",
        channels: "L R C LFE S",
        inputs: "L R C LFE S -",
        env: &[("DDP_ACMOD", "5"), ("DDP_SET_PARAMS", "5=1")],
        mode: "ddp",
        note: "E-AC-3 independent",
    },
    Recipe {
        name: "4.0-quad",
        tool: "dee_ddp_encoder",
        channels: "L R Ls Rs",
        inputs: "L R - - Ls Rs",
        env: &[("DDP_ACMOD", "6")],
        mode: "ddp",
        note: "E-AC-3 independent",
    },
    Recipe {
        name: "4.1-quad",
        tool: "dee_ddp_encoder",
        channels: "L R LFE Ls Rs",
        inputs: "L R - LFE Ls Rs",
        env: &[("DDP_ACMOD", "6"), ("DDP_SET_PARAMS", "5=1")],
        mode: "ddp",
        note: "E-AC-3 independent",
    },
    Recipe {
        name: "5.0",
        tool: "dee_ddp_encoder",
        channels: "L R C Ls Rs",
        inputs: "L R C - Ls Rs",
        env: &[],
        mode: "ddp:lfe_on=0",
        note: "E-AC-3 independent",
    },
    Recipe {
        name: "5.1",
        tool: "dee_ddp_encoder",
        channels: "L R C LFE Ls Rs",
        inputs: "L R C LFE Ls Rs",
        env: &[],
        mode: "ddp",
        note: "E-AC-3 independent",
    },
    // AC-3 core with an E-AC-3 dependent stream.
    Recipe {
        name: "5.1.2",
        tool: "dee_ddp_encoder",
        channels: "L R C LFE Ls Rs Lvh Rvh",
        inputs: "L R C LFE Ls Rs Lvh Rvh",
        env: &[("DDP_ACMOD", "19")],
        mode: "bluray",
        note: "AC-3 core + E-AC-3 dependent",
    },
    Recipe {
        name: "5.1.2-top",
        tool: "dee_ddp_encoder",
        channels: "L R C LFE Ls Rs Lts Rts",
        inputs: "L R C LFE Ls Rs Lts Rts",
        env: &[("DDP_ACMOD", "28")],
        mode: "bluray",
        note: "AC-3 core + E-AC-3 dependent",
    },
    Recipe {
        name: "6.1",
        tool: "dee_ddp_encoder",
        channels: "L R C LFE Ls Rs Cs",
        inputs: "L R C LFE Ls Rs Cs -",
        env: &[("DDP_ACMOD", "24")],
        mode: "bluray",
        note: "AC-3 core + E-AC-3 dependent",
    },
    Recipe {
        name: "7.1",
        tool: "dee_ddp_encoder",
        channels: "L R C LFE Ls Rs Lrs Rrs",
        inputs: "L R C LFE Ls Rs Lrs Rrs",
        env: &[],
        mode: "bluray",
        note: "AC-3 core + E-AC-3 dependent",
    },
    Recipe {
        name: "5.0.2",
        tool: "dee_ddp_encoder",
        channels: "L R C Ls Rs Lvh Rvh",
        inputs: "L R C - Ls Rs Lvh Rvh",
        env: &[
            ("DDP_DOWNMIX_MODE", "4"),
            ("DDP_ACMOD", "19"),
            ("DDP_DATA_RATE", "768"),
            ("DDP_SET_PARAMS", "5=0"),
        ],
        mode: "bluray",
        note: "AC-3 core + E-AC-3 dependent",
    },
    Recipe {
        name: "5.0.2-top",
        tool: "dee_ddp_encoder",
        channels: "L R C Ls Rs Lts Rts",
        inputs: "L R C - Ls Rs Lts Rts",
        env: &[
            ("DDP_DOWNMIX_MODE", "4"),
            ("DDP_ACMOD", "28"),
            ("DDP_DATA_RATE", "768"),
            ("DDP_SET_PARAMS", "5=0"),
        ],
        mode: "bluray",
        note: "AC-3 core + E-AC-3 dependent",
    },
    Recipe {
        name: "6.0",
        tool: "dee_ddp_encoder",
        channels: "L R C Ls Rs Cs",
        inputs: "L R C - Ls Rs Cs -",
        env: &[
            ("DDP_DOWNMIX_MODE", "4"),
            ("DDP_ACMOD", "24"),
            ("DDP_DATA_RATE", "768"),
            ("DDP_SET_PARAMS", "5=0"),
        ],
        mode: "bluray",
        note: "AC-3 core + E-AC-3 dependent",
    },
    Recipe {
        name: "7.0",
        tool: "dee_ddp_encoder",
        channels: "L R C Ls Rs Lrs Rrs",
        inputs: "L R C - Ls Rs Lrs Rrs",
        env: &[
            ("DDP_DOWNMIX_MODE", "4"),
            ("DDP_ACMOD", "21"),
            ("DDP_DATA_RATE", "768"),
            ("DDP_SET_PARAMS", "5=0"),
        ],
        mode: "bluray",
        note: "AC-3 core + E-AC-3 dependent",
    },
    // E-AC-3 independent and dependent streams.
    Recipe {
        name: "5.1.2/eac3",
        tool: "dee_ddp_encoder",
        channels: "L R C LFE Ls Rs Lvh Rvh",
        inputs: "L R C LFE Ls Rs Lvh Rvh",
        env: &[
            ("DDP_DOWNMIX_MODE", "4"),
            ("DDP_ACMOD", "19"),
            ("DDP_DATA_RATE", "768"),
        ],
        mode: "ddp",
        note: "E-AC-3 independent + dependent",
    },
    Recipe {
        name: "6.1/eac3",
        tool: "dee_ddp_encoder",
        channels: "L R C LFE Ls Rs Cs",
        inputs: "L R C LFE Ls Rs Cs -",
        env: &[
            ("DDP_DOWNMIX_MODE", "4"),
            ("DDP_ACMOD", "24"),
            ("DDP_DATA_RATE", "768"),
        ],
        mode: "ddp",
        note: "E-AC-3 independent + dependent",
    },
    Recipe {
        name: "7.1/eac3",
        tool: "dee_ddp_encoder",
        channels: "L R C LFE Ls Rs Lrs Rrs",
        inputs: "L R C LFE Ls Rs Lrs Rrs",
        env: &[
            ("DDP_DOWNMIX_MODE", "4"),
            ("DDP_ACMOD", "21"),
            ("DDP_DATA_RATE", "768"),
        ],
        mode: "ddp",
        note: "E-AC-3 independent + dependent",
    },
    Recipe {
        name: "5.1.2-top/eac3",
        tool: "dee_ddp_encoder",
        channels: "L R C LFE Ls Rs Lts Rts",
        inputs: "L R C LFE Ls Rs Lts Rts",
        env: &[
            ("DDP_DOWNMIX_MODE", "4"),
            ("DDP_ACMOD", "28"),
            ("DDP_DATA_RATE", "768"),
        ],
        mode: "ddp",
        note: "E-AC-3 independent + dependent",
    },
    Recipe {
        name: "5.0.2/eac3",
        tool: "dee_ddp_encoder",
        channels: "L R C Ls Rs Lvh Rvh",
        inputs: "L R C - Ls Rs Lvh Rvh",
        env: &[
            ("DDP_DOWNMIX_MODE", "4"),
            ("DDP_ACMOD", "19"),
            ("DDP_DATA_RATE", "768"),
            ("DDP_SET_PARAMS", "5=0"),
        ],
        mode: "ddp",
        note: "E-AC-3 independent + dependent",
    },
    Recipe {
        name: "5.0.2-top/eac3",
        tool: "dee_ddp_encoder",
        channels: "L R C Ls Rs Lts Rts",
        inputs: "L R C - Ls Rs Lts Rts",
        env: &[
            ("DDP_DOWNMIX_MODE", "4"),
            ("DDP_ACMOD", "28"),
            ("DDP_DATA_RATE", "768"),
            ("DDP_SET_PARAMS", "5=0"),
        ],
        mode: "ddp",
        note: "E-AC-3 independent + dependent",
    },
    Recipe {
        name: "6.0/eac3",
        tool: "dee_ddp_encoder",
        channels: "L R C Ls Rs Cs",
        inputs: "L R C - Ls Rs Cs -",
        env: &[
            ("DDP_DOWNMIX_MODE", "4"),
            ("DDP_ACMOD", "24"),
            ("DDP_DATA_RATE", "768"),
            ("DDP_SET_PARAMS", "5=0"),
        ],
        mode: "ddp",
        note: "E-AC-3 independent + dependent",
    },
    Recipe {
        name: "7.0/eac3",
        tool: "dee_ddp_encoder",
        channels: "L R C Ls Rs Lrs Rrs",
        inputs: "L R C - Ls Rs Lrs Rrs",
        env: &[
            ("DDP_DOWNMIX_MODE", "4"),
            ("DDP_ACMOD", "21"),
            ("DDP_DATA_RATE", "768"),
            ("DDP_SET_PARAMS", "5=0"),
        ],
        mode: "ddp",
        note: "E-AC-3 independent + dependent",
    },
];

/// Recipes for a tool.
pub fn for_tool(tool: &str) -> Vec<&'static Recipe> {
    ALL.iter()
        .flat_map(|t| t.iter())
        .filter(|r| r.tool == tool)
        .collect()
}

/// A named recipe for a tool, by its exact name.
pub fn find(tool: &str, name: &str) -> Option<&'static Recipe> {
    ALL.iter()
        .flat_map(|t| t.iter())
        .find(|r| r.tool == tool && r.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each ddp layout's input slots hold exactly its coded channels, each once, in 1, 2, 6 or 8
    /// slots; a .0 layout leaves the LFE slot (4) empty.
    #[test]
    fn input_slots_match_channels() {
        for r in DDP_LAYOUTS {
            let slots: Vec<&str> = r.inputs.split_whitespace().collect();
            assert!(
                [1, 2, 6, 8].contains(&slots.len()),
                "{}: {} slots",
                r.name,
                slots.len()
            );
            let mut used: Vec<&str> = slots.iter().copied().filter(|s| *s != "-").collect();
            let mut coded: Vec<&str> = r.channels.split_whitespace().collect();
            used.sort_unstable();
            coded.sort_unstable();
            assert_eq!(used, coded, "{}", r.name);
            if slots.len() >= 6 {
                assert_eq!(slots[3] == "LFE", r.channels.contains("LFE"), "{}", r.name);
            }
        }
        assert!(DDPJOC_LAYOUTS.iter().all(|r| r.inputs.is_empty()));
    }
}
