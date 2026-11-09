# Changelog

Each release has a section headed by its tag. The release workflow publishes that section as the
release notes and refuses to run without it, so write it before running the workflow.

## Unreleased

- A native `--data-rate` for `ddp_encoder` and `ddpjoc_encoder` is now handled as `--bitrate`:
  it is checked against the mode's rates and drives the layout's rate handling. Before, a
  layout could code its default rate in its place without a message. `--bitrate` and a native
  `--data-rate` with different totals are refused.
- A native `-e` whose mode differs from the layout's or `--mode` is refused; before, it was
  replaced.

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
