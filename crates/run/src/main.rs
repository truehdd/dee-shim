//! dee_shim: the operator-facing CLI, one subcommand per DEE tool. It turns a named recipe
//! (`--layout`, from `dshim_profiles::recipes`) and the switches into engine levers and the
//! binary's own arguments, adds raw levers (`--set`) and native arguments, checks the levers,
//! then runs the binary in a host child where the engine applies them to the mapped image.
//!
//!   dee_shim ddp_encoder --layout 6.1/eac3 -- --input-format wav -i in.wav -o out.ec3
//!   dee_shim ddp_encoder --layout 2.1 --set DDP_DATA_RATE=448 -- --input-format wav -i in.wav -o out.ec3
//!   dee_shim ddp_encoder --list                                     # the layouts
//!
//! `--dry-run` prints the command instead.

use clap::{error::ErrorKind, Args, CommandFactory, Parser, Subcommand, ValueEnum};
use dshim_levers::Levers;
use dshim_profiles::{rates, recipes};
use std::collections::BTreeMap;
use std::io::Write;

mod help;
mod host;
mod paths;
mod preflight;
use help::{DDPJOC_HELP, DDP_HELP, DTHD_HELP, DTHD_KEEP_QUIET};

#[derive(Parser)]
#[command(
    name = "dee_shim",
    about = "Run a DEE tool with the dee-shim engine",
    version = env!("DSHIM_VERSION"),
    propagate_version = true,
    max_term_width = 100
)]
struct Cli {
    /// Shim log level (default: warn; RUST_LOG also supports module filters)
    #[arg(long, global = true, help_heading = "Logging", value_parser = ["off", "error", "warn", "info", "debug", "trace"])]
    log_level: Option<String>,
    /// Shim diagnostics destination; files append and DEE output is unchanged
    ///
    /// stderr, stdout, PATH, pipe:PATH, fd:N (Unix), or handle:N (Windows).
    #[arg(
        long,
        global = true,
        help_heading = "Logging",
        value_name = "DESTINATION"
    )]
    log_output: Option<String>,
    /// Log record format; json writes one object per line
    #[arg(long, global = true, help_heading = "Logging", value_parser = ["text", "json"])]
    log_format: Option<String>,
    #[command(subcommand)]
    tool: Tool,
}

#[derive(Subcommand)]
enum Tool {
    /// Encode Dolby Digital Plus (dee_ddp_encoder)
    #[command(name = "ddp_encoder")]
    #[command(long_about = DDP_HELP, arg_required_else_help = true)]
    #[command(mut_arg("layout", |a| help::layout_arg(a, "dee_ddp_encoder")))]
    #[command(mut_arg("mode", |a| help::mode_arg(a, "dee_ddp_encoder")))]
    #[command(mut_arg("bitrate", |a| help::bitrate_arg(a, "dee_ddp_encoder")))]
    Ddp(DdpArgs),
    /// Encode Dolby Digital Plus with Atmos (dee_ddpjoc_encoder)
    #[command(name = "ddpjoc_encoder")]
    #[command(long_about = DDPJOC_HELP, arg_required_else_help = true)]
    #[command(mut_arg("layout", |a| help::layout_arg(a, "dee_ddpjoc_encoder")))]
    #[command(mut_arg("mode", |a| help::mode_arg(a, "dee_ddpjoc_encoder")))]
    #[command(mut_arg("bitrate", |a| help::bitrate_arg(a, "dee_ddpjoc_encoder")))]
    Ddpjoc(DdpjocArgs),
    /// Encode Dolby TrueHD with Atmos (dee_dthd_encoder)
    #[command(name = "dthd_encoder")]
    #[command(long_about = DTHD_HELP, arg_required_else_help = true)]
    #[command(mut_arg("oamd_keep_quiet", |a| a.long_help(DTHD_KEEP_QUIET)))]
    Dthd(DthdArgs),
}

mod mll;

/// An on/off switch. Unset leaves DEE's behaviour, or an umbrella's choice.
#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum Switch {
    On,
    Off,
}

impl Switch {
    fn as_str(self) -> &'static str {
        match self {
            Switch::On => "on",
            Switch::Off => "off",
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum CoreType {
    Standard,
    Pliix,
}

impl CoreType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Pliix => "pliix",
        }
    }
}

/// Put each set switch into the lever map as `{prefix}_{name}`.
fn put(env: &mut BTreeMap<String, String>, prefix: &str, switches: &[(Option<Switch>, &str)]) {
    for (s, name) in switches {
        if let Some(s) = s {
            env.insert(format!("{prefix}_{name}"), s.as_str().to_string());
        }
    }
}

/// --bitrate: one total, or a rate per substream.
#[derive(Clone, Copy, Debug)]
enum Rate {
    Total(u32),
    Split(u32, u32),
}

impl std::str::FromStr for Rate {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let kbps = |t: &str| {
            t.trim()
                .parse::<u32>()
                .map_err(|_| format!("{t:?} is not a rate in kbps"))
        };
        match s.split_once(':') {
            None => kbps(s).map(Rate::Total),
            Some((a, b)) => {
                let (r0, r1) = (kbps(a)?, kbps(b)?);
                if !(32..=1024).contains(&r0) || !(32..=1024).contains(&r1) {
                    return Err(format!("{s:?}: each substream rate must be 32..1024 kbps"));
                }
                Ok(Rate::Split(r0, r1))
            }
        }
    }
}

#[derive(Args)]
#[command(next_help_heading = "Layout and rate")]
struct LayoutArgs {
    /// Channel layout, by name (see --list)
    #[arg(long, value_name = "NAME")]
    layout: Option<String>,

    /// Data rate in kbps: a total (1152), or R0:R1 per substream (640:512)
    ///
    /// A total uses native rates or the recipe's allocation. R0:R1 sets the core/independent
    /// and dependent rates directly for an extended layout, with either AC-3 or E-AC-3 first.
    /// The total is their sum and need not be a native rate (see --help).
    #[arg(long, value_name = "KBPS|R0:R1")]
    bitrate: Option<Rate>,

