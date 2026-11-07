//! Input sample rates for dee_dthd_encoder beyond the CLI's own list.
//!
//! The CLI accepts 48 and 96 kHz input and refuses every other rate before it builds the job,
//! although the engine judges the rate against the disc type (BluRay refuses the 44.1 kHz family,
//! HD DVD-Video and DVD-A take it). With a target the CLI's check is lifted, so the engine decides.
//!
//! Past that check, the CLI's restreamer feeds the loudness meter through a resampler set up for
//! 96 and 192 kHz input only. The stub replaces that choice with the resampler's own pairs:
//! 44.1, 88.2, 96 and 192 kHz to 48 kHz, and 176.4 kHz to 44.1 kHz, the one 176.4 kHz pair the
//! resampler has. Any other rate still reaches DEE's refusal. The coded audio does not pass
//! through this resampler.
//!
//! Active with DTHD_TARGET or DTHD_COMPOSE (whose CLI feed is a dummy at any rate). Sites:
//!   dthd.input.rate_*   the CLI's lookups in its rate list, patched to take their found exit.
//!   dthd.restream.meter_rate
//!                       overwritten with a register-free jump to the stub, which stores the
//!                       resampler's input and output rate ids and continues at
//!                       dthd.restream.meter_resume, or at dthd.restream.meter_refuse for any
//!                       other rate.

use crate::site::Resolver;
use crate::write_code;
use dshim_profiles::{hex, Kind};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The CLI's rate lookups. The first two are in every profile; the third only where the wav_list
/// check has a second loop.
const CHECK_SITES: [(&str, bool); 3] = [
    ("dthd.input.rate_single", true),
    ("dthd.input.rate_list", true),
    ("dthd.input.rate_list_unnamed", false),
];

/// Where the stub continues: past the original rate choice, or into DEE's refusal.
static RESUME: AtomicUsize = AtomicUsize::new(0);
static REFUSE: AtomicUsize = AtomicUsize::new(0);

// rax holds the input rate, which DEE's refusal also reads. The resampler takes rate ids: 7 is
// 44.1, 11 is 48, 13 is 88.2, 14 is 96, 16 is 176.4 and 17 is 192 kHz. The stub stores the input
// and output ids where the original choice does, a frame slot that differs by build.
macro_rules! meter_rate_stub {
    ($name:literal, $input:literal, $output:literal) => {
        std::arch::global_asm!(
            concat!(".globl ", $name),
            concat!($name, ":"),
            "mov edx, 11",
            "mov ecx, 14",
            "cmp rax, 96000",
            "je 2f",
            "mov ecx, 17",
            "cmp rax, 192000",
            "je 2f",
            "mov ecx, 7",
            "cmp rax, 44100",
            "je 2f",
            "mov ecx, 13",
            "cmp rax, 88200",
            "je 2f",
            "mov ecx, 16",
            "mov edx, 7",
            "cmp rax, 176400",
            "je 2f",
            "jmp qword ptr [rip + {refuse}]",
            "2:",
            concat!("mov dword ptr [rbp ", $input, "], ecx"),
            concat!("mov dword ptr [rbp ", $output, "], edx"),
            "jmp qword ptr [rip + {resume}]",
            resume = sym RESUME,
            refuse = sym REFUSE,
        );
    };
}

#[cfg(target_os = "macos")]
meter_rate_stub!("_dshim_dthd_meter_rate_stub", "- 0x50", "- 0x4c");
#[cfg(windows)]
meter_rate_stub!("dshim_dthd_meter_rate_stub", "+ 0x50", "+ 0x54");
#[cfg(all(not(target_os = "macos"), not(windows)))]
meter_rate_stub!("dshim_dthd_meter_rate_stub", "- 0x50", "- 0x4c");

extern "C" {
    fn dshim_dthd_meter_rate_stub();
}

fn fail(msg: &str, log: &mut dyn Write) -> ! {
    lerr!(log, "dthd", "rate: {msg}, stopping");
    let _ = log.flush();
    std::process::exit(3);
}

pub fn install(res: &Resolver, log: &mut dyn Write) {
    let dthd = &crate::levers().dthd;
    if dthd.target.is_none() && dthd.compose.is_none() {
        return;
    }
    for (name, required) in CHECK_SITES {
        let Some(r) = res.site(name, log) else {
            if required {
                fail(
                    &format!("the rate lift needs {name}, not in this profile"),
                    log,
                );
            }
            continue;
        };
        let Kind::Patch { new } = r.kind else {
            fail(&format!("{name} is not a patch site"), log);
        };
        if !write_code(r.addr, &hex(new)) {
            fail(&format!("{name} write failed"), log);
        }
        lok!(log, "dthd", "rate: CLI check, patch {name} @ {:#x}", r.addr);
    }
    meter_rate(res, log);
}

fn meter_rate(res: &Resolver, log: &mut dyn Write) {
    let mut addr = |name: &str| match res.site(name, log) {
        Some(r) => r.addr,
        None => fail(
            &format!("the rate lift needs {name}, not in this profile"),
            log,
        ),
    };
    let site = addr("dthd.restream.meter_rate");
    RESUME.store(addr("dthd.restream.meter_resume"), Ordering::Relaxed);
    REFUSE.store(addr("dthd.restream.meter_refuse"), Ordering::Relaxed);
    // jmp qword ptr [rip+0]; .qword stub
    let mut jump = vec![0xFF, 0x25, 0, 0, 0, 0];
    jump.extend_from_slice(
        &(dshim_dthd_meter_rate_stub as *const () as usize as u64).to_le_bytes(),
    );
    if !write_code(site, &jump) {
        fail("dthd.restream.meter_rate write failed", log);
    }
    lok!(
        log,
        "dthd",
        "rate: loudness meter resampler, jump @ {site:#x}"
    );
}
