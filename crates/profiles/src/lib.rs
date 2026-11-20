//! The site profiles of the supported DEE builds (currently 6.7), looked up by binary identity,
//! plus the layout recipes and rate tables. The profile types are in dshim-profile-model.

pub mod dthd_layouts;
pub mod rates;
pub mod recipes;

pub use dshim_profile_model::{hex, le32, Ident, Kind, Os, Profile, Site};

/// Every compiled profile. Each comes from the dshim-dee67 crate, one module per (tool, os), and
/// is compiled only when both its tool feature and dee_6_7 are selected.
pub static ALL: &[&Profile] = &[
    #[cfg(all(feature = "ddp", feature = "dee_6_7"))]
    &dshim_dee67::ddp_linux_6_7_0::PROFILE,
    #[cfg(all(feature = "ddp", feature = "dee_6_7"))]
    &dshim_dee67::ddp_macos_6_7_0::PROFILE,
    #[cfg(all(feature = "ddp", feature = "dee_6_7"))]
    &dshim_dee67::ddp_windows_6_7_0::PROFILE,
    #[cfg(all(feature = "ddpjoc", feature = "dee_6_7"))]
    &dshim_dee67::ddpjoc_macos_6_7_0::PROFILE,
    #[cfg(all(feature = "ddpjoc", feature = "dee_6_7"))]
    &dshim_dee67::ddpjoc_windows_6_7_0::PROFILE,
    #[cfg(all(feature = "ddpjoc", feature = "dee_6_7"))]
    &dshim_dee67::ddpjoc_linux_6_7_0::PROFILE,
    #[cfg(all(feature = "dthd", feature = "dee_6_7"))]
    &dshim_dee67::dthd_macos_6_7_0::PROFILE,
    #[cfg(all(feature = "dthd", feature = "dee_6_7"))]
    &dshim_dee67::dthd_linux_port_6_7_0::PROFILE,
    #[cfg(all(feature = "dthd", feature = "dee_6_7"))]
    &dshim_dee67::dthd_windows_6_7_0::PROFILE,
    #[cfg(all(feature = "ddpdec", feature = "dee_6_7"))]
    &dshim_dee67::ddpdec_linux_6_7_0::PROFILE,
    #[cfg(all(feature = "ddpdec", feature = "dee_6_7"))]
    &dshim_dee67::ddpdec_macos_6_7_0::PROFILE,
    #[cfg(all(feature = "ddpdec", feature = "dee_6_7"))]
    &dshim_dee67::ddpdec_windows_6_7_0::PROFILE,
    #[cfg(all(feature = "loudness", feature = "dee_6_7"))]
    &dshim_dee67::loudness_linux_6_7_0::PROFILE,
    #[cfg(all(feature = "loudness", feature = "dee_6_7"))]
    &dshim_dee67::loudness_macos_6_7_0::PROFILE,
    #[cfg(all(feature = "loudness", feature = "dee_6_7"))]
    &dshim_dee67::loudness_windows_6_7_0::PROFILE,
];

/// The profile whose identity matches `key` (normalised hex, see `Ident::key`).
pub fn by_ident(key: &str) -> Option<&'static Profile> {
    ALL.iter().copied().find(|p| p.ident.key() == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_sites_keep_their_length() {
        for p in ALL {
            for s in p.sites {
                if let Kind::Patch { new } = s.kind {
                    assert_eq!(
                        hex(new).len(),
                        hex(s.expect).len(),
                        "{} {}: new and expect differ in length",
                        p.label(),
                        s.name
                    );
                }
            }
        }
    }
}
