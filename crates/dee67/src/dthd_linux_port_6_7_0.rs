//! TrueHD 6.7.0 on Linux, as built by dthd-mac2linux (https://github.com/truehdd/dthd-mac2linux)
//! from the macOS x86_64 encoder. The port maps the unchanged macOS code at its Mach-O virtual
//! addresses, so it uses the macOS sites. The GNU build-id pins the v0.1.1 build; other builds of
//! the same encoder, such as v0.2.1, are adopted by the signature fallback when every site matches.

use crate::{Ident, Os, Profile};

pub static PROFILE: Profile = Profile {
    tool: "dee_dthd_encoder",
    os: Os::Linux,
    version: "6.7.0 port",
    ident: Ident::GnuBuildId("a204b9fa1cd5bdc8a019d5489a408a74a640d98a"),
    sites: super::dthd_macos_6_7_0::SITES,
};
