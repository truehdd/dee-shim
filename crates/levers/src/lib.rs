//! The engine's levers: what `dee_shim` asks the engine to change in a tool.
//!
//! The CLI collects them as KEY=VAL pairs (layout recipes, switches, `--set`), and
//! `Levers::from_pairs` checks every name and value once, before the tool starts. The host child
//! receives them as one encoded value (`encode`, `decode`) and hands them to the engine, whose
//! modules read the typed fields. The tool's own environment never carries them.

use std::collections::BTreeMap;

/// Every lever name, in the order `to_pairs` writes them.
pub const KEYS: &[&str] = &[
    // dee_ddp_encoder
    "DDP_ACMOD",
    "DDP_DATA_RATE",
    "DDP_DOWNMIX_MODE",
    "DDP_SET_PARAMS",
    "DDP_LFE_ROUTE",
    "DDP_EMDF_OFF",
    "DDP_RATES",
    "DDP_PHASE90",
    "DDP_CORE_TYPE",
    // dee_ddpjoc_encoder
    "DDPJOC_LAYER",
    "DDPJOC_CORE",
    "DDPJOC_RATES",
    "DDPJOC_PHASE90",
    "DDPJOC_OAMD_EXACT_POSITIONS",
    "DDPJOC_OAMD_EXACT_SIZES",
    "DDPJOC_OAMD_OWN_CLUSTERS",
    "DDPJOC_OAMD_EXACT_TIMING",
    "DDPJOC_OAMD_KEEP_QUIET",
    "DDPJOC_OAMD_OWN_GAINS",
    "DDPJOC_OAMD_KEEP_ORDER",
    "DDPJOC_OAMD_CLUSTER_PASSTHROUGH",
    "DDPJOC_OAMD_AS_IS",
    "DDPJOC_OAMD_EXT_PRECISION",
    "DDPJOC_RATE_GATE_OFF",
    // dee_dthd_encoder
    "DTHD_OAMD_EXACT_POSITIONS",
    "DTHD_OAMD_EXACT_SIZES",
    "DTHD_OAMD_OWN_CLUSTERS",
    "DTHD_OAMD_EXACT_TIMING",
    "DTHD_OAMD_KEEP_QUIET",
    "DTHD_OAMD_OWN_GAINS",
    "DTHD_OAMD_KEEP_ORDER",
    "DTHD_OAMD_CLUSTER_PASSTHROUGH",
    "DTHD_OAMD_AS_IS",
    "DTHD_OAMD_UPDATE_TIMES",
    "DTHD_NOISE_SHAPING",
    "DTHD_APODISE",
    "DTHD_AUDIO_UNIT_SCALE",
    "DTHD_AUDIO_EXACT_OVERLAP",
    "DTHD_AUDIO_EXACT_LENGTH",
    "DTHD_AUDIO_AS_IS",
    "DTHD_DRC",
    "DTHD_HIGHRES",
    "DTHD_MLL",
    "DTHD_TARGET",
    "DTHD_COMPOSE",
    "DTHD_LAYOUT",
];

/// A closed set of names, each parsed from and written as one string.
macro_rules! names {
    ($(#[$doc:meta])* $t:ident { $($v:ident = $n:literal),* $(,)? }) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $t { $($v),* }

        impl $t {
            pub const ALL: &'static [$t] = &[$($t::$v),*];

            pub fn name(self) -> &'static str {
                match self { $($t::$v => $n),* }
            }

            fn parse(key: &str, value: &str) -> Result<Self, String> {
                Self::ALL.iter().copied().find(|x| x.name() == value).ok_or_else(|| {
                    let known: Vec<&str> = Self::ALL.iter().map(|x| x.name()).collect();
                    format!("{key}={value} unknown (one of {})", known.join(" "))
                })
            }
        }
    };
}

