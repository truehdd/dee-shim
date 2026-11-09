# dee-shim

Run the Dolby Encoding Engine (DEE) command line encoders with controls their CLI does not
expose: rare channel layouts, higher data rates, an E-AC-3 core for Atmos, and object metadata
coded exactly as authored.

`dee_shim` never modifies a DEE file. It loads the untouched tool into its own process, patches
that private copy in memory, and runs it.

| Subcommand | DEE tool | Output | Platforms |
|---|---|---|---|
| `ddp_encoder` | `dee_ddp_encoder` | Dolby Digital Plus | macOS, Linux, Windows |
| `ddpjoc_encoder` | `dee_ddpjoc_encoder` | Dolby Digital Plus with Atmos (JOC) | macOS, Linux, Windows |
| `dthd_encoder` | `dee_dthd_encoder` | Dolby TrueHD | macOS, Linux¹, Windows |

¹ DEE 6.7 ships no Linux TrueHD encoder. On Linux, `dthd_encoder` runs the macOS encoder as built by
[dthd-mac2linux](https://github.com/truehdd/dthd-mac2linux) (tested with v0.1.1 and v0.2.1).

## Requirements

- Your own licensed copy of DEE. The supported version is currently 6.7. DEE executables and
  licences are not part of this repository or its releases; `dee_shim` leaves licensing to DEE.
- x86_64, as the DEE tools are. Apple silicon runs it under Rosetta 2.
- On Linux, glibc 2.34 or later, as DEE 6.7 needs.

## Quick start

```sh
cargo build --release -p dee_shim
cp target/release/dee_shim /path/to/dee/install/ # beside the DEE tools, so no --binary is needed
dee_shim ddpjoc_encoder --list                           # layouts and data rates
dee_shim ddpjoc_encoder --layout 7.1 -- --input-format atmos_mezz -i in.atmos -o out.ec3
```

On an Apple silicon Mac, build with `--target x86_64-apple-darwin`; the binary is then in
`target/x86_64-apple-darwin/release/`.

## Features

Every feature works the same on macOS, Linux and Windows.

#### `ddp_encoder` · Dolby Digital Plus

| Feature | Switch |
|---|---|
| 31 layouts from 1+1 to 7.1, each with its input order and data rates | `--layout`, `--list` |
| An E-AC-3 core in place of the AC-3 core for the extended layouts | `/eac3` |
| A data rate per substream | `--bitrate R0:R1` |
| The 5.1 core downmix of 7.0 and 7.1 | `--core-type` |
| DEE's encoder mode chosen outright, native `-e` sub-options kept | `--mode` |

#### `ddpjoc_encoder` · Dolby Digital Plus with Atmos

| Feature | Switch |
|---|---|
| 7.1 and 5.1.2-top backbones | `--layout`, `--list` |
| An E-AC-3 core in place of the AC-3 core | `/eac3` |
| A data rate per substream | `--bitrate R0:R1` |
| Surround phase-90 off, which DEE offers only for `ddp_encoder` | `--phase90` |
| Object metadata coded as authored | `--oamd-as-is` |

#### `dthd_encoder` · Dolby TrueHD

| Feature | Switch |
|---|---|
| Object metadata coded as authored | `--oamd-as-is` |
| Audio coded as read, sample for sample | `--as-is` |
| Noise shaping and apodising on or off | `--noise-shaping`, `--apodise` |
| DRC per presentation, `none` included | `--drc` |
| HD DVD-Video, which adds 44.1, 88.2 and 176.4 kHz | `--target hddvd-video` |
| DVD-A (FBB): 2ch at any rate, 6ch up to 96 kHz | `--target dvd-a` |
| No disc rules: any rate at 2, 6 or 8 channels | `--target non-conformant-fba` |
| Other speaker layouts per presentation | `--layout`, `--list` |
| A job authored from a JSON spec | `--compose` |
| Your own job file | `--mll` |

Every tool also takes raw levers (`--set NAME=VALUE`) and prints the command it would run
(`--dry-run`). DEE's own `-e` sub-options, such as the DRC profiles, keep working under the mode
a layout picks (see [Layouts and modes](#layouts-and-modes)).

`dee_shim <tool> --help` explains every switch and lever. `dee_shim ddp_encoder --list` and
`dee_shim ddpjoc_encoder --list` print the layouts, their input order and the accepted data rates;
`dee_shim dthd_encoder --list` prints the TrueHD layouts slot by slot.

## Usage

One subcommand per tool, named as the tool without `dee_`, then its switches, then DEE's own
arguments after `--`:

```sh
dee_shim ddp_encoder --layout 2.1 -- \
  -e ddp:drc_profile=music_light --input-format wav -i in.wav -o out.ec3

dee_shim ddpjoc_encoder --layout 7.1/eac3 --bitrate 384:640 -- \
  --input-format atmos_mezz -i in.atmos -o out.ec3

dee_shim dthd_encoder --oamd-as-is on --as-is on -- \
  --presentation atmos -f atmos_mezz -i in.atmos -o out.mlp
```

| Topic | Behaviour |
|---|---|
| Checks | Every lever is checked before the tool starts. An unknown name or bad value is a usage error (exit 2); a lever this build of the tool cannot apply in full stops the run (exit 3) before DEE writes anything. |
| Finding the tool | `--binary PATH`, or else the tool beside `dee_shim`. |
| Licence file | DEE reads the `license.lic` beside the tool; pass DEE's own `--license PATH` for another. |
| Paths | Resolved against the caller's working directory, which is never changed. |
| WAV input (`ddp_encoder`) | A multichannel WAV holds the layout's input slots in the order `--list` prints, with a silent channel for each `-` slot. DEE codes only the listed slots but measures loudness over every channel, so audio in a `-` slot changes the stream's dialnorm. `--input-format wav_list` takes one mono WAV per slot and `-` for an empty one. |
| Loudness in bluray mode | DEE refuses loudness correction there, so pass `--loudness-management measure_only` with the `ddp_encoder` layouts that have an AC-3 core. |
| Version | `-V` prints `vX.Y.Z+gHASH` at a release tag and `vX.Y.Z+N-gHASH` N commits after it. |

### Layouts and modes

A layout also chooses DEE's encoder mode (its `-e`), so no `-e` is needed:

| Tool | Layouts | DEE mode | Coded stream |
|---|---|---|---|
| `ddp_encoder` | up to 5.1 | `ddp` | one E-AC-3 independent substream |
| `ddp_encoder` | 5.1.2, 5.1.2-top, 6.1, 7.1 and their variants without LFE | `bluray` | AC-3 core plus E-AC-3 dependent |
| `ddp_encoder` | the same with `/eac3` | `ddp` | E-AC-3 independent plus dependent |
| `ddpjoc_encoder` | 5.1 | `ddpjoc` | one E-AC-3 independent substream with JOC |
| `ddpjoc_encoder` | 5.1.2, 5.1.2-top, 7.1 | `bluray` | AC-3 core plus E-AC-3 dependent with JOC |
| `ddpjoc_encoder` | the same with `/eac3` | `bluray` | E-AC-3 independent plus dependent with JOC |

DEE has no mode that codes these extended layouts with an E-AC-3 core. The `/eac3` layouts are
dee_shim's own: it runs DEE in the mode shown and replaces the core, so the mode a layout uses
does not name the stream it produces.

DEE's own `-e` may still be given. Its sub-options are kept, so `-e ddp:drc_profile=none` works with
any `ddp_encoder` layout; a mode named in it must match the layout's mode or `--mode`, or dee_shim
refuses the run. `--mode` overrides the layout's mode outright; sub-options given with it win over
those of a native `-e`. A native `--data-rate` is handled exactly as `--bitrate`; giving both with
different totals is refused.
A layout's own mode is the tested one; another mode can ask for something the encoder refuses.
`dee_ddp_encoder` also has two modes its documentation omits, `bluray` and `bluray_secondary` (one E-AC-3
independent substream, at most 256 kbps). `--dry-run` prints the `-e` that DEE will get.

### TrueHD layouts

The CLI's 2ch, 6ch and 8ch presentations are 2.0, 5.1 and 7.1, read by position (L R C LFE Ls
Rs Lrs Rrs), and `--presentation atmos` takes an Atmos input; none of these needs `--layout`.
For other speakers, `--layout [TIER=]NAME` renames a presentation's slots, where NAME is a
layout from `dee_shim dthd_encoder --list` or a comma list of slot labels; `--help` has the
details. The downmix presentations are mixed again with DEE's own coefficients.

```sh
# 5.1.2: front heights in slots 7 and 8
dee_shim dthd_encoder --layout 5.1.2 -- \
  --presentation 8ch --input-format wav -i in.wav -o out.mlp

# an Atmos input whose 8ch presentation is rendered as 5.1.2 instead of 7.1
dee_shim dthd_encoder --layout 8ch=5.1.2 -- \
  --presentation atmos --input-format atmos_mezz -i in.atmos -o out.mlp
```

### Tips

**Custom dialnorm.** DEE sets it natively, and dee_shim passes it through: give
`--loudness-management skip:dialnorm=N` after the `--`, with N from -31 to -1. Only
`dee_dthd_encoder` documents this mode, but `dee_ddp_encoder` and `dee_ddpjoc_encoder` accept it
too, with every layout including the `/eac3` ones; each substream carries the value. `skip` turns
loudness measurement off, so N is coded as given.

```sh
dee_shim ddpjoc_encoder --layout 7.1 -- \
  --loudness-management skip:dialnorm=-20 \
  --input-format atmos_mezz -i in.atmos -o out.ec3
```

## How it works

`dee_shim` starts a child copy of itself that maps the DEE tool into its own address space
(`dlopen` on macOS, its own ELF loader on Linux, `LoadLibraryExW` on Windows), links it, applies
the levers, and calls the tool's entry point. The levers travel to the child as one value that
the tool's environment never sees.

The engine keeps *what a control does* apart from *where it is in one binary*:

- **Modules** (`crates/dshim/src/modules/`) implement each control and name the code points it
  touches by stable names.
- **Profiles** (`crates/dee67/src/`) give each point's address and expected bytes in one build,
  pinned by the binary's identity.

Every site is byte-checked before anything is written, so a control the build does not support
is refused rather than producing a wrong stream.

| Crate | Role |
|---|---|
| `crates/run` | the `dee_shim` CLI and the in-process host |
| `crates/dshim` | the engine: modules, site resolution, detours |
| `crates/levers` | the typed levers, their names, checks and encoding |
| `crates/profiles` | layout recipes, rate tables, the profile list |
| `crates/dee67` | the DEE 6.7 profiles, one per tool and OS |
| `crates/profile-model` | the profile types |
| `crates/image` | executable identity (ELF, Mach-O, PE) |
| `crates/log` | log filters, formats and outputs |

## Building

Rust 1.90 or later. On Linux use rustup's toolchain: its rust-lld link is required, as a
`dee_shim` linked by the system GNU linker fails to load.

```sh
cargo build --locked --release -p dee_shim                                # Linux, Windows
cargo build --locked --release -p dee_shim --target x86_64-apple-darwin   # macOS
```

`-V` takes its version from the nearest `v*` tag, so build from a clone with its tags. A build
without Git history, such as from a source archive, prints `vX.Y.Z+unknown` with the version in
`Cargo.toml`, which each release tag matches.

## Releases

What changed in each release is in [CHANGELOG.md](CHANGELOG.md); its section for a tag is that
release's notes. To release, set the version in `Cargo.toml`, write its section, and run the
Release workflow on master from the Actions tab: it tags the commit `vX.Y.Z`, builds the three
packages and attaches them to a draft release to publish by hand.

## License

Apache License 2.0; see [LICENSE](LICENSE) and [NOTICE](NOTICE). Third-party crates keep their
own licences, listed in [THIRD_PARTY_LICENSES.txt](THIRD_PARTY_LICENSES.txt).
