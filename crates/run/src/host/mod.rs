//! In-process host: run the DEE tool inside a `dee_shim` process.
//!
//! Two processes, both this executable. The parent keeps the CLI, temporary jobs, cleanup and
//! the exit code; it starts itself again with `DEE_SHIM_HOST` naming the tool and, when any lever
//! is set, `DEE_SHIM_LEVERS` holding the checked levers. That child removes both from its
//! environment, maps the untouched tool into its own address space, links it, calls
//! `dshim_engine::init_hosted` with the levers so the engine patches the mapped image, and enters
//! the tool. The tool ends with `exit()`, which is why it cannot share the parent's process.
//!
//! The per-OS loaders live in `macos.rs`, `linux` and `windows`, each exposing
//! `run(binary, args, levers) -> i32`. `args` excludes argv[0]; the loader presents `binary` as
//! the tool's own path and argv[0].

use dshim_levers::Levers;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod linux;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use linux as os;

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
mod macos;
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
use macos as os;

#[cfg(all(windows, target_arch = "x86_64"))]
mod windows;
#[cfg(all(windows, target_arch = "x86_64"))]
use windows as os;

// Choosing how the parent starts the host child is per OS too, and runs in the parent, which on
// macOS may be the arm64 slice of a universal dee_shim while the tools are x86_64.
#[cfg(target_os = "macos")]
mod macos_launch;

/// The command that starts this executable as a host child.
fn host_command(me: &Path) -> Result<Command, String> {
    #[cfg(target_os = "macos")]
    {
        macos_launch::command(me)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(Command::new(me))
    }
}

/// Names the tool binary in the host child's environment. Its presence makes `dee_shim` a host.
const HOST_ENV: &str = "DEE_SHIM_HOST";

/// The encoded levers (`Levers::encode`) in the host child's environment; absent when none is set.
const LEVERS_ENV: &str = "DEE_SHIM_LEVERS";

/// Exit status when the host child cannot load or start the tool.
pub const LOAD_FAILED: i32 = 70;

/// Whether this build can run a host child: any x86_64 build, and on macOS a universal build
/// whose x86_64 slice the parent can start.
pub fn available() -> bool {
    if cfg!(target_arch = "x86_64") {
        return true;
    }
    #[cfg(target_os = "macos")]
    if let Ok(me) = std::env::current_exe() {
        return macos_launch::command(&me).is_ok();
    }
    false
}

/// Called first in `main`. In a host child, run the tool and return its exit status; in the
/// ordinary CLI, return None.
pub fn child() -> Option<i32> {
    let binary = std::env::var_os(HOST_ENV)?;
    std::env::remove_var(HOST_ENV);
    let levers = std::env::var(LEVERS_ENV).unwrap_or_default();
    std::env::remove_var(LEVERS_ENV);
    let levers = match Levers::decode(&levers) {
        Ok(levers) => levers,
        Err(error) => {
            eprintln!("dee_shim: in-process host: levers: {error}");
            return Some(LOAD_FAILED);
        }
    };
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    Some(run_tool(Path::new(&binary), &args, levers))
}

/// Set the levers on a command that starts a host child (the parent's spawn, and the Linux
/// loader's restart).
fn pass_levers(command: &mut Command, levers: &Levers) {
    if !levers.is_empty() {
        command.env(LEVERS_ENV, levers.encode());
    }
}

#[cfg(target_arch = "x86_64")]
fn run_tool(binary: &Path, args: &[OsString], levers: Levers) -> i32 {
    os::run(binary, args, levers)
}

#[cfg(not(target_arch = "x86_64"))]
fn run_tool(_binary: &Path, _args: &[OsString], _levers: Levers) -> i32 {
    eprintln!("dee_shim: the in-process host needs the x86_64 build of dee_shim");
    LOAD_FAILED
}

/// Parent side: run `binary` with `native` arguments in a host child and return its status.
/// `temp` redirects the child's temporary directory.
pub fn spawn(
    binary: &Path,
    native: &[String],
    levers: &Levers,
    temp: Option<&Path>,
) -> Result<i32, String> {
    let binary = binary
        .canonicalize()
        .map_err(|e| format!("{}: {e}", binary.display()))?;
    let me = std::env::current_exe().map_err(|e| format!("locate dee_shim: {e}"))?;
    let mut command = host_command(&me)?;
    command.args(native).env(HOST_ENV, &binary);
    pass_levers(&mut command, levers);
    if let Some(directory) = temp {
        for key in ["TMPDIR", "TMP", "TEMP"] {
            command.env(key, directory);
        }
    }
    let status = command
        .status()
        .map_err(|e| format!("start the in-process host: {e}"))?;
    Ok(status.code().unwrap_or(1))
}
