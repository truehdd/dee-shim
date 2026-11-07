//! Site profiles for DEE 6.7 builds, one module per (tool, os). The profile types come from
//! `dshim-profile-model`; `dshim-profiles` lists these profiles in its `ALL` table.

// Not every build selects a profile that needs each of these.
#[allow(unused_imports)]
use dshim_profile_model::{Ident, Kind, Os, Profile, Site};

#[cfg(all(feature = "ddp", feature = "dee_6_7"))]
pub mod ddp_linux_6_7_0;
#[cfg(all(feature = "ddp", feature = "dee_6_7"))]
pub mod ddp_macos_6_7_0;
#[cfg(all(feature = "ddp", feature = "dee_6_7"))]
pub mod ddp_windows_6_7_0;
#[cfg(all(feature = "ddpjoc", feature = "dee_6_7"))]
pub mod ddpjoc_linux_6_7_0;
#[cfg(all(feature = "ddpjoc", feature = "dee_6_7"))]
pub mod ddpjoc_macos_6_7_0;
#[cfg(all(feature = "ddpjoc", feature = "dee_6_7"))]
pub mod ddpjoc_windows_6_7_0;
#[cfg(all(feature = "dthd", feature = "dee_6_7"))]
pub mod dthd_linux_port_6_7_0;
#[cfg(all(feature = "dthd", feature = "dee_6_7"))]
pub mod dthd_macos_6_7_0;
#[cfg(all(feature = "dthd", feature = "dee_6_7"))]
pub mod dthd_windows_6_7_0;