names! {
    /// A TrueHD DRC profile, by its CLI name.
    Profile {
        FilmStandard = "film_standard",
        FilmLight = "film_light",
        MusicStandard = "music_standard",
        MusicLight = "music_light",
        Speech = "speech",
        None = "none",
    }
}

names! {
    /// The 7.0/7.1 to 5.0/5.1 core downmix of DD+.
    CoreType { Standard = "standard", Pliix = "pliix" }
}

names! {
    /// The JOC backbone layer; 5.1.2 is DEE's own.
    Layer { L512 = "5.1.2", L512Top = "5.1.2-top", L71 = "7.1" }
}

names! {
    /// The JOC core codec; AC-3 is DEE's own.
    Core { Ac3 = "ac3", Eac3 = "eac3" }
}

names! {
    /// The disc type a TrueHD job is authored for, or none: non-conformant-fba keeps only the
    /// TrueHD format's own rules.
    Target {
        BluRay = "bluray",
        DvdA = "dvd-a",
        HddvdVideo = "hddvd-video",
        NonConformantFba = "non-conformant-fba",
    }
}

impl Target {
    /// The `<disctypes>` element name in a `.mll`.
    pub fn mll_name(self) -> &'static str {
        match self {
            Target::BluRay => "BluRay",
            Target::DvdA => "DVD-A",
            // The disc type with the fewest rules (it takes every rate); the high-rate gates
            // lift the rest.
            Target::HddvdVideo | Target::NonConformantFba => "HDDVD-Video",
        }
    }
}

/// The object metadata switches, shared by ddpjoc and dthd. Each is on, off or unset; an unset
/// switch takes its umbrella's value (as is, then cluster pass-through), else off.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Oamd {
    pub exact_positions: Option<bool>,
    pub exact_sizes: Option<bool>,
    pub own_clusters: Option<bool>,
    pub exact_timing: Option<bool>,
    pub keep_quiet: Option<bool>,
    pub own_gains: Option<bool>,
    pub keep_order: Option<bool>,
    pub cluster_passthrough: Option<bool>,
    pub as_is: Option<bool>,
}

impl Oamd {
    pub fn as_is(&self) -> bool {
        self.as_is.unwrap_or(false)
    }

    pub fn exact_positions(&self) -> bool {
        self.exact_positions.unwrap_or(self.as_is())
    }

    pub fn exact_sizes(&self) -> bool {
        self.exact_sizes.unwrap_or(self.as_is())
    }

    pub fn cluster_passthrough(&self) -> bool {
        self.cluster_passthrough.unwrap_or(self.as_is())
    }

    pub fn own_clusters(&self) -> bool {
        self.own_clusters.unwrap_or(self.cluster_passthrough())
    }

    pub fn exact_timing(&self) -> bool {
        self.exact_timing.unwrap_or(self.cluster_passthrough())
    }

    pub fn keep_quiet(&self) -> bool {
        self.keep_quiet.unwrap_or(self.cluster_passthrough())
    }

    pub fn own_gains(&self) -> bool {
        self.own_gains.unwrap_or(self.cluster_passthrough())
    }

    pub fn keep_order(&self) -> bool {
        self.keep_order.unwrap_or(self.cluster_passthrough())
    }

