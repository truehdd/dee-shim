//! Operator help for each DEE tool.

use clap::builder::{PossibleValue, PossibleValuesParser};
use clap::Arg;
use dshim_profiles::recipes;

pub fn layout_values(tool: &str) -> PossibleValuesParser {
    // --list describes each layout; the help lists the names only.
    PossibleValuesParser::new(
        recipes::for_tool(tool)
            .into_iter()
            .map(|recipe| PossibleValue::new(recipe.name)),
    )
}

pub fn layout_arg(arg: Arg, tool: &str) -> Arg {
    let description = if tool == "dee_ddpjoc_encoder" {
        "Coded backbone: 5.1 streaming, or an extended layout (see --list)"
    } else {
        "Channel layout without JOC metadata (see --list)"
    };
    arg.help(description)
        .long_help(description)
        .value_parser(layout_values(tool))
}

pub fn mode_arg(arg: Arg, tool: &str) -> Arg {
    let (short, long) = if tool == "dee_ddpjoc_encoder" {
        (
            "Native mode: ddpjoc (streaming, default) or bluray (extended backbone)",
            "Native -e mode: ddpjoc for streaming, bluray for extended backbones. A layout supplies \
             its mode and --mode overrides it. A native -e must name the same mode or none; its \
             sub-options are kept. The \
             /eac3 layouts run with bluray, whose AC-3 core dee_shim replaces. Sub-options pass \
             through, e.g. bluray:drc_profile_line=none.",
        )
    } else {
        (
            "Native mode: dd, ddp, bluray or bluray_secondary",
            "Overrides the layout's mode. A native -e must name the same mode or none; its sub-options \
             are kept. \
             Sub-options pass through, e.g. \
             ddp:lfe_on=0. The recipe's mode is the tested one; an override may be incompatible \
             with its channel layout.",
        )
    };
    arg.help(short).long_help(long)
}

pub fn bitrate_arg(arg: Arg, tool: &str) -> Arg {
    let total = if tool == "dee_ddpjoc_encoder" {
        "A total is on the mode's list, or for /eac3 any whole kbps 64..2048, split by weighted \
         allocation."
    } else {
        "A total is on the mode's list, or for /eac3 on the layout's allocation list."
    };
    arg.help("Total kbps, or R0:R1 (core/independent:dependent)")
        .long_help(format!(
            "{total} R0:R1 sets the two coded rates of an extended layout: an AC-3 core takes \
             224..640 on its own list, E-AC-3 any whole kbps 32..1024 per substream, subject to \
             layout and metadata budgets. See --list for the accepted rates."
        ))
}

pub const DDP_HELP: &str = "\
Encode Dolby Digital Plus (dee_ddp_encoder).

--layout picks a named channel layout; `dee_shim ddp_encoder --list` shows each layout's input slots,
mode and the accepted data rates. These layouts carry channel audio without JOC metadata.

Extended layouts are 5.1.2 (front heights), 5.1.2-top (top surrounds), 6.1 and 7.1, and their
variants without LFE, 5.0.2, 5.0.2-top, 6.0 and 7.0. Each codes an AC-3 core plus an E-AC-3
dependent substream in bluray mode. Append /eac3 for an E-AC-3 independent substream in place
of the AC-3 core, in ddp mode.

MODE
  A layout supplies the binary's -e, so no -e is needed; --mode overrides it. Besides the
  documented dd and ddp, the binary has two hidden modes: bluray (AC-3 core plus E-AC-3
  dependent) and bluray_secondary (one E-AC-3 independent substream, at most 256 kbps). An
  override can ask for what the encoder refuses (6.1 in bluray_secondary fails: its acmod is
  invalid there). Sub-options pass through, as in --mode ddp:lfe_on=0.

BITRATE
  --bitrate takes a total, or R0:R1 (core or independent : dependent) for an extended layout.
  A total must be on the mode's list; for /eac3 the layout's allocation splits it. R0:R1 sets
  each substream directly, with either core. The check uses the input channel count, before
  any layout change. `dee_shim ddp_encoder --list` gives the accepted rates.

CORE
  --core-type standard|pliix selects the 5.1 core downmix of 7.0 and 7.1 (also /eac3); DEE's
  default is pliix. It changes the coded core audio, not the stereo downmix metadata.