    /// Encoder mode, the binary's -e, hidden modes included
    ///
    /// Overrides the layout's mode and a native -e. Sub-options pass through (ddp:lfe_on=0)
    #[arg(long, value_name = "MODE")]
    mode: Option<String>,

    /// List the layouts and the accepted data rates, then exit
    #[arg(long, display_order = 99)]
    list: bool,
}

#[derive(Args)]
#[command(next_help_heading = "Object metadata")]
struct Oamd {
    /// Code object positions as authored, unsmoothed
    ///
    /// DEE smooths them with a 25 ms one-pole per 512 samples, so a master smoothed once already,
    /// such as a TrueHD decode, is smoothed twice
    #[arg(long, value_name = "on|off", hide = true)]
    oamd_exact_positions: Option<Switch>,

    /// Code object sizes as authored, unsmoothed
    ///
    /// DEE smooths them with a 20 ms one-pole per 512 samples
    #[arg(long, value_name = "on|off", hide = true)]
    oamd_exact_sizes: Option<Switch>,

    /// When the objects fit, give each its own cluster
    ///
    /// DEE folds near objects into one cluster
    #[arg(long, value_name = "on|off", hide = true)]
    oamd_own_clusters: Option<Switch>,

    /// When the objects fit, code each jump in its own frame
    ///
    /// DEE holds or advances a position jump by one frame
    #[arg(long, value_name = "on|off", hide = true)]
    oamd_exact_timing: Option<Switch>,

    /// When the objects fit, keep quiet ones in the clusterer
    ///
    /// When every object with any weight fits, let the quiet ones (weight under 0.5, about
    /// -90 dBFS) reach the clusterer; DEE gives them a leftover position
    #[arg(long, value_name = "on|off", hide = true)]
    oamd_keep_quiet: Option<Switch>,

    /// When the objects fit, mix each only into its own cluster
    ///
    /// DEE can mix an object into another cluster at the same position
    #[arg(long, value_name = "on|off", hide = true)]
    oamd_own_gains: Option<Switch>,

    /// When the objects fit, code the elements in master order
    ///
    /// Bed channels other than LFE that sound from the start, then the objects; DEE codes them
    /// in the order its clusterer first picked them
    #[arg(long, value_name = "on|off", hide = true)]
    oamd_keep_order: Option<Switch>,

    /// Own clusters, exact timing, keep quiet, own gains and keep order
    ///
    /// Above the clusterer's budget the clusterer runs as shipped
    #[arg(long, value_name = "on|off", hide = true)]
    oamd_cluster_passthrough: Option<Switch>,

    /// Code a prepared master's object metadata as authored
    ///
    /// Positions and sizes unsmoothed (positions at extended precision in ddpjoc); when the
    /// objects fit, one cluster per object, each jump in its own frame, quiet objects kept,
    /// each mixed only into its own cluster, and the elements in master order
    #[arg(long, value_name = "on|off", hide_possible_values = true)]
    oamd_as_is: Option<Switch>,
}

impl Oamd {
    fn put(&self, env: &mut BTreeMap<String, String>, prefix: &str) {
        put(
            env,
            prefix,
            &[
                (self.oamd_exact_positions, "OAMD_EXACT_POSITIONS"),
                (self.oamd_exact_sizes, "OAMD_EXACT_SIZES"),
                (self.oamd_own_clusters, "OAMD_OWN_CLUSTERS"),
                (self.oamd_exact_timing, "OAMD_EXACT_TIMING"),
                (self.oamd_keep_quiet, "OAMD_KEEP_QUIET"),
                (self.oamd_own_gains, "OAMD_OWN_GAINS"),
                (self.oamd_keep_order, "OAMD_KEEP_ORDER"),
                (self.oamd_cluster_passthrough, "OAMD_CLUSTER_PASSTHROUGH"),
                (self.oamd_as_is, "OAMD_AS_IS"),
            ],
        );
    }
}

#[derive(Args, Default)]
#[command(next_help_heading = "Runner")]
struct Runner {
    /// Set an engine lever by name (see --help); repeatable, applied last
    #[arg(long = "set", value_name = "KEY=VAL", value_parser = setting)]
    set: Vec<String>,

    /// The DEE binary (default: <tool> beside dee_shim)
    #[arg(long, value_name = "PATH")]
    binary: Option<String>,

    /// Print the command instead of running it
    #[arg(long)]
    dry_run: bool,

    /// Arguments for the DEE binary itself, after `--`
    #[arg(last = true, allow_hyphen_values = true)]
    native: Vec<String>,
}

#[derive(Args)]
struct DdpArgs {
    #[command(flatten)]
    recipe: LayoutArgs,

    /// 5.1 core downmix for 7.0/7.1 layouts, including /eac3
    ///
    /// Native default: pliix. Standard folds each rear channel into its matching surround;
    /// PLIIx mixes each rear into both surrounds. The stereo downmix metadata is separate.
    #[arg(
        long,
        value_name = "standard|pliix",
        hide_possible_values = true,
        help_heading = "Core"
    )]
    core_type: Option<CoreType>,

    #[command(flatten)]
    run: Runner,
}

#[derive(Args)]
struct DdpjocArgs {
    #[command(flatten)]
    recipe: LayoutArgs,

    /// Surround phase-90 filter in the coded backbone (default: on)
    ///
    /// Applies in ddpjoc and bluray modes. Off removes the filter from the coded surrounds,
    /// changing legacy channel decoding; the Atmos render is unchanged.
    #[arg(
        long,
        value_name = "on|off",
        hide_possible_values = true,
        help_heading = "Core"
    )]
    phase90: Option<Switch>,

    /// Code positions with OAMD extended precision
    ///
    /// A fifth of a 6-bit step, as dee_dthd_encoder codes them with legacy_authoring=0
    #[arg(
        long,
        value_name = "on|off",
        hide = true,
        help_heading = "Object metadata"
    )]
    oamd_ext_precision: Option<Switch>,

    #[command(flatten)]
    oamd: Oamd,
    #[command(flatten)]
    run: Runner,
}