    fn fields(&mut self) -> [(&'static str, &mut Option<bool>); 9] {
        [
            ("OAMD_EXACT_POSITIONS", &mut self.exact_positions),
            ("OAMD_EXACT_SIZES", &mut self.exact_sizes),
            ("OAMD_OWN_CLUSTERS", &mut self.own_clusters),
            ("OAMD_EXACT_TIMING", &mut self.exact_timing),
            ("OAMD_KEEP_QUIET", &mut self.keep_quiet),
            ("OAMD_OWN_GAINS", &mut self.own_gains),
            ("OAMD_KEEP_ORDER", &mut self.keep_order),
            ("OAMD_CLUSTER_PASSTHROUGH", &mut self.cluster_passthrough),
            ("OAMD_AS_IS", &mut self.as_is),
        ]
    }
}

/// dee_ddp_encoder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ddp {
    /// Core parameter 4, the layout (acmod) code.
    pub acmod: Option<i32>,
    /// The data rate the encoder codes at, in kbps.
    pub data_rate: Option<i32>,
    /// Front-end downmix mode for the encode pass.
    pub downmix_mode: Option<i32>,
    /// Core parameters to set or add, (id, value).
    pub set_params: Vec<(i32, i32)>,
    /// Route the LFE for codes 0..2.
    pub lfe_route: bool,
    /// Bypass the EMDF data rate gate.
    pub emdf_off: bool,
    /// Independent and dependent substream rates, in kbps.
    pub rates: Option<[i32; 2]>,
    /// Surround phase-90 shift; off is the change.
    pub phase90: Option<bool>,
    pub core_type: Option<CoreType>,
}

/// dee_ddpjoc_encoder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ddpjoc {
    pub layer: Option<Layer>,
    pub core: Option<Core>,
    pub rates: Option<[i32; 2]>,
    pub phase90: Option<bool>,
    pub oamd: Oamd,
    /// Extended OAMD position precision; defaults to the as is umbrella.
    pub ext_precision: Option<bool>,
    /// Remove the Atmos data-rate policy gate.
    pub rate_gate_off: bool,
}

impl Ddpjoc {
    pub fn ext_precision(&self) -> bool {
        self.ext_precision.unwrap_or(self.oamd.as_is())
    }
}

/// TrueHD presentation types, in the order of `DthdDrc::typed`.
pub const DTHD_TYPES: [&str; 4] = ["2ch", "6ch", "8ch", "atmos"];

/// DTHD_DRC: one profile for every presentation type, and any per type.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DthdDrc {
    pub all: Option<Profile>,
    pub typed: [Option<Profile>; 4],
}

impl DthdDrc {
    /// The profile per presentation type (`DTHD_TYPES`); None leaves the type as DEE sets it.
    pub fn choice(&self) -> [Option<Profile>; 4] {
        self.typed.map(|t| t.or(self.all))
    }

    fn parse(key: &str, value: &str) -> Result<Self, String> {
        let mut drc = DthdDrc::default();
        for entry in value.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            match entry.split_once('=') {
                None => drc.all = Some(Profile::parse(key, entry)?),
                Some((t, p)) => {
                    let k = DTHD_TYPES.iter().position(|x| *x == t).ok_or_else(|| {
                        format!("{key} type {t:?} unknown (one of {})", DTHD_TYPES.join(" "))
                    })?;
                    drc.typed[k] = Some(Profile::parse(key, p)?);
                }
            }
        }
        Ok(drc)
    }

    fn text(&self) -> String {
        let all = self.all.map(|p| p.name().to_string());
        let typed = DTHD_TYPES
            .iter()
            .zip(self.typed)
            .filter_map(|(t, p)| p.map(|p| format!("{t}={}", p.name())));
        all.into_iter().chain(typed).collect::<Vec<_>>().join(",")
    }
}

/// dee_dthd_encoder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dthd {
    pub oamd: Oamd,
    pub oamd_update_times: Option<bool>,
    pub noise_shaping: Option<bool>,
    pub apodise: Option<bool>,
    pub audio_unit_scale: Option<bool>,
    pub audio_exact_overlap: Option<bool>,
    pub audio_exact_length: Option<bool>,
    pub audio_as_is: Option<bool>,
    pub drc: Option<DthdDrc>,
    pub highres: Option<bool>,
    /// A `.mll` that replaces the CLI's job.
    pub mll: Option<String>,
    /// The disc type to swap into the CLI's job.
    pub target: Option<Target>,
    /// A `.mll` template whose body replaces the CLI's job body.
    pub compose: Option<String>,
    /// Presentation layouts, `[TIER=]SPEC` entries joined by `;` (dshim_profiles::dthd_layouts).
    pub layout: Option<String>,
}

