use super::Pipe;
use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle};

const FILE_TYPE_PIPE: u32 = 3;
const HANDLE_FLAG_INHERIT: u32 = 1;
const DUPLICATE_SAME_ACCESS: u32 = 2;
const FILE_MODE_INFORMATION: u32 = 16;
const FILE_SYNCHRONOUS_IO: u32 = 0x10 | 0x20;

#[repr(C)]
#[derive(Default)]
struct IoStatus {
    status: usize,
    information: usize,
}

#[link(name = "ntdll")]
extern "system" {
    fn NtQueryInformationFile(
        handle: *mut c_void,
        status: *mut IoStatus,
        information: *mut u32,
        length: u32,
        class: u32,
    ) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> *mut c_void;
    fn DuplicateHandle(
        source: *mut c_void,
        handle: *mut c_void,
        target: *mut c_void,
        duplicate: *mut *mut c_void,
        access: u32,
        inherit: i32,
        options: u32,
    ) -> i32;
    fn GetFileType(handle: *mut c_void) -> u32;
    fn SetHandleInformation(handle: *mut c_void, mask: u32, flags: u32) -> i32;
}

pub(super) fn open(output: &str) -> io::Result<(Pipe, String)> {
    let file = if let Some(value) = output.strip_prefix("handle:") {
        let handle: usize = value
            .parse()
            .map_err(|_| io::Error::other("handle:N requires a handle number"))?;
        let mut duplicate = std::ptr::null_mut();
        // SAFETY: DuplicateHandle validates the supplied value and returns an owned handle.
        let success = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                handle as *mut _,
                GetCurrentProcess(),
                &mut duplicate,
                0,
                1,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if success == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: this handle was allocated by DuplicateHandle for this process.
        unsafe { File::from_raw_handle(duplicate) }
    } else if let Some(path) = output.strip_prefix("pipe:") {
        OpenOptions::new().write(true).open(path)?
    } else {
        return Err(io::Error::other("fd:N requires Unix"));
    };
    let handle = file.as_raw_handle();
    // SAFETY: file owns the handle being queried and marked for encoder inheritance.
    unsafe {
        if GetFileType(handle) != FILE_TYPE_PIPE {
            return Err(io::Error::other("log handle must be a pipe"));
        }
        let mut status = IoStatus::default();
        let mut mode = 0u32;
        let result = NtQueryInformationFile(
            handle,
            &mut status,
            &mut mode,
            std::mem::size_of_val(&mode) as u32,
            FILE_MODE_INFORMATION,
        );
        if result < 0 {
            return Err(io::Error::other(format!(
                "query log pipe mode: NTSTATUS {result:#x}"
            )));
        }
        if mode & FILE_SYNCHRONOUS_IO == 0 {
            return Err(io::Error::other("log handle must use synchronous I/O"));
        }
        if SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok((
        Pipe { file, limit: 512 },
        format!("handle:{}", handle as usize),
    ))
}
