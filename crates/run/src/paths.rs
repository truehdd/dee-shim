//! Native file arguments are relative to the caller, not the executable or scratch directory.

use std::path::Path;

/// A URI scheme such as DEE's `named_pipe`; a single letter is a drive.
fn scheme(name: &str) -> bool {
    name.len() > 1
        && name.as_bytes()[0].is_ascii_alphabetic()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'.' | b'-'))
}

/// A DEE URI such as `named_pipe://...`, which is not a path.
fn uri(value: &str) -> bool {
    value
        .split_once("://")
        .is_some_and(|(name, _)| scheme(name))
}

pub fn absolute(path: &str, cwd: &Path) -> String {
    if path == "-" || path.is_empty() || uri(path) || Path::new(path).is_absolute() {
        path.to_owned()
    } else {
        cwd.join(path).to_string_lossy().into_owned()
    }
}

/// The entries of a wav_list value; the colon of a drive or a URI scheme does not split.
fn split_list(value: &str) -> Vec<&str> {
    let mut paths = Vec::new();
    let mut start = 0;
    for (index, byte) in value.bytes().enumerate() {
        if byte != b':' {
            continue;
        }
        let drive = index == start + 1
            && value.as_bytes()[start].is_ascii_alphabetic()
            && matches!(value.as_bytes().get(index + 1), Some(b'/' | b'\\'));
        let uri = value[index..].starts_with("://") && scheme(&value[start..index]);
        if !drive && !uri {
            paths.push(&value[start..index]);
            start = index + 1;
        }
    }
    paths.push(&value[start..]);
    paths
}

fn wav_list(value: &str, cwd: &Path) -> String {
    let paths: Vec<String> = split_list(value)
        .into_iter()
        .map(|p| absolute(p, cwd))
        .collect();
    paths.join(":")
}

/// The last value of a native option, given as `-x v`, `--long v` or `--long=v`.
fn value<'a>(arguments: &'a [String], short: Option<&str>, long: &str) -> Option<&'a str> {
    let prefix = format!("{long}=");
    arguments
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, argument)| {
            if argument == long || Some(argument.as_str()) == short {
                arguments.get(index + 1).map(String::as_str)
            } else {
                argument.strip_prefix(&prefix)
            }
        })
}

fn input_format(arguments: &[String]) -> Option<&str> {
    value(arguments, Some("-f"), "--input-format")
}

/// The sample rate in a WAV (RIFF, RIFX not handled, or RF64) header's fmt chunk.
fn wav_rate(path: &str) -> Option<u32> {
    use std::io::Read;
    let mut head = vec![0u8; 1 << 16];
    let n = std::fs::File::open(path).ok()?.read(&mut head).ok()?;
    let head = &head[..n];
    if !matches!(head.get(..4)?, b"RIFF" | b"RF64") || head.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut at = 12;
    while at + 8 <= head.len() {
        let size = u32::from_le_bytes(head[at + 4..at + 8].try_into().ok()?) as usize;
        if &head[at..at + 4] == b"fmt " {
            return Some(u32::from_le_bytes(
                head.get(at + 12..at + 16)?.try_into().ok()?,
            ));
        }
        at += 8 + size + (size & 1);
    }
    None
}

/// DEE measures loudness at 48 kHz, and its resampler has no 176.4 kHz to 48 kHz pair, so a
/// 176.4 kHz WAV feed fails late with a meter error unless loudness measurement is skipped.
pub fn meter_rate_check(arguments: &[String]) -> Result<(), String> {
    let list = match input_format(arguments) {
        Some("wav") => false,
        Some("wav_list") => true,
        _ => return Ok(()),
    };
    let Some(input) = value(arguments, Some("-i"), "--input") else {
        return Ok(());
    };
    let first = if list {
        split_list(input).into_iter().find(|p| *p != "-")
    } else {
        Some(input)
    };
    let skip = value(arguments, None, "--loudness-management")
        .is_some_and(|m| m.split(':').next() == Some("skip"));
    if !skip && first.and_then(wav_rate) == Some(176_400) {
        return Err(
            "176.4 kHz input needs --loudness-management skip:dialnorm=N: DEE measures \
                    loudness at 48 kHz and cannot resample 176.4 kHz to it"
                .to_string(),
        );
    }
    Ok(())
}