impl Dthd {
    /// Each frame's update coded at its sample in the master; follows the cluster pass-through.
    pub fn oamd_update_times(&self) -> bool {
        self.oamd_update_times
            .unwrap_or(self.oamd.cluster_passthrough())
    }

    pub fn audio_as_is(&self) -> bool {
        self.audio_as_is.unwrap_or(false)
    }

    /// None leaves DEE's choice; as is turns it off.
    pub fn noise_shaping(&self) -> Option<bool> {
        self.noise_shaping.or((self.audio_as_is()).then_some(false))
    }

    /// None leaves DEE's choice; as is turns it off.
    pub fn apodise(&self) -> Option<bool> {
        self.apodise.or((self.audio_as_is()).then_some(false))
    }

    pub fn audio_unit_scale(&self) -> bool {
        self.audio_unit_scale.unwrap_or(self.audio_as_is())
    }

    pub fn audio_exact_overlap(&self) -> bool {
        self.audio_exact_overlap.unwrap_or(self.audio_as_is())
    }

    pub fn audio_exact_length(&self) -> bool {
        self.audio_exact_length.unwrap_or(self.audio_as_is())
    }

    /// The high-rate gates: on with DTHD_HIGHRES, or with the non-conformant target unless
    /// DTHD_HIGHRES says off.
    pub fn highres(&self) -> bool {
        self.highres
            .unwrap_or(self.target == Some(Target::NonConformantFba))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Levers {
    pub ddp: Ddp,
    pub ddpjoc: Ddpjoc,
    pub dthd: Dthd,
}

fn switch(key: &str, value: &str) -> Result<bool, String> {
    match value {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!("{key}={value} unknown (on or off)")),
    }
}

fn flag(key: &str, value: &str) -> Result<bool, String> {
    match value {
        "1" | "on" => Ok(true),
        "0" | "off" => Ok(false),
        _ => Err(format!("{key}={value} unknown (1 or 0)")),
    }
}

fn int(key: &str, value: &str) -> Result<i32, String> {
    value
        .trim()
        .parse()
        .map_err(|_| format!("{key}={value} is not an integer"))
}

fn rates(key: &str, value: &str) -> Result<[i32; 2], String> {
    let parsed = value.split_once(',').and_then(|(a, b)| {
        let r = [a.trim().parse().ok()?, b.trim().parse().ok()?];
        r.iter().all(|k| (32..=1024).contains(k)).then_some(r)
    });
    parsed.ok_or_else(|| format!("{key}={value} must be r0,r1 kbps, each 32..1024"))
}

fn pairs(key: &str, value: &str) -> Result<Vec<(i32, i32)>, String> {
    value
        .split(',')
        .filter(|t| !t.trim().is_empty())
        .map(|t| {
            let (a, b) = t
                .split_once('=')
                .ok_or_else(|| format!("{key}: {t:?} is not id=value"))?;
            Ok((int(key, a)?, int(key, b)?))
        })
        .collect()
}

fn on_off(b: bool) -> &'static str {
    if b {
        "on"
    } else {
        "off"
    }
}

impl Levers {
    /// Check and convert KEY=VAL pairs. Any unknown name, bad value or conflicting pair is an
    /// error naming it.
    pub fn from_pairs(pairs_in: &BTreeMap<String, String>) -> Result<Levers, String> {
        let mut l = Levers::default();
        for (key, value) in pairs_in {
            l.set(key, value)?;
        }
        let d = &l.dthd;
        if d.mll.is_some() && (d.target.is_some() || d.compose.is_some() || d.layout.is_some()) {
            return Err(
                "DTHD_MLL (full .mll substitution) and the compose switches (DTHD_TARGET, \
                        DTHD_COMPOSE, DTHD_LAYOUT) are mutually exclusive"
                    .into(),
            );
        }
        if d.compose.is_some() && d.layout.is_some() {
            return Err("DTHD_COMPOSE cannot be combined with DTHD_LAYOUT".into());
        }
        if d.compose.is_some() && d.target.is_some() {
            return Err("DTHD_COMPOSE cannot be combined with DTHD_TARGET".into());
        }
        Ok(l)
    }

