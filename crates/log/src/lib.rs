//! Logging shared by the launcher and the engine.

use env_logger::{Builder, Target};
use std::io::Write;
use std::path::Path;

mod pipe;
mod record;

pub fn output() -> String {
    std::env::var("DSHIM_LOG")
        .or_else(|_| std::env::var("DSHIM_OUT"))
        .unwrap_or_else(|_| "stderr".into())
}

pub fn filter() -> String {
    std::env::var("DSHIM_LOG_LEVEL")
        .or_else(|_| std::env::var("RUST_LOG"))
        .unwrap_or_else(|_| "warn".into())
}

pub fn format() -> String {
    std::env::var("DSHIM_LOG_FORMAT").unwrap_or_else(|_| "text".into())
}

pub fn is_pipe(output: &str) -> bool {
    ["pipe:", "fd:", "handle:"]
        .iter()
        .any(|prefix| output.starts_with(prefix))
}

pub fn resolve(output: &str, cwd: &Path) -> String {
    if let Some(path) = output.strip_prefix("pipe:") {
        return format!("pipe:{}", cwd.join(path).display());
    }
    if matches!(output, "stderr" | "stdout") || is_pipe(output) {
        output.to_owned()
    } else {
        cwd.join(output).to_string_lossy().into_owned()
    }
}

pub fn init(filter: &str, output: &str, format: &str) -> Result<String, String> {
    let format = record::Format::parse(format)?;
    let mut destination = output.to_owned();
    let mut limit = None;
    let target = match output {
        "stderr" => Target::Stderr,
        "stdout" => Target::Stdout,
        pipe if is_pipe(pipe) => {
            let (pipe, inherited) = pipe::Pipe::open(pipe)
                .map_err(|error| format!("open shim log pipe {output:?}: {error}"))?;
            limit = Some(pipe.limit());
            destination = inherited;
            Target::Pipe(Box::new(pipe))
        }
        path => {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|error| format!("open shim log {path:?}: {error}"))?;
            Target::Pipe(Box::new(file))
        }
    };
    let mut builder = Builder::new();
    builder
        .parse_filters(filter)
        .target(target)
        .format(move |buffer, event| buffer.write_all(&record::encode(format, event, limit)?));
    builder.try_init().map_err(|error| error.to_string())?;
    Ok(destination)
}

pub fn flush() {
    log::logger().flush();
}
