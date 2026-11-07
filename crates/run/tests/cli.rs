use std::process::{Command, Output};

fn run(arguments: &[&str]) -> Output {
    command(arguments).output().unwrap()
}

fn command(arguments: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dee_shim"));
    command
        .args(arguments)
        .env_remove("DSHIM_LOG")
        .env_remove("DSHIM_OUT")
        .env_remove("DSHIM_LOG_LEVEL")
        .env_remove("DSHIM_LOG_FORMAT")
        .env_remove("RUST_LOG")
        .env("NO_COLOR", "1");
    command
}

fn usage_error(arguments: &[&str], message: &str) {
    let result = run(arguments);
    let stderr = String::from_utf8(result.stderr).unwrap();
    assert_eq!(result.status.code(), Some(2), "{stderr}");
    assert!(result.stdout.is_empty());
    assert!(stderr.starts_with("error:"), "{stderr}");
    assert!(stderr.contains(message), "{stderr}");
    assert!(
        stderr
            .replace("dee_shim.exe", "dee_shim")
            .contains("Usage: dee_shim ddp_encoder"),
        "{stderr}"
    );
    assert!(!stderr.contains('\u{1b}'));
}

#[test]
fn bare_subcommands_show_help_without_starting_dee() {
    for tool in ["ddp_encoder", "ddpjoc_encoder", "dthd_encoder"] {
        let result = run(&[tool]);
        let stderr = String::from_utf8(result.stderr).unwrap();
        assert_eq!(result.status.code(), Some(2), "{tool}: {stderr}");
        assert!(result.stdout.is_empty());
        assert!(stderr
            .replace("dee_shim.exe", "dee_shim")
            .contains(&format!("Usage: dee_shim {tool}")));
        assert!(stderr.contains("--help"));
        assert!(!stderr.contains("ERROR"));
    }
}

#[test]
fn argument_checks_use_clap_even_when_logging_is_disabled() {
    let binary = env!("CARGO_BIN_EXE_dee_shim");
    usage_error(
        &[
            "--log-level",
            "off",
            "ddp_encoder",
            "--binary",
            binary,
            "--layout",
            "5.1",
            "--bitrate",
            "641",
        ],
        "--bitrate 641 is not accepted",
    );
    usage_error(
        &[
            "ddp_encoder",
            "--binary",
            binary,
            "--layout",
            "5.1",
            "--core-type",
            "standard",
        ],
        "--core-type requires --layout 7.0 or 7.1",
    );
    usage_error(
        &[
            "ddp_encoder",
            "--binary",
            binary,
            "--layout",
            "5.1",
            "--bitrate",
            "512:512",
        ],
        "needs bluray mode or an extended /eac3 layout",
    );
}

#[test]
fn malformed_settings_are_not_silently_ignored() {
    for setting in ["broken", "=value"] {
        let result = run(&["ddp_encoder", "--set", setting]);
        let stderr = String::from_utf8(result.stderr).unwrap();
        assert_eq!(result.status.code(), Some(2));
        assert!(stderr.starts_with("error:"));
        assert!(stderr.contains("expected KEY=VAL"), "{stderr}");
    }
}

#[test]
fn native_arguments_require_the_separator() {
    let binary = env!("CARGO_BIN_EXE_dee_shim");
    for arguments in [
        vec!["ddp_encoder", "--layuot", "5.1"],
        vec!["ddp_encoder", "--binary", binary, "-i", "input.wav"],
    ] {
        let result = run(&arguments);
        let stderr = String::from_utf8(result.stderr).unwrap();
        assert_eq!(result.status.code(), Some(2));
        assert!(stderr.starts_with("error: unexpected argument"), "{stderr}");
        assert!(stderr.contains("Usage:"), "{stderr}");
    }
    let result = run(&["ddp_encoder", "--layuot", "5.1"]);
    assert!(String::from_utf8(result.stderr)
        .unwrap()
        .contains("--layout"));
}

#[test]
fn arguments_after_the_separator_belong_to_dee() {
    let binary = env!("CARGO_BIN_EXE_dee_shim");
    let result = run(&[
        "ddp_encoder",
        "--binary",
        binary,
        "--dry-run",
        "--",
        "--layout",
        "native-value",
        "--unknown-native-option",
        "--",
    ]);
    assert!(result.status.success(), "{:?}", result.stderr);
    let stdout = String::from_utf8(result.stdout).unwrap();
    assert!(stdout.contains("--layout"), "{stdout}");
    assert!(stdout.contains("native-value"), "{stdout}");
    assert!(stdout.contains("--unknown-native-option"), "{stdout}");
}