NATIVE OPTIONS
  DRC profiles, the surround phase-90 shift and the 3 dB surround attenuation are DEE's own -e
  sub-options (drc_profile, surround_90deg_phase_shift, surround_3dB_attenuation). They keep
  working under the mode a layout picks:
    dee_shim ddp_encoder --layout 7.1 -- -e ddp:drc_profile=none:surround_90deg_phase_shift=0 ...

INPUT
  The binary's own input and output options pass through after --:
    dee_shim ddp_encoder --layout 6.1/eac3 --bitrate 1024 -- --input-format wav -i in.wav -o out.ec3
  A multichannel WAV holds the layout's input slots in order (see --list), with a silent
  channel for each '-' slot: DEE codes only the listed slots but measures loudness over every
  channel, so audio in a '-' slot changes the stream's dialnorm. With --input-format wav_list
  give one mono WAV per slot and '-' for an empty one.

LEVERS (--set KEY=VAL; the switches above set the others)
  DDP_ACMOD=<0..28>           core parameter 4, the layout (acmod) code --layout picks
  DDP_DATA_RATE=<kbps>        the data rate the encoder codes at
  DDP_SET_PARAMS=<id=v,...>   set or add core parameters (5=1 forces the LFE on)
  DDP_DOWNMIX_MODE=<n>        front-end downmix mode for the encode pass (4 is off)
  DDP_LFE_ROUTE=1             route the LFE for codes 0..2 and lift the error 137 checks
  DDP_EMDF_OFF=1              bypass the EMDF data rate gate (experimental)
  With DDP_EMDF_OFF=1, DDP_DATA_RATE has coded 3024 kbps in plain DD+ with three-block frames,
  to which the six-block per-substream limits do not apply. It lifts the metadata gate, not the
  --bitrate check.
  Example: dee_shim ddp_encoder --set DDP_ACMOD=28 -- -e bluray --loudness-management measure_only
    --input-format wav -i in.wav -o out.ec3";

pub const DDPJOC_HELP: &str = "\
Encode Dolby Digital Plus with Atmos (dee_ddpjoc_encoder).

--layout picks the coded backbone; `dee_shim ddpjoc_encoder --list` shows each layout's channels, mode
and the accepted data rates. Every layout carries JOC metadata.

LAYOUT AND MODE
  5.1 uses ddpjoc mode, DEE's default: one E-AC-3 independent substream with JOC (streaming).
  5.1.2 (front heights), 5.1.2-top (top surrounds) and 7.1 use bluray mode: an AC-3 core plus
  an E-AC-3 dependent substream with JOC. Append /eac3 for an E-AC-3 independent substream in
  place of the AC-3 core, as with ddp's /eac3 layouts. DEE has no such mode: dee_shim runs
  these with -e bluray and replaces the core. A layout supplies -e;
  --mode overrides it.

BITRATE
  --bitrate takes a total, or R0:R1 (core or independent : dependent) for an extended layout;
  streaming has one substream. A total must be on the mode's list, except with /eac3, which
  takes any whole kbps total and splits it by weighted allocation after the JOC and framing
  reservation: the dependent gets the reserved bits and the remainder, and neither substream
  exceeds 1024. Without --bitrate, /eac3 uses 1152. `dee_shim ddpjoc_encoder --list` gives the
  accepted rates.

INPUT
  Every layout takes an Atmos master; the channels name the coded backbone, not the master's
  bed or the Atmos render.
    dee_shim ddpjoc_encoder --layout 7.1/eac3 --bitrate 512:512 -- \\
        --input-format atmos_mezz -i in.atmos -o out.ec3

OBJECT METADATA, DRC AND PHASE-90
  --oamd-as-is codes an already prepared master's object metadata as it is. DRC profiles are DEE's own -e sub-options (drc_profile, drc_profile_line,
  drc_profile_rf) and keep working under the mode a layout picks; none still writes overload
  protection. ddpjoc has no phase-90 sub-option: --phase90, or ddp's
  surround_90deg_phase_shift=0 given in -e, turns the filter off through dee_shim.";

pub const DTHD_HELP: &str = "\
Encode Dolby TrueHD with Atmos (dee_dthd_encoder).

Give DEE's own arguments after --. Extended position precision is DEE's own
--presentation atmos:legacy_authoring=0.

