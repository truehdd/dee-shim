use super::Pipe;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::FileTypeExt;

pub(super) fn open(output: &str) -> io::Result<(Pipe, String)> {
    let file = if let Some(value) = output.strip_prefix("fd:") {
        let fd: i32 = value
            .parse()
            .map_err(|_| io::Error::other("fd:N requires a descriptor number"))?;
        if fd < 3 {
            return Err(io::Error::other(
                "use a dedicated pipe descriptor above stderr",
            ));
        }
        // SAFETY: fcntl validates fd and allocates an owned descriptor outside stdio.
        let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD, 3) };
        if duplicate < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: this descriptor was just allocated by fcntl and is owned by this logger.
        unsafe { File::from_raw_fd(duplicate) }
    } else if let Some(path) = output.strip_prefix("pipe:") {
        OpenOptions::new().write(true).open(path)?
    } else {
        return Err(io::Error::other("handle:N requires Windows"));
    };
    let file = if file.as_raw_fd() < 3 {
        // SAFETY: move an opened FIFO away from any closed standard descriptor slots.
        let duplicate = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD, 3) };
        if duplicate < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fcntl returned this newly owned descriptor.
        unsafe { File::from_raw_fd(duplicate) }
    } else {
        file
    };
    let fd = file.as_raw_fd();
    // SAFETY: file owns a valid descriptor for the following descriptor queries.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if !file.metadata()?.file_type().is_fifo() || flags & libc::O_ACCMODE == libc::O_RDONLY {
        return Err(io::Error::other("log descriptor must be a writable pipe"));
    }
    if flags & libc::O_NONBLOCK != 0 {
        return Err(io::Error::other("log pipe must use blocking writes"));
    }
    // SAFETY: keep this owned pipe available through the encoder's exec.
    let descriptor_flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if descriptor_flags < 0
        || unsafe { libc::fcntl(fd, libc::F_SETFD, descriptor_flags & !libc::FD_CLOEXEC) } < 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fpathconf queries the atomic write bound for this valid pipe.
    let limit = unsafe { libc::fpathconf(fd, libc::_PC_PIPE_BUF) };
    let limit = usize::try_from(limit)
        .ok()
        .filter(|limit| *limit >= 512)
        .unwrap_or(512);
    Ok((Pipe { file, limit }, format!("fd:{fd}")))
}

pub(super) fn write(file: &File, bytes: &[u8]) -> io::Result<usize> {
    // SAFETY: all signal sets are initialized before use. Only the current thread's mask changes.
    unsafe {
        let mut set = std::mem::zeroed();
        let mut previous = std::mem::zeroed();
        let mut pending = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGPIPE);
        let error = libc::pthread_sigmask(libc::SIG_BLOCK, &set, &mut previous);
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error));
        }
        libc::sigpending(&mut pending);
        let already_pending = libc::sigismember(&pending, libc::SIGPIPE) == 1;
        let written = libc::write(file.as_raw_fd(), bytes.as_ptr().cast(), bytes.len());
        let result = if written < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(written as usize)
        };
        if result
            .as_ref()
            .is_err_and(|error| error.raw_os_error() == Some(libc::EPIPE))
            && !already_pending
        {
            libc::sigpending(&mut pending);
            if libc::sigismember(&pending, libc::SIGPIPE) == 1 {
                let mut signal = 0;
                libc::sigwait(&set, &mut signal);
            }
        }
        libc::pthread_sigmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut());
        result
    }
}
