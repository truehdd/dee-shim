//! Select the 7.0/7.1 to 5.0/5.1 core downmix in DD+.

use crate::site::Resolver;
use crate::write_code;
use dshim_levers::CoreType;
use dshim_profiles::Kind;
use std::io::Write;

pub fn install(res: &Resolver, log: &mut dyn Write) {
    let Some(value) = crate::levers().ddp.core_type else {
        return;
    };
    let selector = match value {
        CoreType::Pliix => 0,
        CoreType::Standard => 1,
    };

    let Some(site) = res.site("ddp.core.surround_downmix_type", log) else {
        lerr!(
            log,
            "core_type",
            "5.1 core type is unavailable on this build"
        );
        let _ = log.flush();
        std::process::exit(3);
    };
    let Kind::Patch { new } = site.kind else {
        lerr!(log, "core_type", "invalid surround downmix patch site");
        let _ = log.flush();
        std::process::exit(3);
    };

    // Each profile replaces the setter with mov dword [context+24], selector; ret.
    let mut bytes = dshim_profiles::hex(new);
    bytes[3] = selector;
    if !write_code(site.addr, &bytes) {
        lerr!(log, "core_type", "could not set 5.1 core type");
        let _ = log.flush();
        std::process::exit(3);
    }

    lok!(log, "core_type", "5.1 core type: {}", value.name());
}