    fn set(&mut self, key: &str, v: &str) -> Result<(), String> {
        let (p, j, t) = (&mut self.ddp, &mut self.ddpjoc, &mut self.dthd);
        match key {
            "DDP_ACMOD" => p.acmod = Some(int(key, v)?),
            "DDP_DATA_RATE" => p.data_rate = Some(int(key, v)?),
            "DDP_DOWNMIX_MODE" => p.downmix_mode = Some(int(key, v)?),
            "DDP_SET_PARAMS" => p.set_params = pairs(key, v)?,
            "DDP_LFE_ROUTE" => p.lfe_route = flag(key, v)?,
            "DDP_EMDF_OFF" => p.emdf_off = flag(key, v)?,
            "DDP_RATES" => p.rates = Some(rates(key, v)?),
            "DDP_PHASE90" => p.phase90 = Some(switch(key, v)?),
            "DDP_CORE_TYPE" => p.core_type = Some(CoreType::parse(key, v)?),
            "DDPJOC_LAYER" => j.layer = Some(Layer::parse(key, v)?),
            "DDPJOC_CORE" => j.core = Some(Core::parse(key, v)?),
            "DDPJOC_RATES" => j.rates = Some(rates(key, v)?),
            "DDPJOC_PHASE90" => j.phase90 = Some(switch(key, v)?),
            "DDPJOC_OAMD_EXT_PRECISION" => j.ext_precision = Some(switch(key, v)?),
            "DDPJOC_RATE_GATE_OFF" => j.rate_gate_off = flag(key, v)?,
            "DTHD_OAMD_UPDATE_TIMES" => t.oamd_update_times = Some(switch(key, v)?),
            "DTHD_NOISE_SHAPING" => t.noise_shaping = Some(switch(key, v)?),
            "DTHD_APODISE" => t.apodise = Some(switch(key, v)?),
            "DTHD_AUDIO_UNIT_SCALE" => t.audio_unit_scale = Some(switch(key, v)?),
            "DTHD_AUDIO_EXACT_OVERLAP" => t.audio_exact_overlap = Some(switch(key, v)?),
            "DTHD_AUDIO_EXACT_LENGTH" => t.audio_exact_length = Some(switch(key, v)?),
            "DTHD_AUDIO_AS_IS" => t.audio_as_is = Some(switch(key, v)?),
            "DTHD_DRC" => t.drc = Some(DthdDrc::parse(key, v)?),
            "DTHD_HIGHRES" => t.highres = Some(switch(key, v)?),
            "DTHD_MLL" => t.mll = Some(v.to_string()),
            "DTHD_TARGET" => t.target = Some(Target::parse(key, v)?),
            "DTHD_COMPOSE" => t.compose = Some(v.to_string()),
            "DTHD_LAYOUT" => t.layout = Some(v.to_string()),
            _ => {
                let oamd = if let Some(rest) = key.strip_prefix("DDPJOC_") {
                    Some((&mut j.oamd, rest))
                } else {
                    key.strip_prefix("DTHD_").map(|rest| (&mut t.oamd, rest))
                };
                let slot = oamd.and_then(|(o, rest)| {
                    o.fields()
                        .into_iter()
                        .find(|(n, _)| *n == rest)
                        .map(|(_, s)| s)
                });
                match slot {
                    Some(s) => *s = Some(switch(key, v)?),
                    None => return Err(unknown(key)),
                }
            }
        }
        Ok(())
    }

