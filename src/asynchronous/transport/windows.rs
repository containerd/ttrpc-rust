use std::convert::TryFrom;
use std::io::{Error as IoError, Result as IoResult};
use std::mem::{replace, zeroed};
use std::os::windows::ffi::OsStringExt as _;
use std::time::Duration;
use std::os::windows::io::AsRawHandle;

use async_stream::stream;
use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};
use tokio::time::sleep;
use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_PIPE_BUSY};
use windows_sys::Win32::Storage::FileSystem::*;

use super::{Listener, Socket};

impl Listener {
    pub fn bind_named_pipe(addr: impl AsRef<str>) -> IoResult<Self> {
        let listener = ServerOptions::new()
            .first_pipe_instance(true)
            .create(addr.as_ref())?;
        Self::try_from(listener)
    }
}

impl Socket {
    pub async fn connect_named_pipe(addr: impl AsRef<str>) -> IoResult<Self> {
        let client = loop {
            match ClientOptions::new().open(addr.as_ref()) {
                Ok(client) => break client,
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => (),
                Err(e) => return Err(e),
            }
            sleep(Duration::from_millis(50)).await;
        };

        Ok(Self::from(client))
    }
}

impl TryFrom<NamedPipeServer> for Listener {
    type Error = IoError;
    fn try_from(mut listener: NamedPipeServer) -> IoResult<Self> {
        let name = get_pipe_name(&listener)?;
        Ok(Self::new(stream! {
            loop {
                yield listener
                    .connect()
                    .await
                    .and_then(|_| ServerOptions::new().create(&name))
                    .map(|l| replace(&mut listener, l));
            }
        }))
    }
}

impl From<NamedPipeServer> for Socket {
    fn from(socket: NamedPipeServer) -> Self {
        Self::new(socket)
    }
}

impl From<NamedPipeClient> for Socket {
    fn from(socket: NamedPipeClient) -> Self {
        Self::new(socket)
    }
}

fn get_pipe_name(pipe: &impl AsRawHandle) -> IoResult<std::ffi::OsString> {
    let mut info: FILE_NAME_INFO = unsafe { zeroed() };
    let handle = pipe.as_raw_handle();

    let success = unsafe {
        GetFileInformationByHandleEx(
            handle as _,
            FileNameInfo,
            &mut info as *mut FILE_NAME_INFO as _,
            size_of::<FILE_NAME_INFO>() as _,
        )
    };

    // First call to GetFileInformationByHandleEx should fail with ERROR_MORE_DATA.
    // This gives us the size we need to allocate to store the data.
    if success == 0 {
        let err = IoError::last_os_error();
        if err.raw_os_error() != Some(ERROR_MORE_DATA as i32) {
            return Err(err);
        }
    }

    // Allocate enough space for a second call to GetFileInformationByHandleEx.
    let size = size_of::<FILE_NAME_INFO>() + info.FileNameLength as usize;
    let mut info = vec![0u8; size];

    let success = unsafe {
        GetFileInformationByHandleEx(handle as _, FileNameInfo, info.as_mut_ptr() as _, size as _)
    };

    // If the second call fails, bail out.
    if success == 0 {
        return Err(IoError::last_os_error());
    }

    let info = info.as_ptr() as *const FILE_NAME_INFO;
    let info = unsafe { info.as_ref() }.unwrap();

    let name = unsafe {
        std::slice::from_raw_parts(info.FileName.as_ptr() as _, (info.FileNameLength / 2) as _)
    };
    let name = std::ffi::OsString::from_wide(name);
    let mut full_name = std::ffi::OsString::from(r"\\.\pipe");
    full_name.extend([name]);

    Ok(full_name)
}

#[tokio::test]
async fn test_get_pipe_name() {
    let listener = ServerOptions::new()
        .first_pipe_instance(true)
        .create(r"\\.\pipe\hello_world")
        .unwrap();

    let name = get_pipe_name(&listener)
        .unwrap()
        .to_string_lossy()
        .to_string();

    assert_eq!(name, r"\\.\pipe\hello_world");
}