#[test]
fn settings_keep_embedded_equals_signs() {
    let binary = env!("CARGO_BIN_EXE_dee_shim");
    let result = run(&[
        "ddp_encoder",
        "--binary",
        binary,
        "--set",
        "DDP_SET_PARAMS=5=1",
        "--dry-run",
    ]);
    assert!(result.status.success(), "{:?}", result.stderr);
    assert!(String::from_utf8(result.stdout)
        .unwrap()
        .contains("DDP_SET_PARAMS=5=1"));
}

#[test]
fn compose_templates_are_removed_after_dry_runs_and_launch_failures() {
    let root = tempfile::tempdir().unwrap();
    let temp = root.path().join("temp");
    std::fs::create_dir(&temp).unwrap();
    let spec = root.path().join("job.json");
    let body = r#"{"presentations":[{"type":"two-channel","channels":[]}]}"#;
    std::fs::write(&spec, body).unwrap();
    let missing = root.path().join("absent-encoder");
    let binary = env!("CARGO_BIN_EXE_dee_shim");
    for dry_run in [true, false] {
        let encoder = if dry_run {
            binary
        } else {
            missing.to_str().unwrap()
        };
        let mut command = command(&[
            "dthd_encoder",
            "--binary",
            encoder,
            "--compose",
            spec.to_str().unwrap(),
        ]);
        command
            .env("TMPDIR", &temp)
            .env("TMP", &temp)
            .env("TEMP", &temp);
        if dry_run {
            command.arg("--dry-run");
        }
        let result = command.output().unwrap();
        assert_eq!(result.status.success(), dry_run, "{:?}", result.stderr);
        assert_eq!(std::fs::read_dir(&temp).unwrap().count(), 0);
        assert_eq!(std::fs::read_to_string(&spec).unwrap(), body);
    }
}

#[test]
fn layout_list_does_not_require_a_binary() {
    for tool in ["ddp_encoder", "ddpjoc_encoder"] {
        let result = run(&[tool, "--list"]);
        assert!(result.status.success());
        assert!(result.stderr.is_empty());
        assert!(String::from_utf8(result.stdout)
            .unwrap()
            .contains("layouts for"));
    }
}

#[test]
#[cfg(any(target_os = "macos", windows))]
fn missing_binary_is_reported() {
    let result = run(&["ddp_encoder", "--layout", "5.1"]);
    let stderr = String::from_utf8(result.stderr).unwrap();
    assert_ne!(result.status.code(), Some(0), "{stderr}");
    assert!(stderr.contains("--binary"), "{stderr}");
    assert!(stderr.contains("none beside dee_shim"), "{stderr}");
}

// The in-process dry run prints the resolved tool, so this needs an x86_64 build.
#[test]
#[cfg(target_arch = "x86_64")]
fn the_tool_beside_dee_shim_is_found_without_moving_the_caller() {
    let suffix = std::env::consts::EXE_SUFFIX;
    let install = tempfile::tempdir().unwrap();
    let shim = install.path().join(format!("dee_shim{suffix}"));
    std::fs::copy(env!("CARGO_BIN_EXE_dee_shim"), &shim).unwrap();
    std::fs::write(install.path().join(format!("dee_ddp_encoder{suffix}")), b"").unwrap();
    let caller = tempfile::tempdir().unwrap();
    let result = Command::new(&shim)
        .current_dir(caller.path())
        .args(["ddp_encoder", "--dry-run", "--", "-i", "in.wav"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8(result.stdout).unwrap();
    assert_eq!(result.status.code(), Some(0), "{stdout}");
    let name = |dir: &tempfile::TempDir| {
        dir.path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    };
    let tool = stdout.lines().next().unwrap_or_default();
    assert!(tool.contains(&name(&install)), "{stdout}");
    assert!(
        tool.contains(&format!("dee_ddp_encoder{suffix}")),
        "{stdout}"
    );
    let native = stdout
        .lines()
        .find(|l| l.starts_with("native arguments:"))
        .unwrap();
    assert!(
        native.contains(&name(&caller)) && native.contains("in.wav"),
        "{stdout}"
    );
}