#[derive(Args)]
struct DthdArgs {
    /// Channel layout of a presentation (2ch, 6ch, 8ch), or of the top one; repeatable
    ///
    /// NAME is a layout from --list or a comma list of slot labels (L,R,-,-,Cs), - leaving a
    /// slot out. Renames the slots the CLI reads from the input, or picks a downmix's outputs
    #[arg(
        long,
        value_name = "[TIER=]NAME",
        value_parser = dthd_layout,
        conflicts_with_all = ["mll", "compose"],
        help_heading = "Layout"
    )]
    layout: Vec<String>,

    /// List the layouts, then exit
    #[arg(long, help_heading = "Layout")]
    list: bool,

    #[command(flatten)]
    oamd: Oamd,

    /// Code each frame's update at its sample in the master, with its ramp
    ///
    /// DEE codes it at the start of its 1536-sample frame with a 1536-sample ramp
    #[arg(
        long,
        value_name = "on|off",
        hide = true,
        help_heading = "Object metadata"
    )]
    oamd_update_times: Option<Switch>,

    /// Requantise every channel to 20 bits with a shaped dither
    ///
    /// Unset keeps DEE's choice: on for Atmos, off for channel input
    #[arg(
        long,
        value_name = "on|off",
        hide_possible_values = true,
        help_heading = "Audio"
    )]
    noise_shaping: Option<Switch>,

    /// Apply DEE's apodising filter, which changes the coded samples
    ///
    /// Unset keeps DEE's choice: on for 96 kHz input only. On takes effect from 88.2 kHz up; DEE
    /// ignores it at 44.1 and 48 kHz, Atmos included
    #[arg(
        long,
        value_name = "on|off",
        hide_possible_values = true,
        help_heading = "Audio"
    )]
    apodise: Option<Switch>,

    /// Hand the TrueHD core the PCM at the scale it was read at
    ///
    /// DEE scales it by 8388607/8388608, so samples at or above 0.75 of full scale lose 1 LSB
    ///
    #[arg(
        long,
        value_name = "on|off",
        hide_possible_values = true,
        hide = true,
        help_heading = "Audio"
    )]
    audio_unit_scale: Option<Switch>,

    /// Keep a lone object's own samples where blocks overlap
    ///
    /// For a cluster that carries one object alone. DEE sums the clusterer's two windowed block
    /// halves in float, up to 1 LSB off on samples at or above a quarter of full scale
    #[arg(
        long,
        value_name = "on|off",
        hide_possible_values = true,
        hide = true,
        help_heading = "Audio"
    )]
    audio_exact_overlap: Option<Switch>,

    /// End the stream at the master's last sample
    ///
    /// DEE pads the master to a whole 512-sample block
    #[arg(
        long,
        value_name = "on|off",
        hide_possible_values = true,
        hide = true,
        help_heading = "Audio"
    )]
    audio_exact_length: Option<Switch>,

    /// Code the audio as it was read: noise shaping and apodising off, PCM at the scale it was
    /// read at, a lone object's samples kept exact where blocks overlap, and the stream ended
    /// at the master's last sample
    ///
    /// --noise-shaping or --apodise given with it wins
    #[arg(
        long,
        value_name = "on|off",
        hide_possible_values = true,
        help_heading = "Audio"
    )]
    as_is: Option<Switch>,

    /// Allow more than the disc channel limit at 176.4/192 kHz
    ///
    /// DEE refuses more than 6 channels (BluRay) or 2 (HD DVD-Video) at 176.4 or 192 kHz, in the
    /// job and again in its bitstream verifier. This lifts every such gate so an 8-channel
    /// 192 kHz FBA stream can be authored. On with --target non-conformant-fba
    #[arg(
        long,
        value_name = "on|off",
        hide_possible_values = true,
        hide = true,
        help_heading = "High rate"
    )]
    highres: Option<Switch>,

    /// DRC profile of a presentation (2ch, 6ch, 8ch, atmos), or of all; repeatable
    ///
    /// PROFILE is film_standard, film_light, music_standard, music_light, speech or none. Over
    /// DEE's --presentation TYPE:drc_profile; none keeps only the peak protection
    #[arg(long, value_name = "[TYPE=]PROFILE", value_parser = dthd_drc, help_heading = "DRC")]
    drc: Vec<String>,

    /// Read FILE as the engine job instead of the one built from --presentation
    ///
    /// Points the encoder's options file reader at FILE, reaching .mll-only parameters the CLI
    /// cannot express. The switches above still apply
    #[arg(long, value_name = "FILE", help_heading = "Job")]
    mll: Option<String>,

    /// Retarget the CLI's composed job to a disc type
    ///
    /// bluray, dvd-a, hddvd-video or non-conformant-fba. Swaps the <disctypes> in the job the CLI
    /// builds, so the CLI composes for a target it never selects on its own. dvd-a writes an FBB
    /// (MLP) stream, which has no DRC or loudness metadata: 2ch at any rate, 6ch up to 96 kHz.
    /// non-conformant-fba drops every disc rule and keeps TrueHD's own: any of the six rates at
    /// 2, 6 or 8 channels, within the 18 Mbps peak data rate. A target also lifts the CLI's own
    /// 48/96 kHz input check, so the disc type decides the rate: 44.1, 88.2 and 176.4 kHz need
    /// hddvd-video, dvd-a or non-conformant-fba, and 176.4 kHz also needs
    /// --loudness-management skip:dialnorm=N, as DEE cannot measure loudness at that rate.
    #[arg(long, value_name = "TARGET", value_parser = dthd_target, help_heading = "Job")]
    target: Option<String>,

    /// Author the engine job from a JSON spec
    ///
    /// FILE is a high-level spec (disctype, presentations, channel <use>/file/slot/coef, group1); the
    /// job the CLI composes is replaced with it, keeping the CLI's output path. Reaches the rare
    /// channel layouts, the 44.1 kHz family and >2 presentations without hand-writing a .mll.
    /// A channel's slot takes its audio from the CLI input after --, which otherwise only has to
    /// pass the CLI's checks, at any rate. --noise-shaping,
    /// --apodise and --as-is fill a presentation's apply_noise_shaping and apodise when the
    /// spec leaves them out
    #[arg(long, value_name = "FILE", help_heading = "Job")]
    compose: Option<String>,

    #[command(flatten)]
    run: Runner,
}