pub fn native(arguments: &mut [String], cwd: &Path) {
    let list = input_format(arguments) == Some("wav_list");
    let mut next = None;
    for argument in arguments {
        if let Some(input) = next.take() {
            *argument = if input && list {
                wav_list(argument, cwd)
            } else {
                absolute(argument, cwd)
            };
            continue;
        }
        let (flag, value) = argument.split_once('=').unwrap_or((argument, ""));
        let input = matches!(flag, "-i" | "--input");
        if input
            || matches!(
                flag,
                "-o" | "--output" | "-l" | "--license" | "--log-file" | "--temp-dir"
            )
        {
            if argument.contains('=') {
                let path = if input && list {
                    wav_list(value, cwd)
                } else {
                    absolute(value, cwd)
                };
                *argument = format!("{flag}={path}");
            } else {
                next = Some(input);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caller_paths_cover_missing_outputs_and_equals_syntax() {
        let cwd = std::env::current_dir().unwrap().join("caller");
        let mut arguments = vec![
            "-i",
            "in file.wav",
            "--output=out.ec3",
            "--license",
            "license.lic",
            "--cc",
            "0",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        native(&mut arguments, &cwd);
        assert_eq!(arguments[1], cwd.join("in file.wav").to_string_lossy());
        assert_eq!(
            arguments[2],
            format!("--output={}", cwd.join("out.ec3").display())
        );
        assert_eq!(arguments[4], cwd.join("license.lic").to_string_lossy());
        assert_eq!(arguments[6], "0");
    }

    #[test]
    fn wav_list_keeps_silent_slots_and_absolute_paths() {
        let cwd = std::env::current_dir().unwrap().join("caller");
        let absolute = cwd.join("centre.wav").to_string_lossy().into_owned();
        let mut arguments = vec![
            "--input-format=wav_list".into(),
            "--input".into(),
            format!("left.wav:-:{absolute}:right.wav"),
        ];
        native(&mut arguments, &cwd);
        assert_eq!(
            arguments[2],
            format!(
                "{}:-:{absolute}:{}",
                cwd.join("left.wav").display(),
                cwd.join("right.wav").display()
            )
        );
        assert_eq!(super::absolute("-", &cwd), "-");
    }

    #[test]
    fn dee_uris_are_not_paths() {
        let cwd = std::env::current_dir().unwrap().join("caller");
        let pipe = r"named_pipe://\\.\pipe\feed";
        let mut arguments = vec![
            "-f".into(),
            "wav_list".into(),
            "-i".into(),
            format!("{pipe}:-:left.wav"),
            "--output=named_pipe:///tmp/out".into(),
        ];
        native(&mut arguments, &cwd);
        assert_eq!(
            arguments[3],
            format!("{pipe}:-:{}", cwd.join("left.wav").display())
        );
        assert_eq!(arguments[4], "--output=named_pipe:///tmp/out");
        assert!(!uri("C://x") && !uri("dir/a://b"));
    }

    #[test]
    fn meter_rate_check_reads_the_feed_header() {
        let dir = tempfile::tempdir().unwrap();
        let wav = |rate: u32| {
            let path = dir.path().join(format!("{rate}.wav"));
            let mut b = b"RIFF\0\0\0\0WAVEJUNK\x02\0\0\0\0\0fmt \x10\0\0\0\x01\0\x02\0".to_vec();
            b.extend_from_slice(&rate.to_le_bytes());
            b.extend_from_slice(&[0; 8]);
            std::fs::write(&path, b).unwrap();
            path.to_string_lossy().into_owned()
        };
        let (high, low) = (wav(176_400), wav(48_000));
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(wav_rate(&high), Some(176_400));
        assert!(meter_rate_check(&args(&["-f", "wav", "-i", &high])).is_err());
        assert!(meter_rate_check(&args(&["--input-format=wav", "--input", &low])).is_ok());
        let list = format!("-:{high}");
        assert!(meter_rate_check(&args(&["-f", "wav_list", "-i", &list])).is_err());
        let skip = args(&[
            "-f",
            "wav",
            "-i",
            &high,
            "--loudness-management",
            "skip:dialnorm=-24",
        ]);
        assert!(meter_rate_check(&skip).is_ok());
        assert!(meter_rate_check(&args(&["-f", "atmos_mezz", "-i", &high])).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn windows_drive_colons_are_not_list_separators() {
        let cwd = Path::new(r"C:\caller");
        assert_eq!(
            wav_list(r"D:\audio\left.wav:-:right.wav", cwd),
            r"D:\audio\left.wav:-:C:\caller\right.wav"
        );
    }
}