    /// The levers as canonical KEY=VAL pairs, only those set; `from_pairs` reads them back.
    pub fn to_pairs(&self) -> Vec<(&'static str, String)> {
        let mut out: Vec<(&'static str, String)> = Vec::new();
        let mut put = |k: &'static str, v: Option<String>| {
            if let Some(v) = v {
                out.push((k, v));
            }
        };
        let (p, j, t) = (&self.ddp, &self.ddpjoc, &self.dthd);
        let rate = |r: Option<[i32; 2]>| r.map(|[a, b]| format!("{a},{b}"));
        let sw = |b: Option<bool>| b.map(|b| on_off(b).to_string());
        let fl = |b: bool| b.then(|| "1".to_string());
        put("DDP_ACMOD", p.acmod.map(|v| v.to_string()));
        put("DDP_DATA_RATE", p.data_rate.map(|v| v.to_string()));
        put("DDP_DOWNMIX_MODE", p.downmix_mode.map(|v| v.to_string()));
        put(
            "DDP_SET_PARAMS",
            (!p.set_params.is_empty()).then(|| {
                let items: Vec<String> = p
                    .set_params
                    .iter()
                    .map(|(i, v)| format!("{i}={v}"))
                    .collect();
                items.join(",")
            }),
        );
        put("DDP_LFE_ROUTE", fl(p.lfe_route));
        put("DDP_EMDF_OFF", fl(p.emdf_off));
        put("DDP_RATES", rate(p.rates));
        put("DDP_PHASE90", sw(p.phase90));
        put("DDP_CORE_TYPE", p.core_type.map(|v| v.name().to_string()));
        put("DDPJOC_LAYER", j.layer.map(|v| v.name().to_string()));
        put("DDPJOC_CORE", j.core.map(|v| v.name().to_string()));
        put("DDPJOC_RATES", rate(j.rates));
        put("DDPJOC_PHASE90", sw(j.phase90));
        for (n, s) in j.oamd.clone().fields() {
            put(oamd_key("DDPJOC", n), sw(*s));
        }
        put("DDPJOC_OAMD_EXT_PRECISION", sw(j.ext_precision));
        put("DDPJOC_RATE_GATE_OFF", fl(j.rate_gate_off));
        for (n, s) in t.oamd.clone().fields() {
            put(oamd_key("DTHD", n), sw(*s));
        }
        put("DTHD_OAMD_UPDATE_TIMES", sw(t.oamd_update_times));
        put("DTHD_NOISE_SHAPING", sw(t.noise_shaping));
        put("DTHD_APODISE", sw(t.apodise));
        put("DTHD_AUDIO_UNIT_SCALE", sw(t.audio_unit_scale));
        put("DTHD_AUDIO_EXACT_OVERLAP", sw(t.audio_exact_overlap));
        put("DTHD_AUDIO_EXACT_LENGTH", sw(t.audio_exact_length));
        put("DTHD_AUDIO_AS_IS", sw(t.audio_as_is));
        put("DTHD_DRC", t.drc.as_ref().map(DthdDrc::text));
        put("DTHD_HIGHRES", sw(t.highres));
        put("DTHD_MLL", t.mll.clone());
        put("DTHD_TARGET", t.target.map(|v| v.name().to_string()));
        put("DTHD_COMPOSE", t.compose.clone());
        put("DTHD_LAYOUT", t.layout.clone());
        out
    }

    /// No lever is set: the engine leaves the tool as shipped.
    pub fn is_empty(&self) -> bool {
        self.to_pairs().is_empty()
    }

    /// One line per lever, KEY=VAL, with `\` and newlines in values escaped.
    pub fn encode(&self) -> String {
        let lines: Vec<String> = self
            .to_pairs()
            .into_iter()
            .map(|(k, v)| format!("{k}={}", v.replace('\\', "\\\\").replace('\n', "\\n")))
            .collect();
        lines.join("\n")
    }