/// Check a dthd --target value.
fn dthd_target(v: &str) -> Result<String, String> {
    match v {
        "bluray" | "dvd-a" | "hddvd-video" | "non-conformant-fba" => Ok(v.to_string()),
        _ => Err(format!(
            "target {v:?} unknown (bluray, dvd-a, hddvd-video or non-conformant-fba)"
        )),
    }
}

/// Check a dthd --layout entry, `[TIER=]NAME`.
fn dthd_layout(v: &str) -> Result<String, String> {
    if v.contains(';') {
        return Err(format!("{v:?}: one presentation per --layout"));
    }
    dshim_profiles::dthd_layouts::parse(v)?;
    Ok(v.to_string())
}

/// Print `dthd --list`: each layout's input slots and the presentations it fits.
fn dthd_list() {
    use dshim_profiles::dthd_layouts::{slots, Tier, LAYOUTS};
    println!(
        "layouts for dthd_encoder (WAV input slots in order, - is an empty slot; slots past a layout are empty):"
    );
    let width = LAYOUTS
        .iter()
        .map(|l| l.name.len())
        .max()
        .unwrap_or(0)
        .max(6);
    let cell = 5;
    let head: String = (1..=8).map(|n| format!("{n:<cell$}")).collect();
    println!("  {:<width$} {head}{:<14}note", "layout", "for");
    for l in LAYOUTS {
        let labels: Vec<&str> = l.slots.split_whitespace().collect();
        let row: String = (0..8)
            .map(|k| format!("{:<cell$}", labels.get(k).copied().unwrap_or("")))
            .collect();
        // One fixed cell per presentation, so each stays in its own column.
        let fits: String = Tier::ALL
            .into_iter()
            .map(|t| {
                let name = if slots(l.name, t.slots()).is_ok() {
                    t.name()
                } else {
                    ""
                };
                format!("{name:<4}")
            })
            .collect();
        let line = format!("  {:<width$} {row}{fits}  {}", l.name, l.note);
        println!("{}", line.trim_end());
    }
}

/// Check a dthd --drc entry, `[TYPE=]PROFILE`.
fn dthd_drc(v: &str) -> Result<String, String> {
    let (ty, p) = v.split_once('=').map_or((None, v), |(t, p)| (Some(t), p));
    if let Some(t) = ty.filter(|t| !["2ch", "6ch", "8ch", "atmos"].contains(t)) {
        return Err(format!(
            "presentation {t:?} unknown (2ch, 6ch, 8ch or atmos)"
        ));
    }
    const PROFILES: [&str; 6] = [
        "film_standard",
        "film_light",
        "music_standard",
        "music_light",
        "speech",
        "none",
    ];
    if !PROFILES.contains(&p) {
        return Err(format!("profile {p:?} unknown ({})", PROFILES.join(", ")));
    }
    Ok(v.to_string())
}

impl Tool {
    fn exe(&self) -> &'static str {
        match self {
            Tool::Ddp(_) => "dee_ddp_encoder",
            Tool::Ddpjoc(_) => "dee_ddpjoc_encoder",
            Tool::Dthd(_) => "dee_dthd_encoder",
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Tool::Ddp(_) => "ddp_encoder",
            Tool::Ddpjoc(_) => "ddpjoc_encoder",
            Tool::Dthd(_) => "dthd_encoder",
        }
    }

    fn recipe(&self) -> Option<&LayoutArgs> {
        match self {
            Tool::Ddp(a) => Some(&a.recipe),
            Tool::Ddpjoc(a) => Some(&a.recipe),
            Tool::Dthd(_) => None,
        }
    }

    fn run(&self) -> &Runner {
        match self {
            Tool::Ddp(a) => &a.run,
            Tool::Ddpjoc(a) => &a.run,
            Tool::Dthd(a) => &a.run,
        }
    }

    fn run_mut(&mut self) -> &mut Runner {
        match self {
            Tool::Ddp(args) => &mut args.run,
            Tool::Ddpjoc(args) => &mut args.run,
            Tool::Dthd(args) => &mut args.run,
        }
    }

    /// The levers of the tool's own switches.
    fn switches(
        &self,
        env: &mut BTreeMap<String, String>,
    ) -> Result<Option<tempfile::NamedTempFile>, String> {
        match self {
            Tool::Ddp(a) => {
                if let Some(core_type) = a.core_type {
                    env.insert("DDP_CORE_TYPE".to_owned(), core_type.as_str().to_owned());
                }
            }
            Tool::Ddpjoc(a) => {
                put(
                    env,
                    "DDPJOC",
                    &[
                        (a.phase90, "PHASE90"),
                        (a.oamd_ext_precision, "OAMD_EXT_PRECISION"),
                    ],
                );
                a.oamd.put(env, "DDPJOC");
            }
            Tool::Dthd(a) => {
                a.oamd.put(env, "DTHD");
                put(
                    env,
                    "DTHD",
                    &[
                        (a.oamd_update_times, "OAMD_UPDATE_TIMES"),
                        (a.noise_shaping, "NOISE_SHAPING"),
                        (a.apodise, "APODISE"),
                        (a.audio_unit_scale, "AUDIO_UNIT_SCALE"),
                        (a.audio_exact_overlap, "AUDIO_EXACT_OVERLAP"),
                        (a.audio_exact_length, "AUDIO_EXACT_LENGTH"),
                        (a.as_is, "AUDIO_AS_IS"),
                        (a.highres, "HIGHRES"),
                    ],
                );
                if !a.drc.is_empty() {
                    env.insert("DTHD_DRC".to_string(), a.drc.join(","));
                }
                if !a.layout.is_empty() {
                    let joined = a.layout.join(";");
                    dshim_profiles::dthd_layouts::parse(&joined)
                        .map_err(|e| format!("--layout: {e}"))?;
                    env.insert("DTHD_LAYOUT".to_string(), joined);
                }
                if let Some(p) = &a.mll {
                    env.insert("DTHD_MLL".to_string(), p.clone());
                }
                if let Some(t) = &a.target {
                    env.insert("DTHD_TARGET".to_string(), t.clone());
                }
                if let Some(spec) = &a.compose {
                    let as_is = a.as_is == Some(Switch::On);
                    let pick =
                        |s: Option<Switch>| s.map(|s| s == Switch::On).or(as_is.then_some(false));
                    let body = mll::author(spec, pick(a.noise_shaping), pick(a.apodise))
                        .map_err(|e| format!("--compose: {e}"))?;
                    let mut template = tempfile::Builder::new()
                        .prefix("dee-shim-compose-")
                        .suffix(".mll")
                        .tempfile()
                        .map_err(|e| format!("--compose: create template: {e}"))?;
                    template
                        .write_all(body.as_bytes())
                        .map_err(|e| format!("--compose: write template: {e}"))?;
                    env.insert(
                        "DTHD_COMPOSE".to_string(),
                        template.path().to_string_lossy().into_owned(),
                    );
                    return Ok(Some(template));
                }
            }
        }
        Ok(None)
    }
}