--oamd-as-is codes an already clustered master's object metadata as it is, and --as-is hands
the TrueHD core the samples as they were read. With both on, a master whose objects fit in the
elements decodes sample for sample:
  dee_shim dthd_encoder --oamd-as-is on --as-is on -- \\
      --presentation atmos:number_of_elements=16:legacy_authoring=0 \\
      --input-format atmos_mezz -i in.atmos -o out.mlp

LAYOUT
  The CLI reads a presentation's input by position: WAV channel k, or wav_list entry k, is
  slot k, whatever the WAV's channel mask says, and a - entry in a wav_list is a silent slot.
  It names the slots L R (2ch), L R C LFE Ls Rs (6ch), or those and Lrs Rrs (8ch).
  --layout [TIER=]NAME renames one presentation's slots, - leaving a slot out; NAME is a layout
  from --list or a comma list of slot labels, and without TIER it is for the top presentation.
  Repeat it for another presentation. A downmix presentation takes its outputs from the layout
  and is mixed again from the presentation above it with DEE's own coefficients. The engine
  judges the result against the disc type.
    dee_shim dthd_encoder --layout 8ch=7.1-wide -- --presentation 8ch --input-format wav \\
        -i in.wav -o out.mlp
  1.0 in a 2ch presentation takes slot 1 and is coded as Mono; with --target dvd-a it becomes
  a lone centre.
    dee_shim dthd_encoder --layout 2ch=1.0 -- --presentation 2ch --input-format wav_list \\
        -i mono.wav:- -o out.mlp
  Each --presentation given its own input is an independent presentation. The CLI stops at two;
  dee_shim lifts that, so 8ch, 6ch and 2ch can each have their own.
  With an Atmos input, the engine renders the 8ch presentation from the objects into the
  speakers its layout names: 5.1 plus Lrs Rrs, Lvh Rvh, Lts Rts, Lw Rw, Lc Rc or Cs (7.1,
  5.1.2, 5.1.2-top, 7.1-wide, 7.1-center, 6.1).
    dee_shim dthd_encoder --layout 8ch=5.1.2 -- --presentation atmos --input-format atmos_mezz \\
        -i in.atmos -o out.mlp

JOB
  --target and --layout edit the job the CLI builds, which covers most needs. --compose authors
  the whole job from a JSON spec, for presentations the CLI cannot compose; a channel takes its
  audio from a file, or from a slot of the CLI input after -- (\"slot\": 3, or \"slot\":
  \"6ch:3\" for another presentation's input). --mll FILE has the engine read your own job
  file, reaching parameters only a .mll can hold. --mll excludes the others, and --compose
  excludes --target and --layout; the other switches apply to every job.

SAMPLE RATE
  The CLI takes 48 and 96 kHz input. With --target it takes any rate the disc type allows:
  BluRay 48, 96 and 192 kHz; HD DVD-Video also 44.1, 88.2 and 176.4 kHz. DVD-A takes every
  rate as FBB (MLP): 2ch at any rate, 6ch up to 96 kHz.
    dee_shim dthd_encoder --target hddvd-video -- --presentation 6ch --input-format wav \\
        -i in88200.wav -o out.mlp
  DEE measures loudness at 48 kHz through a resampler that has no 176.4 kHz to 48 kHz pair, so
  176.4 kHz needs the dialogue level given: --loudness-management skip:dialnorm=-24.
  --target non-conformant-fba drops every disc rule and keeps TrueHD's own: any of the six rates
  at 2, 6 or 8 channels, within the 18 Mbps peak data rate, so also more channels at 176.4 and
  192 kHz than BluRay (six) or HD DVD-Video (two) allow. The stream fits no disc, and DEE
  says so in a warning.
    dee_shim dthd_encoder --target non-conformant-fba --apodise off -- \\
        --presentation 8ch --input-format wav -i in192000.wav -o out.mlp \\
        --loudness-management skip:dialnorm=-24
  DEE apodises 96 kHz input (and no other rate), a filter that changes the coded samples;
  --apodise off turns it off, so 96 kHz decodes sample for sample, and --apodise on applies it
  at 88.2, 176.4 and 192 kHz too (DEE ignores it at 44.1 and 48 kHz).";

pub const DTHD_KEEP_QUIET: &str = "\
When the objects fit, keep quiet ones in the clusterer

When every object with any weight fits, let the quiet ones (weight under 0.5, about -90 dBFS) \
reach the clusterer, and mix every object whose block is not all zero. DEE gives a quiet object \
a leftover position, and leaves out of the mix a block it judges silent, which drops objects at \
dither level";
