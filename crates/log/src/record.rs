use serde::Serialize;
use std::io;
use time::{macros::format_description, OffsetDateTime};

#[derive(Clone, Copy)]
pub enum Format {
    Text,
    Json,
}

impl Format {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            _ => Err(format!(
                "unknown shim log format {value:?}; expected text or json"
            )),
        }
    }
}

#[derive(Serialize)]
struct Record<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    timestamp: String,
    pid: u32,
    level: &'static str,
    target: &'a str,
    message: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    truncated: bool,
}

pub fn encode(
    format: Format,
    record: &log::Record<'_>,
    limit: Option<usize>,
) -> io::Result<Vec<u8>> {
    let mut event = Record {
        kind: "log",
        timestamp: if matches!(format, Format::Json) {
            OffsetDateTime::now_utc()
                .format(format_description!(
                    "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
                ))
                .map_err(io::Error::other)?
        } else {
            String::new()
        },
        pid: std::process::id(),
        level: match record.level() {
            log::Level::Error => "error",
            log::Level::Warn => "warn",
            log::Level::Info => "info",
            log::Level::Debug => "debug",
            log::Level::Trace => "trace",
        },
        target: record.target(),
        message: record.args().to_string(),
        truncated: false,
    };
    loop {
        let mut line = match format {
            Format::Json => serde_json::to_vec(&event).map_err(io::Error::other)?,
            Format::Text => format!(
                "{} {}: {}{}",
                event.target,
                record.level(),
                event.message,
                if event.truncated { " [truncated]" } else { "" }
            )
            .into_bytes(),
        };
        line.push(b'\n');
        let Some(limit) = limit.filter(|limit| line.len() > *limit) else {
            return Ok(line);
        };
        if event.message.is_empty() {
            return Err(io::Error::other("log metadata exceeds pipe record limit"));
        }
        let mut end = event.message.len().min(limit / 2);
        if end == event.message.len() {
            end /= 2;
        }
        while !event.message.is_char_boundary(end) {
            end -= 1;
        }
        event.message.truncate(end);
        event.truncated = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_one_line_with_escaped_messages_and_stable_fields() {
        let record = log::Record::builder()
            .target("dshim::test")
            .level(log::Level::Warn)
            .args(format_args!("quotes \" and newline\n日本語"))
            .build();
        let line = encode(Format::Json, &record, None).unwrap();
        assert_eq!(line.iter().filter(|byte| **byte == b'\n').count(), 1);
        let event: serde_json::Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(event["type"], "log");
        assert_eq!(event["level"], "warn");
        assert_eq!(event["target"], "dshim::test");
        assert_eq!(event["message"], "quotes \" and newline\n日本語");
        assert_eq!(event["pid"], std::process::id());
        assert!(event["timestamp"].as_str().unwrap().ends_with('Z'));
        assert!(event.get("truncated").is_none());
    }

    #[test]
    fn bounded_json_keeps_utf8_and_complete_records() {
        let message = "\"日本語\n".repeat(4096);
        let check = |record: &log::Record| {
            for format in [Format::Text, Format::Json] {
                let line = encode(format, record, Some(512)).unwrap();
                assert!(line.len() <= 512);
                assert!(std::str::from_utf8(&line).is_ok());
                assert_eq!(line.last(), Some(&b'\n'));
            }
            let line = encode(Format::Json, record, Some(512)).unwrap();
            let event: serde_json::Value = serde_json::from_slice(&line).unwrap();
            assert_eq!(event["truncated"], true);
            assert!(!event["message"].as_str().unwrap().is_empty());
            assert!(message.starts_with(event["message"].as_str().unwrap()));
            assert!(encode(Format::Json, record, None).unwrap().len() > 512);
        };
        // One statement, so the format_args! temporary outlives the record (Rust 1.88).
        check(
            &log::Record::builder()
                .target("dshim::test")
                .args(format_args!("{message}"))
                .build(),
        );
    }
}