    pub fn decode(text: &str) -> Result<Levers, String> {
        let mut map = BTreeMap::new();
        for line in text.split('\n').filter(|l| !l.is_empty()) {
            let (k, v) = line
                .split_once('=')
                .ok_or_else(|| format!("lever line {line:?} has no '='"))?;
            let mut value = String::with_capacity(v.len());
            let mut chars = v.chars();
            while let Some(c) = chars.next() {
                match (c, c == '\\') {
                    (_, true) => match chars.next() {
                        Some('n') => value.push('\n'),
                        Some(other) => value.push(other),
                        None => return Err(format!("lever line {line:?} ends in '\\'")),
                    },
                    (c, false) => value.push(c),
                }
            }
            map.insert(k.to_string(), value);
        }
        Levers::from_pairs(&map)
    }
}

/// The error for an unknown name, listing the names of the same tool (its prefix) if any.
fn unknown(key: &str) -> String {
    let prefix = key.split('_').next().unwrap_or(key);
    let same: Vec<&str> = KEYS
        .iter()
        .copied()
        .filter(|k| k.split('_').next() == Some(prefix))
        .collect();
    let known = if same.is_empty() { KEYS.to_vec() } else { same };
    format!("{key} is not a lever (known: {})", known.join(" "))
}

/// The full name of an object metadata switch, e.g. DDPJOC_OAMD_AS_IS.
fn oamd_key(tool: &str, name: &str) -> &'static str {
    KEYS.iter()
        .find(|k| k.strip_prefix(tool).and_then(|r| r.strip_prefix('_')) == Some(name))
        .expect("every object metadata switch is in KEYS")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(items: &[(&str, &str)]) -> BTreeMap<String, String> {
        items
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// One value for every key, so the round trip covers each.
    fn every() -> BTreeMap<String, String> {
        KEYS.iter()
            .map(|k| {
                let v = match *k {
                    "DDP_ACMOD" => "28",
                    "DDP_DATA_RATE" => "1536",
                    "DDP_DOWNMIX_MODE" => "4",
                    "DDP_SET_PARAMS" => "5=1,13=0",
                    "DDP_RATES" | "DDPJOC_RATES" => "640,512",
                    "DDP_CORE_TYPE" => "pliix",
                    "DDPJOC_LAYER" => "7.1",
                    "DDPJOC_CORE" => "eac3",
                    "DDP_LFE_ROUTE" | "DDP_EMDF_OFF" | "DDPJOC_RATE_GATE_OFF" => "1",
                    "DTHD_DRC" => "film_light,atmos=none",
                    "DTHD_MLL" => "/x/job.mll",
                    "DTHD_TARGET" => "dvd-a",
                    "DTHD_COMPOSE" => "/x/tmpl\\with\nnewline.mll",
                    "DTHD_LAYOUT" => "8ch=L,R,C,-,Ls,Rs,Cs;2ch=1.0",
                    _ => "on",
                };
                (k.to_string(), v.to_string())
            })
            .collect()
    }

    #[test]
    fn round_trip_every_key() {
        let mut m = every();
        // DTHD_MLL excludes the compose switches, and DTHD_COMPOSE the target and layout; check
        // them apart.
        m.remove("DTHD_MLL");
        m.remove("DTHD_TARGET");
        m.remove("DTHD_LAYOUT");
        let l = Levers::from_pairs(&m).unwrap();
        let back: BTreeMap<String, String> = l
            .to_pairs()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        assert_eq!(back, m);
        assert_eq!(Levers::decode(&l.encode()).unwrap(), l);
        let mll = map(&[("DTHD_MLL", "/x/job.mll")]);
        let l = Levers::from_pairs(&mll).unwrap();
        assert_eq!(Levers::decode(&l.encode()).unwrap(), l);
    }

    #[test]
    fn keys_cover_every_slot() {
        // Every KEYS entry is accepted; anything else is refused.
        for (k, v) in every() {
            Levers::from_pairs(&map(&[(&k, &v)])).unwrap_or_else(|e| panic!("{k}: {e}"));
        }
        assert!(Levers::from_pairs(&map(&[("DDPJOC_LAYR", "7.1")]))
            .unwrap_err()
            .contains("not a lever"));
        assert!(Levers::from_pairs(&map(&[("DDP_PHASE90", "maybe")])).is_err());
        assert!(Levers::from_pairs(&map(&[("DDP_SET_PARAMS", "5")])).is_err());
        assert!(Levers::from_pairs(&map(&[("DDPJOC_RATES", "16,512")])).is_err());
    }

    #[test]
    fn umbrellas() {
        let l = Levers::from_pairs(&map(&[
            ("DDPJOC_OAMD_AS_IS", "on"),
            ("DDPJOC_OAMD_KEEP_QUIET", "off"),
        ]))
        .unwrap();
        let o = &l.ddpjoc.oamd;
        assert!(
            o.exact_positions() && o.own_clusters() && o.own_gains() && l.ddpjoc.ext_precision()
        );
        assert!(!o.keep_quiet() && o.keep_order());
        let l = Levers::from_pairs(&map(&[("DTHD_OAMD_CLUSTER_PASSTHROUGH", "on")])).unwrap();
        assert!(l.dthd.oamd.keep_quiet() && !l.dthd.oamd.exact_positions());
        let l = Levers::from_pairs(&map(&[("DTHD_AUDIO_AS_IS", "on")])).unwrap();
        assert_eq!(l.dthd.apodise(), Some(false));
        assert_eq!(l.dthd.noise_shaping(), Some(false));
        let l = Levers::from_pairs(&map(&[("DTHD_AUDIO_AS_IS", "on"), ("DTHD_APODISE", "on")]))
            .unwrap();
        assert_eq!(l.dthd.apodise(), Some(true));
        let l = Levers::from_pairs(&map(&[])).unwrap();
        assert_eq!(l.dthd.apodise(), None);
    }

    #[test]
    fn conflicts() {
        let both = map(&[("DTHD_MLL", "a.mll"), ("DTHD_TARGET", "bluray")]);
        assert!(Levers::from_pairs(&both).is_err());
        let both = map(&[("DTHD_COMPOSE", "t.mll"), ("DTHD_TARGET", "bluray")]);
        assert!(Levers::from_pairs(&both).is_err());
        let both = map(&[("DTHD_COMPOSE", "t.mll"), ("DTHD_LAYOUT", "5.1")]);
        assert!(Levers::from_pairs(&both).is_err());
        let both = map(&[("DTHD_MLL", "a.mll"), ("DTHD_LAYOUT", "5.1")]);
        assert!(Levers::from_pairs(&both).is_err());
        let ok = map(&[("DTHD_TARGET", "dvd-a"), ("DTHD_LAYOUT", "2ch=1.0")]);
        assert!(Levers::from_pairs(&ok).is_ok());
    }

    #[test]
    fn non_conformant_target() {
        let l = Levers::from_pairs(&map(&[("DTHD_TARGET", "non-conformant-fba")])).unwrap();
        assert_eq!(l.dthd.target, Some(Target::NonConformantFba));
        assert_eq!(Target::NonConformantFba.mll_name(), "HDDVD-Video");
        assert!(l.dthd.highres());
        let off = map(&[
            ("DTHD_TARGET", "non-conformant-fba"),
            ("DTHD_HIGHRES", "off"),
        ]);
        assert!(!Levers::from_pairs(&off).unwrap().dthd.highres());
    }

    #[test]
    fn every_recipe_parses() {
        for table in dshim_profiles::recipes::ALL {
            for r in *table {
                let m: BTreeMap<String, String> = r
                    .env
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();
                Levers::from_pairs(&m)
                    .unwrap_or_else(|e| panic!("recipe {} {}: {e}", r.tool, r.name));
            }
        }
    }
}
