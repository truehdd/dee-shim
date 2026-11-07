//! Inheritable log pipes shared by the launcher and encoder.

use std::fs::File;
use std::io::{self, Write};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

pub struct Pipe {
    file: File,
    limit: usize,
}

impl Pipe {
    pub fn open(output: &str) -> io::Result<(Self, String)> {
        #[cfg(unix)]
        let result = unix::open(output);
        #[cfg(windows)]
        let result = windows::open(output);
        result
    }

    pub fn limit(&self) -> usize {
        self.limit
    }
}

impl Write for Pipe {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "log record exceeds pipe limit",
            ));
        }
        #[cfg(unix)]
        return unix::write(&self.file, bytes);
        #[cfg(windows)]
        self.file.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
