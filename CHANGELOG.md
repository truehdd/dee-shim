# Changelog

Each release has a section headed by its tag. The release workflow publishes that section as the
release notes and refuses to run without it, so write it before running the workflow.

## v0.5.0

- `--unpadded-wav` is now `--unpadded`; the old name still works. In `ddp_encoder` it also takes
  raw PCM with only the layout's channels (`pcm_s24le:channels=4:...` for 4.0), from a file or
  through `named_pipe://`. The result is the same as from the padded input.
- `dthd_encoder` refuses `-o named_pipe://...`. DEE cannot write TrueHD to a pipe: it made a
  directory of that name, wrote the stream there and reported success.
- Fixed: with `ddpjoc_encoder --layout 7.1` and `5.1.2-top` (also `/eac3`), the 5.1 core's Ls
  and Rs were mixed from the wrong channels (L and R plus the rear or top pair), and on 7.1
  they were 1280 samples late. They now hold the surrounds with the rear or top pair folded
  in, in time. A decoder that reads the whole stream was not affected (#9).
- A native `-e` that names a mode now overrides the layout's mode, as `--mode` does: `--layout
  4.0 -- -e dd` is `--layout 4.0 --mode dd`. It was refused before (#7).

## v0.4.0

- New `--unpadded-wav` for `ddp_encoder` and `dthd_encoder`: with `--layout`, the WAV holds only
  the layout's channels, in order, instead of one channel per input slot: four channels for
  4.0, where DEE wants six or eight with silent ones. The result is the same as from the padded
  file (#7).

## v0.3.0

- `ddp_encoder`: the help, `--list` and the README now say that `--mode dd` codes every layout
  up to 5.1 as Dolby Digital (AC-3). `--mode` keeps the layout's sub-options; before,
  `--layout 5.0 --mode dd` came out with an LFE channel. `--mode dd` with an extended layout
  is refused; before, DEE mixed 7.1 down to 5.1 without a message and failed on the others
  with a parameter error.
- New `ddp_decoder` subcommand for `dee_ddp_decoder`. `--layout native` writes every channel
  the stream codes, so a 7.1 stream decodes to eight channels; DEE's own `-c` has 5.1, stereo
  and mono (#6).
- New `measure_loudness` subcommand for `dee_measure_loudness`. `--layout` measures the layouts
  DEE's meter has but its CLI refuses: 7.1, 5.1.4, 7.1.4, 9.1.6, and 3.0, 3.0-back, 4.0,
  4.0-quad and 5.0, which the result reports as `multichannel_5_1` (#6).

## v0.2.1

- `ddpjoc_encoder` and `dthd_encoder` on Windows: a zero-fill in DEE that AMD Zen 3 runs about
  45 times slower than Intel is replaced on every run, with the same result. Before, both
  encoders were 2.5 to 4 times slower there. `DDPJOC_ZERO_FILL_ORIGINAL=1` and
  `DTHD_ZERO_FILL_ORIGINAL=1` keep DEE's own code.

## v0.2.0

- A native `--data-rate` for `ddp_encoder` and `ddpjoc_encoder` is now handled as `--bitrate`:
  it is checked against the mode's rates and drives the layout's rate handling. Before, a
  layout could code its default rate in its place without a message. `--bitrate` and a native
  `--data-rate` with different totals are refused.
- A native `-e` whose mode differs from the layout's or `--mode` is refused; before, it was
  replaced.
- `dthd_encoder --oamd-as-is`: an object below DEE's silence threshold now gets its own cluster
  when the frame fits. Before, it was mixed into a neighbour's cluster and both came back 1 LSB
  off (#4).
- `--oamd-as-is`, for `dthd_encoder` and `ddpjoc_encoder`: a master whose elements fit is coded
  in master order: bed channels other than LFE that sound from the start, then the objects.
  Before, the order was the one DEE's clusterer first picked them in. `--oamd-keep-order off`
  keeps DEE's order.
- `--oamd-as-is`, for `dthd_encoder` and `ddpjoc_encoder`: each frame's position update is
  coded at its sample in the master, with its ramp. Before, DEE moved it to the start of its
  1536-sample frame, up to 32 ms early, with a 1536-sample ramp (#3). `--oamd-update-times off`
  keeps DEE's timing.
- `dthd_encoder --as-is`: the stream ends at the master's last sample. Before, DEE padded the
  master with silence to a whole 512-sample block. `--audio-exact-length off` keeps DEE's end.
- A DEE URI such as `named_pipe://...` given to `-i`, `-o` or another file option is passed
  through unchanged, also inside a `wav_list`. Before, the caller's directory was put in front
  of it and DEE could not open it.

## v0.1.0

First release, for DEE 6.7 on macOS, Linux and Windows.

- `ddp_encoder`: 31 layouts from 1+1 to 7.1, an E-AC-3 core for the extended layouts (`/eac3`),
  a data rate per substream, the 5.1 core downmix of 7.0 and 7.1, and DEE's encoder mode chosen
  outright.
- `ddpjoc_encoder`: 7.1 and 5.1.2-top backbones, an E-AC-3 core, a data rate per substream,
  surround phase-90 off, and object metadata coded as authored.
- `dthd_encoder`: object metadata and audio coded as authored, noise shaping and apodising on or
  off, DRC per presentation, the HD DVD-Video, DVD-A and non-conformant targets, layouts per
  presentation including the Atmos 8ch render, jobs from a JSON spec, and your own job file.