fn usage_error(tool: &Tool, kind: ErrorKind, message: impl std::fmt::Display) -> ! {
    let mut command = Cli::command();
    command.build();
    command
        .find_subcommand_mut(tool.name())
        .expect("tool has a subcommand")
        .error(kind, message)
        .exit()
}

fn setting(value: &str) -> Result<String, String> {
    match value.split_once('=') {
        Some((key, _)) if !key.is_empty() => Ok(value.to_owned()),
        _ => Err("expected KEY=VAL with a nonempty key".into()),
    }
}

/// Remove native copies of the options dee_shim sets itself, `-e`/`--encoder` (`drop_mode`) and
/// `--data-rate` (`drop_rate`), in both `--opt value` and `--opt=value` forms.
fn strip_owned(native: &[String], drop_mode: bool, drop_rate: bool) -> Vec<String> {
    let takes_value = |t: &str| -> bool {
        (drop_mode && (t == "-e" || t == "--encoder")) || (drop_rate && t == "--data-rate")
    };
    let inline = |t: &str| -> bool {
        (drop_mode && (t.starts_with("-e=") || t.starts_with("--encoder=")))
            || (drop_rate && t.starts_with("--data-rate="))
    };
    let mut out = Vec::new();
    let mut skip_next = false;
    for t in native {
        if skip_next {
            skip_next = false;
            continue;
        }
        if takes_value(t) {
            skip_next = true;
            continue;
        }
        if inline(t) {
            continue;
        }
        out.push(t.clone());
    }
    out
}

/// An `-e` value as its mode, when it names one, and its sub-options.
fn split_mode(value: Option<&str>) -> (Option<String>, Vec<String>) {
    let mut parts: Vec<String> = value
        .unwrap_or("")
        .split(':')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect();
    let base = (parts.first().is_some_and(|p| !p.contains('='))).then(|| parts.remove(0));
    (base, parts)
}

/// The `-e`/`--encoder` value in native args, if the operator gave one.
fn native_mode(native: &[String]) -> Option<String> {
    let mut it = native.iter();
    while let Some(t) = it.next() {
        if t == "-e" || t == "--encoder" {
            return it.next().cloned();
        }
        if let Some(v) = t
            .strip_prefix("-e=")
            .or_else(|| t.strip_prefix("--encoder="))
        {
            return Some(v.to_string());
        }
    }
    None
}

fn native_rate(native: &[String]) -> Result<Option<u32>, String> {
    let mut args = native.iter();
    while let Some(arg) = args.next() {
        let value = if arg == "--data-rate" {
            Some(args.next().ok_or("--data-rate needs a value")?.as_str())
        } else {
            arg.strip_prefix("--data-rate=")
        };
        if let Some(value) = value {
            return if value == "auto" {
                Ok(None)
            } else {
                value
                    .parse()
                    .map(Some)
                    .map_err(|_| format!("--data-rate {value:?}: expected a total in kbps or auto"))
            };
        }
    }
    Ok(None)
}

