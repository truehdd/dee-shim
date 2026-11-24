//! WAV inputs that hold only their layout's channels: the DTHD_WAV_SLOTS and DDP_WAV_SLOTS
//! levers.

use dshim_levers::{wav_format, WavSlots};
use dshim_profiles::dthd_layouts::{self, Tier};

/// One input of the tool: what the messages call it, its files, how many input slots it has
/// and which of them its layout names.
struct Input {
    name: String,
    files: Vec<String>,
    list: bool,
    total: usize,
    named: Vec<usize>,
}

/// The inputs given with only their layout's channels (--unpadded-wav). Every other input must
/// fit its slots, since DEE's own count check is then open.
fn unpadded(inputs: &[Input]) -> Result<Vec<WavSlots>, String> {
    let mut found: Vec<WavSlots> = Vec::new();
    let mut other = None;
    for input in inputs {
        let count = if input.list {
            input.files.len()
        } else {
            match input.files.first().and_then(|path| wav_format(path)) {
                Some((channels, _)) => channels as usize,
                None => continue,
            }
        };
        if count == input.total {
            continue;
        }
        if !input.list && count == input.named.len() {
            let same = |w: &&WavSlots| w.slots.len() == count && w.total != input.total;
            if let Some(w) = found.iter().find(same) {
                return Err(format!(
                    "{} and {} both have {count} channels for different presentations; pad one \
                     of them to its presentation's channels",
                    w.path, input.files[0]
                ));
            }
            found.push(WavSlots {
                total: input.total,
                slots: input.named.clone(),
                path: input.files[0].clone(),
            });
        } else {
            other.get_or_insert((input, count));
        }
    }
    match other {
        Some((input, count)) => Err(format!(
            "{} has {count} channels; it takes {} (one per input slot){}",
            input.name,
            input.total,
            if input.list || input.named.len() == input.total {
                String::new()
            } else {
                format!(" or {} (the layout's channels in order)", input.named.len())
            }
        )),
        None => Ok(found),
    }
}

fn option<'a>(argument: &'a str, next: Option<&'a String>, names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|name| {
        if argument == *name {
            next.map(String::as_str)
        } else if name.starts_with("--") {
            argument.strip_prefix(name)?.strip_prefix('=')
        } else {
            None
        }
    })
}

/// dee_dthd_encoder: each `--presentation` with its format and input, and the layouts given.
pub fn dthd(native: &[String], layouts: &str) -> Result<Vec<WavSlots>, String> {
    let layouts = dthd_layouts::parse(layouts).unwrap_or_default();
    let mut found: Vec<(Option<Tier>, Option<&str>, Option<&str>)> = Vec::new();
    let mut atmos = false;
    for (index, argument) in native.iter().enumerate() {
        let next = native.get(index + 1);
        if let Some(value) = option(argument, next, &["--presentation"]) {
            let kind = value.split(':').next().unwrap_or(value);
            atmos |= kind == "atmos";
            found.push((Tier::parse(kind), None, None));
        } else if let Some(last) = found.last_mut() {
            if let Some(value) = option(argument, next, &["-f", "--input-format"]) {
                last.1 = Some(value);
            } else if let Some(value) = option(argument, next, &["-i", "--input"]) {
                last.2 = Some(value);
            }
        }
    }
    let top = found.iter().filter_map(|p| p.0).max_by_key(|t| t.slots());
    let mut inputs = Vec::new();
    for (tier, format, input) in found {
        let (Some(tier), Some(format), Some(input)) = (tier, format, input) else {
            continue;
        };
        if format != "wav" && format != "wav_list" {
            continue;
        }
        let layout = layouts
            .iter()
            .find(|(t, _)| *t == Some(tier) || (t.is_none() && !atmos && top == Some(tier)));
        let total = tier.slots();
        let named = match layout {
            Some((_, spec)) => dthd_layouts::slots(spec, total)?
                .iter()
                .enumerate()
                .filter_map(|(slot, label)| label.map(|_| slot))
                .collect(),
            None => (0..total).collect(),
        };
        let list = format == "wav_list";
        inputs.push(Input {
            name: format!("the {} input", tier.name()),
            files: if list {
                crate::paths::split_list(input)
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            } else {
                vec![input.to_owned()]
            },
            list,
            total,
            named,
        });
    }
    unpadded(&inputs)
}

/// dee_ddp_encoder: the one WAV input, with the layout's input slots (`L R C - S -`).
pub fn ddp(native: &[String], slots: &str) -> Result<Vec<WavSlots>, String> {
    let last = |names: &[&str]| {
        (0..native.len())
            .rev()
            .find_map(|index| option(&native[index], native.get(index + 1), names))
    };
    let (Some("wav"), Some(input)) = (last(&["-f", "--input-format"]), last(&["-i", "--input"]))
    else {
        return Ok(Vec::new());
    };
    let labels: Vec<&str> = slots.split_whitespace().collect();
    unpadded(&[Input {
        name: "the input".to_owned(),
        files: vec![input.to_owned()],
        list: false,
        total: labels.len(),
        named: (0..labels.len()).filter(|i| labels[*i] != "-").collect(),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(dir: &std::path::Path, name: &str, channels: u16) -> String {
        let mut bytes = b"RIFF\x24\0\0\0WAVEfmt \x10\0\0\0\x01\0".to_vec();
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&48000u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(b"data\0\0\0\0");
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_wav_with_the_layouts_channels_is_spread_over_its_slots() {
        let dir = tempfile::tempdir().unwrap();
        let (four, six) = (
            wav(dir.path(), "four.wav", 4),
            wav(dir.path(), "six.wav", 6),
        );
        let found = ddp(&args(&["--input-format=wav", "-i", &four]), "L R C - S -").unwrap();
        assert_eq!(found[0].slots, [0, 1, 2, 4]);
        assert_eq!((found[0].total, found[0].path.as_str()), (6, four.as_str()));
        // A padded WAV is read as before; any other count is refused.
        assert!(ddp(&args(&["-f", "wav", "-i", &six]), "L R C - S -")
            .unwrap()
            .is_empty());
        assert!(ddp(&args(&["-f", "wav", "-i", &four]), "L R C LFE Ls Rs").is_err());

        let run = |layout: &str, rest: &[&str]| {
            let mut native = args(&["--presentation", "8ch", "-f", "wav", "-i", &four]);
            native.extend(args(rest));
            dthd(&native, layout)
        };
        assert_eq!(run("4.0", &[]).unwrap()[0].slots, [0, 1, 2, 4]);
        // A 6ch layout does not explain the 8ch input.
        assert!(run("6ch=4.0-quad", &[]).is_err());
        // An input that fits neither way is refused here.
        let wrong = ["--presentation", "6ch", "-f", "wav", "-i", &four];
        assert!(run("4.0", &wrong)
            .unwrap_err()
            .contains("the 6ch input has 4 channels"));
        let fits = ["--presentation", "6ch", "-f", "wav", "-i", &six];
        assert_eq!(run("4.0", &fits).unwrap().len(), 1);
    }
}