fn parse_set(items: &[String]) -> BTreeMap<String, String> {
    items
        .iter()
        .filter_map(|s| {
            s.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect()
}

fn native_temp_requested(arguments: &[String]) -> bool {
    arguments.iter().enumerate().any(|(index, argument)| {
        argument == "--temp-dir"
            || argument.starts_with("--temp-dir=")
            || argument == "--keep-temp=1"
            || (argument == "--keep-temp"
                && arguments.get(index + 1).map(String::as_str) == Some("1"))
    })
}

fn native_temp_dir(arguments: &[String]) -> Option<&std::path::Path> {
    arguments
        .iter()
        .enumerate()
        .find_map(|(index, argument)| {
            if argument == "--temp-dir" {
                arguments.get(index + 1).map(String::as_str)
            } else {
                argument.strip_prefix("--temp-dir=")
            }
        })
        .map(std::path::Path::new)
}

/// The Blu-ray mode's own data rates: DEE accepts these totals itself.
const BLURAY: &[u16] = rates::JOC_BLURAY;

impl Cli {
    fn prepare_launch(&mut self) -> Result<(), String> {
        let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
        let filter = self.log_level.clone().unwrap_or_else(dshim_log::filter);
        let output = self.log_output.clone().unwrap_or_else(dshim_log::output);
        let format = self.log_format.clone().unwrap_or_else(dshim_log::format);
        let output = dshim_log::resolve(&output, &cwd);
        let output = dshim_log::init(&filter, &output, &format)?;
        std::env::set_var("DSHIM_LOG", output);
        std::env::set_var("DSHIM_LOG_FORMAT", format);
        std::env::set_var("DSHIM_LOG_LEVEL", filter);
        paths::native(&mut self.tool.run_mut().native, &cwd);
        let exe = self.tool.exe();
        let run = self.tool.run_mut();
        if run.binary.is_none() {
            run.binary = beside_dee_shim(exe);
        }
        Ok(())
    }
}

/// The tool `exe` (with the platform's executable suffix) in the directory that holds dee_shim,
/// either as invoked or with symlinks resolved, so a dee_shim placed in or linked from a DEE
/// install needs no `--binary`. The caller's working directory is not used or changed.
fn beside_dee_shim(exe: &str) -> Option<String> {
    let me = std::env::current_exe().ok()?;
    let name = format!("{exe}{}", std::env::consts::EXE_SUFFIX);
    [Some(me.clone()), me.canonicalize().ok()]
        .into_iter()
        .flatten()
        .filter_map(|path| Some(path.parent()?.join(&name)))
        .find(|candidate| candidate.is_file())
        .map(|found| found.to_string_lossy().into_owned())
}

/// Run the tool in a host child (see `host`): its exit status, or 1 or 2 if it cannot start.
fn in_process(
    run: &Runner,
    exe: &str,
    native: &[String],
    levers: &Levers,
    temp: Option<&std::path::Path>,
) -> i32 {
    let binary = match &run.binary {
        Some(binary) => std::path::PathBuf::from(binary),
        None => {
            log::error!("--binary <path to {exe}> is required (none beside dee_shim)");
            return 2;
        }
    };
    if run.dry_run {
        println!("host {} in-process", binary.display());
        println!(
            "levers: {}",
            levers
                .to_pairs()
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        println!("native arguments: {}", native.join(" "));
        return 0;
    }
    if !levers.is_empty() {
        if let Err(e) = preflight::optional_profile(&binary, exe) {
            log::error!("optional capability preflight: {e}");
            return 2;
        }
    }
    match host::spawn(&binary, native, levers, temp) {
        Ok(code) => code,
        Err(error) => {
            log::error!("{error}");
            1
        }
    }
}

fn main() {
    // Drop launch resources before exiting with the native status.
    std::process::exit(run());
}

fn run() -> i32 {
    if let Some(code) = host::child() {
        return code;
    }
    let mut cli = Cli::parse();
    if let Err(error) = cli.prepare_launch() {
        eprintln!("dee_shim: {error}");
        return 2;
    }
    let tool = &cli.tool;
    let exe = tool.exe();
    let run = tool.run();
    let recipe = tool.recipe();
    // --bitrate is a total (K) or a rate per substream (R0:R1) for DD+ and JOC.
    let (bitrate, split_arg) = match recipe.and_then(|r| r.bitrate) {
        Some(Rate::Total(k)) => (Some(k), None),
        Some(Rate::Split(r0, r1)) => {
            if exe != "dee_ddpjoc_encoder" && exe != "dee_ddp_encoder" {
                usage_error(tool, ErrorKind::InvalidValue, format!("--bitrate {r0}:{r1}: a rate per substream is for ddpjoc_encoder or ddp_encoder (a two-substream layout); give {} a total", tool.name()));
            }
            (None, Some((r0, r1)))
        }
        None => (None, None),
    };
    // A native `--data-rate` takes the `--bitrate` path, and both given must agree. dthd's
    // `--data-rate` is its own option.
    let mut bitrate = bitrate;
    let mut rate_flag = "--bitrate";
    if recipe.is_some() {
        let native = native_rate(&run.native)
            .unwrap_or_else(|error| usage_error(tool, ErrorKind::InvalidValue, error));
        if let Some(n) = native {
            let ours = bitrate.or(split_arg.map(|(r0, r1)| r0 + r1));
            match ours {
                Some(k) if k != n => usage_error(
                    tool,
                    ErrorKind::ArgumentConflict,
                    format!("--bitrate gives {k} kbps but the native --data-rate gives {n}; give one, or the same total"),
                ),
                Some(_) => {}
                None => {
                    bitrate = Some(n);
                    rate_flag = "--data-rate";
                }
            }
        }
    }
    if let Tool::Dthd(a) = tool {
        if a.list {
            dthd_list();
            return 0;
        }
    }
    if recipe.is_some_and(|r| r.list) {
        let layouts = recipes::for_tool(exe);
        // One aligned column per channel: the input slots where the layouts have them (ddp),
        // else the coded backbone (ddpjoc, whose input is an Atmos master).
        let slots = layouts.iter().any(|r| !r.inputs.is_empty());
        let labels = |r: &recipes::Recipe| -> Vec<&'static str> {
            let text = if slots {
                r.inputs
            } else {
                r.channels.trim_end_matches(" + JOC")
            };
            text.split_whitespace().collect()
        };
        if slots {
            println!(
                "layouts for {} (WAV input slots in order, - is an empty slot; -e is the native mode):",
                tool.name()
            );
        } else {
            println!(
                "layouts for {} (coded backbone channels, each plus JOC objects; -e is the native mode):",
                tool.name()
            );
        }
        let width = layouts
            .iter()
            .map(|r| r.name.len())
            .chain([6])
            .max()
            .unwrap_or(0);
        let cell = layouts
            .iter()
            .flat_map(|r| labels(r))
            .map(str::len)
            .max()
            .unwrap_or(0)
            + 1;
        let count = layouts.iter().map(|r| labels(r).len()).max().unwrap_or(0);
        let row = |cells: Vec<String>| -> String {
            let mut text: String = cells.iter().map(|c| format!("{c:<cell$}")).collect();
            text.extend(std::iter::repeat_n(' ', (count - cells.len()) * cell));
            text
        };
        let head = if slots {
            row((1..=count).map(|n| n.to_string()).collect())
        } else {
            format!("{:<span$}", "channels", span = count * cell)
        };
        println!("  {:<width$} {head}mode", "layout");
        for r in layouts {
            let mode = if r.mode.is_empty() { "-" } else { r.mode };
            let cells = labels(r).iter().map(|c| c.to_string()).collect();
            println!(
                "  {:<width$} {}-e {:<13} {}",
                r.name,
                row(cells),
                mode,
                r.note
            );
        }
        let r = rates::render(exe, 2, 92);
        if !r.is_empty() {
            print!("\n{r}");
        }
        return 0;
    }

    if let Tool::Ddp(args) = tool {
        if args.core_type.is_some()
            && !args
                .recipe
                .layout
                .as_deref()
                .is_some_and(|layout| matches!(layout.split('/').next(), Some("7.0" | "7.1")))
        {
            usage_error(
                tool,
                ErrorKind::ArgumentConflict,
                "--core-type requires --layout 7.0 or 7.1 (also /eac3)",
            );
        }
    }

    // The recipe's levers, and the mode it wants.
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    let mut recipe_mode: Option<String> = None;
    if let Some(name) = recipe.and_then(|r| r.layout.as_ref()) {
        let Some(r) = recipes::find(exe, name) else {
            usage_error(
                tool,
                ErrorKind::InvalidValue,
                format!(
                    "no layout {name:?} for {}; see `dee_shim {} --list`",
                    tool.name(),
                    tool.name()
                ),
            );
        };
        for (k, v) in r.env {
            env.insert(k.to_string(), v.to_string());
        }
        if !r.mode.is_empty() {
            recipe_mode = Some(r.mode.to_string());
        }
    }

    // We own `-e` and `--data-rate`: native copies are stripped and ours goes first. A native
    // mode that differs from ours is refused; a native -e keeps its sub-options, under --mode's.
    let native_e = native_mode(&run.native);
    let (native_base, mut native_opts) = split_mode(native_e.as_deref());
    let mut rewrite = false;
    if exe == "dee_ddpjoc_encoder" {
        // ddpjoc has no phase-90 sub-option, so ddp's spelling of it sets the lever.
        if let Some(i) = native_opts
            .iter()
            .position(|o| o.starts_with("surround_90deg_phase_shift="))
        {
            let opt = native_opts.remove(i);
            let on = match opt.split_once('=').map_or("", |(_, v)| v) {
                "1" | "true" => "on",
                "0" | "false" => "off",
                _ => usage_error(
                    tool,
                    ErrorKind::InvalidValue,
                    format!("-e {opt}: expected 0 or 1"),
                ),
            };
            env.insert("DDPJOC_PHASE90".to_owned(), on.to_owned());
            rewrite = true;
        }
    }
    let ours = recipe.and_then(|r| r.mode.clone()).or(recipe_mode);
    let final_mode = if ours.is_some() || rewrite {
        let (base, own) = split_mode(ours.as_deref());
        let key = |o: &str| o.split('=').next().unwrap_or(o).to_owned();
        let own_keys: Vec<String> = own.iter().map(|o| key(o)).collect();
        if let (Some(b), Some(n)) = (base.as_deref(), native_base.as_deref()) {
            if b != n {
                let source = if recipe.and_then(|r| r.mode.as_ref()).is_some() {
                    "--mode".to_owned()
                } else {
                    format!(
                        "--layout {}",
                        recipe.and_then(|r| r.layout.as_deref()).unwrap_or("")
                    )
                };
                usage_error(
                    tool,
                    ErrorKind::ArgumentConflict,
                    format!("{source} runs mode {b} but the native -e names {n}; give one, or the same mode"),
                );
            }
        }
        let opts: Vec<String> = native_opts
            .into_iter()
            .filter(|o| !own_keys.contains(&key(o)))
            .chain(own)
            .collect();
        let parts: Vec<String> = base.or(native_base).into_iter().chain(opts).collect();
        (!parts.is_empty()).then(|| parts.join(":"))
    } else {
        None
    };

    // E-AC-3 uses the weighted JOC allocation for every total, without AC-3 rounding.
    let eac3_core = env.get("DDPJOC_CORE").map(String::as_str) == Some("eac3");
    let default_split = (eac3_core && split_arg.is_none()).then(|| {
        let total = bitrate.unwrap_or(1152);
        let (first, dependent) = rates::joc_eac3_split(total).unwrap_or_else(|error| {
            usage_error(tool, ErrorKind::InvalidValue, format!("{rate_flag} {total}: {error}"));
        });
        log::info!("E-AC-3 JOC total {total}: weighted split {first}:{dependent} (use --bitrate R0:R1 to choose)");
        (first, dependent)
    });
    let eff_mode = final_mode.clone().or_else(|| native_mode(&run.native));
    let mode = eff_mode.as_deref().map(rates::base_mode);
    let ddp_eac3 = exe == "dee_ddp_encoder"
        && mode == Some("ddp")
        && recipe
            .and_then(|r| r.layout.as_deref())
            .and_then(|name| recipes::find(exe, name))
            .is_some_and(|r| r.name.ends_with("/eac3"));
    let split = split_arg.or(default_split).map(|(r0, r1)| {
        let ac3_core = if exe == "dee_ddp_encoder" {
            if mode != Some("bluray") && !ddp_eac3 {
                usage_error(tool, ErrorKind::ArgumentConflict, format!("--bitrate {r0}:{r1} needs bluray mode or an extended /eac3 layout; see `dee_shim ddp_encoder --list`"));
            }
            !ddp_eac3
        } else {
            if mode != Some("bluray") {
                usage_error(tool, ErrorKind::ArgumentConflict, format!("--bitrate {r0}:{r1} needs bluray mode; streaming JOC has only one substream"));
            }
            !eac3_core
        };
        if ac3_core && !rates::contains(rates::AC3_CORE, r0) {
            let values: Vec<String> = rates::AC3_CORE.iter().map(u16::to_string).collect();
            usage_error(tool, ErrorKind::InvalidValue, format!("AC-3 core rate {r0} is not supported. Valid kbps: {}", values.join(" ")));
        }
        (r0, r1)
    });
    let mut native = strip_owned(
        &run.native,
        final_mode.is_some(),
        bitrate.is_some() || split.is_some(),
    );

    let layout_rates = recipe
        .and_then(|r| r.layout.as_deref())
        .filter(|_| eff_mode.as_deref().map(rates::base_mode) == Some("ddp"))
        .and_then(|layout| rates::layout_rates(exe, layout));
    let rate_mode = eff_mode.as_deref().or(if exe == "dee_ddpjoc_encoder" {
        Some("ddpjoc")
    } else {
        None
    });

    // Refuse a --bitrate the mode never accepts (at any input channel count), naming the valid
    // ones; the per-channel-count decision stays with the binary. The mode is ours, else the
    // native -e; with neither, the binary's default applies and nothing is checked.
    if let Some(k) = bitrate.filter(|_| split.is_none()) {
        if let Some(m) = rate_mode {
            let ok = layout_rates
                .map(<[u16]>::to_vec)
                .unwrap_or_else(|| rates::union_for_mode(exe, m));
            if !ok.is_empty() && !rates::contains(&ok, k) {
                let list: Vec<String> = ok.iter().map(|v| v.to_string()).collect();
                usage_error(
                    tool,
                    ErrorKind::InvalidValue,
                    format!("{rate_flag} {k} is not accepted in mode {}. Valid: {}\n\nSee `dee_shim {} --list` for the list per input channel count.",
                    rates::base_mode(m),
                    list.join(" "),
                    tool.name()),
                );
            }
        }
    }

    let mut owned: Vec<String> = Vec::new();
    if let Some(m) = &final_mode {
        owned.push("-e".to_string());
        owned.push(m.clone());
    }
    if let Some((r0, r1)) = split {
        let total = r0 + r1;
        let (var, dee_rate) = if exe == "dee_ddp_encoder" && !ddp_eac3 {
            // ddp Blu-ray takes the totals 768/1024/1280/1536/1664. DEE runs at the smallest that
            // covers the sum, so its output buffer fits; the hook codes the real substream rates.
            let totals = rates::union_for_mode(exe, "bluray");
            let dee = totals
                .iter()
                .copied()
                .find(|&t| u32::from(t) >= total)
                .map(u32::from)
                .unwrap_or_else(|| totals.last().map(|&t| u32::from(t)).unwrap_or(total));
            ("DDP_RATES", dee)
        } else if ddp_eac3 {
            ("DDP_RATES", 768)
        } else {
            (
                "DDPJOC_RATES",
                if rates::contains(BLURAY, total) {
                    total
                } else {
                    u32::from(BLURAY[0])
                },
            )
        };
        env.insert(var.to_string(), format!("{r0},{r1}"));
        owned.push("--data-rate".to_string());
        owned.push(dee_rate.to_string());
        if dee_rate != total {
            log::info!("DEE runs at --data-rate {dee_rate}; the stream is {r0} + {r1} = {total} kbps (DEE's own log reports {dee_rate})");
        }
    } else if let Some(k) = bitrate {
        if env.contains_key("DDP_DATA_RATE") {
            env.insert("DDP_DATA_RATE".to_string(), k.to_string());
        }
        let frontend_rate =
            if layout_rates.is_some() && !rates::contains(&rates::union_for_mode(exe, "ddp"), k) {
                768
            } else {
                k
            };
        owned.push("--data-rate".to_string());
        owned.push(frontend_rate.to_string());
        if frontend_rate != k {
            log::info!("DEE runs at --data-rate {frontend_rate}; the shim codes {k} kbps");
        }
    }

    let _compose = match tool.switches(&mut env) {
        Ok(template) => template,
        Err(error) => {
            log::error!("{error}");
            return 2;
        }
    };

    // --set last, so a raw lever wins over the recipe, --bitrate and the switches.
    for (k, v) in parse_set(&run.set) {
        env.insert(k, v);
    }
    // Every name and value is checked here, before the tool starts.
    let levers = Levers::from_pairs(&env).unwrap_or_else(|error| {
        usage_error(tool, ErrorKind::InvalidValue, error);
    });
    if exe == "dee_dthd_encoder" && (levers.dthd.target.is_some() || levers.dthd.compose.is_some())
    {
        if let Err(error) = paths::meter_rate_check(&native) {
            log::error!("{error}");
            return 2;
        }
    }

    let _job_temp = if exe == "dee_dthd_encoder" && !run.dry_run && !native_temp_requested(&native)
    {
        match tempfile::Builder::new().prefix("dee-shim-job-").tempdir() {
            Ok(directory) => {
                owned.push("--temp-dir".into());
                owned.push(directory.path().to_string_lossy().into_owned());
                Some(directory)
            }
            Err(error) => {
                log::error!("create TrueHD job directory: {error}");
                return 2;
            }
        }
    } else {
        None
    };

    owned.append(&mut native);
    let native = owned;
    let child_temp = if exe == "dee_dthd_encoder" {
        native_temp_dir(&native)
    } else {
        None
    };

    log::debug!("starting {exe}");

    if !host::available() {
        log::error!("this build of dee_shim cannot run the tool (it needs an x86_64 slice)");
        return 2;
    }
    in_process(run, exe, &native, &levers, child_temp)
}
