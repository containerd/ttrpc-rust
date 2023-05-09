/*
	Copyright The containerd Authors.

	Licensed under the Apache License, Version 2.0 (the "License");
	you may not use this file except in compliance with the License.
	You may obtain a copy of the License at

		http://www.apache.org/licenses/LICENSE-2.0

	Unless required by applicable law or agreed to in writing, software
	distributed under the License is distributed on an "AS IS" BASIS,
	WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
	See the License for the specific language governing permissions and
	limitations under the License.
*/

use crate::error::Result;
use crate::error::Error;
use std::cell::UnsafeCell;
use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{IntoRawHandle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc};
use std::{io};

use windows_sys::Win32::Foundation::{ CloseHandle, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE };
use windows_sys::Win32::Storage::FileSystem::{ ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX };
use windows_sys::Win32::System::IO::{ GetOverlappedResult, OVERLAPPED };
use windows_sys::Win32::System::Pipes::{ CreateNamedPipeW, ConnectNamedPipe,DisconnectNamedPipe, PIPE_WAIT, PIPE_UNLIMITED_INSTANCES, PIPE_REJECT_REMOTE_CLIENTS };
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent};

const PIPE_BUFFER_SIZE: u32 = 65536;
const WAIT_FOR_EVENT: i32 = 1;

pub struct PipeListener {
    first_instance: AtomicBool,
    shutting_down: AtomicBool,
    address: String,
    connection_event: isize,
}

#[repr(C)]
struct Overlapped {
    inner: UnsafeCell<OVERLAPPED>,
}

impl Overlapped {
    fn new_with_event(event: isize) -> Overlapped  {        
        let mut ol = Overlapped {
            inner: UnsafeCell::new(unsafe { std::mem::zeroed() }),
        };
        ol.inner.get_mut().hEvent = event;
        ol
    }

    fn as_mut_ptr(&self) -> *mut OVERLAPPED {
        self.inner.get()
    }
}

impl PipeListener {
    pub(crate) fn new(sockaddr: &str) -> Result<PipeListener> {
        let connection_event = create_event()?;
        Ok(PipeListener {
            first_instance: AtomicBool::new(true),
            shutting_down: AtomicBool::new(false),
            address: sockaddr.to_string(),
            connection_event
        })
    }

    // accept returns:
    // - Ok(Some(PipeConnection)) if a new connection is established
    // - Err(io::Error) if there is an error and listener loop should be shutdown
    pub(crate) fn accept(&self, quit_flag: &Arc<AtomicBool>) -> std::result::Result<Option<PipeConnection>, io::Error> {
        if quit_flag.load(Ordering::SeqCst) {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "listener shutdown for quit flag",
            ));
        }

        // Create a new pipe instance for every new client
        let instance = self.new_instance()?;
        let np = match PipeConnection::new(instance) {
            Ok(np) => np,
            Err(e) => {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("failed to create new pipe instance: {:?}", e),
                ));
            }
        };
        
        let ol = Overlapped::new_with_event(self.connection_event);

        trace!("listening for connection");
        let result = unsafe { ConnectNamedPipe(np.named_pipe, ol.as_mut_ptr())};
        if result != 0 {
            if let Some(error) = self.handle_shutdown(&np) {
                return Err(error);
            }
            return Err(io::Error::last_os_error());
        }

        match io::Error::last_os_error() {
            e if e.raw_os_error() == Some(ERROR_IO_PENDING as i32) => {
                let mut bytes_transfered = 0;
                let res = unsafe {GetOverlappedResult(np.named_pipe, ol.as_mut_ptr(), &mut bytes_transfered, WAIT_FOR_EVENT) };
                match res {
                    0 => {
                        return Err(io::Error::last_os_error());
                    }
                    _ => {
                        if let Some(shutdown_signal) = self.handle_shutdown(&np) {
                            return Err(shutdown_signal);
                        }
                        Ok(Some(np))
                    }
                }
            }
            e if e.raw_os_error() == Some(ERROR_PIPE_CONNECTED as i32) => {
                if let Some(error) = self.handle_shutdown(&np) {
                    return Err(error);
                }
                Ok(Some(np))
            }
            e => {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("failed to connect pipe: {:?}", e),
                ));
            }
        }
    }

    fn handle_shutdown(&self, np: &PipeConnection) -> Option<io::Error> {
        if self.shutting_down.load(Ordering::SeqCst) {
            np.close().unwrap_or_else(|err| trace!("Failed to close the pipe {:?}", err));
            return Some(io::Error::new(
                io::ErrorKind::Other,
                "closing pipe",
            ));
        }
        None
    }

    fn new_instance(&self) -> io::Result<isize> {
        let name = OsStr::new(&self.address.as_str())
            .encode_wide()
            .chain(Some(0)) // add NULL termination
            .collect::<Vec<_>>();

        let mut open_mode = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED ;

        if self.first_instance.load(Ordering::SeqCst) {
            open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
            self.first_instance.swap(false, Ordering::SeqCst);
        }

        // null for security attributes means the handle cannot be inherited and write access is restricted to system
        // https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights
        match  unsafe { CreateNamedPipeW(name.as_ptr(), open_mode, PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS, PIPE_UNLIMITED_INSTANCES, PIPE_BUFFER_SIZE, PIPE_BUFFER_SIZE, 0, std::ptr::null_mut())} {
            INVALID_HANDLE_VALUE => {
                return Err(io::Error::last_os_error())
            }
            h => {
                return Ok(h)
            },
        };
    }

    pub fn close(&self) -> Result<()> {
        // release the ConnectNamedPipe thread by signaling the event and clean up event handle
        self.shutting_down.store(true, Ordering::SeqCst);
        set_event(self.connection_event)?;
        close_handle(self.connection_event)
    }
}

pub struct PipeConnection {
    named_pipe: isize,
    read_event: isize,
    write_event: isize,
}

// PipeConnection on Windows is used by both the Server and Client to read and write to the named pipe
// The named pipe is created with the overlapped flag enable the simultaneous read and write operations.
// This is required since a read and write be issued at the same time on a given named pipe instance.
//
// An event is created for the read and write operations.  When the read or write is issued
// it either returns immediately or the thread is suspended until the event is signaled when 
// the overlapped (async) operation completes and the event is triggered allow the thread to continue.
// 
// Due to the implementation of the sync Server and client there is always only one read and one write 
// operation in flight at a time so we can reuse the same event.
// 
// For more information on overlapped and events: https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-getoverlappedresult#remarks
// "It is safer to use an event object because of the confusion that can occur when multiple simultaneous overlapped operations are performed on the same file, named pipe, or communications device." 
// "In this situation, there is no way to know which operation caused the object's state to be signaled."
impl PipeConnection {
    pub(crate) fn new(h: isize) -> Result<PipeConnection> {
        trace!("creating events for thread {:?} on pipe instance {}", std::thread::current().id(), h as i32);
        let read_event = create_event()?;
        let write_event = create_event()?;
        Ok(PipeConnection {
            named_pipe: h,
            read_event: read_event,
            write_event: write_event,
        })
    }

    pub(crate) fn id(&self) -> i32 {
        self.named_pipe as i32
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize> {
        trace!("starting read for thread {:?} on pipe instance {}", std::thread::current().id(), self.named_pipe as i32);
        let ol = Overlapped::new_with_event(self.read_event);

        let len = std::cmp::min(buf.len(), u32::MAX as usize) as u32;
        let mut bytes_read= 0;
        let result = unsafe { ReadFile(self.named_pipe, buf.as_mut_ptr() as *mut _, len, &mut bytes_read,ol.as_mut_ptr()) };
        if result > 0 && bytes_read > 0 {
            // Got result no need to wait for pending read to complete
            return Ok(bytes_read as usize)
        }

        // wait for pending operation to complete (thread will be suspended until event is signaled)
        match io::Error::last_os_error() {
            ref e if e.raw_os_error() == Some(ERROR_IO_PENDING as i32) => {
                let mut bytes_transfered = 0;
                let res = unsafe {GetOverlappedResult(self.named_pipe, ol.as_mut_ptr(), &mut bytes_transfered, WAIT_FOR_EVENT) };
                match res {
                    0 => {
                        return Err(handle_windows_error(io::Error::last_os_error()))
                    }
                    _ => {
                        return Ok(bytes_transfered as usize)
                    }
                }
            }
            ref e => {
                return Err(Error::Others(format!("failed to read from pipe: {:?}", e)))
            }
        }
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize> {
        trace!("starting write for thread {:?} on pipe instance {}", std::thread::current().id(), self.named_pipe as i32);
        let ol = Overlapped::new_with_event(self.write_event);
        let mut bytes_written = 0;
        let len = std::cmp::min(buf.len(), u32::MAX as usize) as u32;
        let result = unsafe { WriteFile(self.named_pipe, buf.as_ptr() as *const _,len, &mut bytes_written, ol.as_mut_ptr())};
        if result > 0 && bytes_written > 0 {
            // No need to wait for pending write to complete
            return Ok(bytes_written as usize)
        }

        // wait for pending operation to complete (thread will be suspended until event is signaled)
        match io::Error::last_os_error() {
            ref e if e.raw_os_error() == Some(ERROR_IO_PENDING as i32) => {
                let mut bytes_transfered = 0;
                let res = unsafe {GetOverlappedResult(self.named_pipe, ol.as_mut_ptr(), &mut bytes_transfered, WAIT_FOR_EVENT) };
                match res {
                    0 => {
                        return Err(handle_windows_error(io::Error::last_os_error()))
                    }
                    _ => {
                        return Ok(bytes_transfered as usize)
                    }
                }
            }
            ref e => {
                return Err(Error::Others(format!("failed to write to pipe: {:?}", e)))
            }
        }
    }

    pub fn close(&self) -> Result<()> {
        close_handle(self.named_pipe)?;
        close_handle(self.read_event)?;
        close_handle(self.write_event)
    }

    pub fn shutdown(&self) -> Result<()> {
        let result = unsafe { DisconnectNamedPipe(self.named_pipe) };
        match result {
            0 => Err(handle_windows_error(io::Error::last_os_error())),
            _ => Ok(()),
        }
    }
}

pub struct ClientConnection {
    address: String
}

fn close_handle(handle: isize) -> Result<()> {
    let result = unsafe { CloseHandle(handle) };
    match result {
        0 => Err(handle_windows_error(io::Error::last_os_error())),
        _ => Ok(()),
    }
}

fn create_event() -> Result<isize> {
    let result = unsafe { CreateEventW(std::ptr::null_mut(), 0, 1, std::ptr::null_mut()) };
    match result {
        0 => Err(handle_windows_error(io::Error::last_os_error())),
        _ => Ok(result),
    }
}

fn set_event(event: isize) -> Result<()> {
    let result = unsafe { SetEvent(event) };
    match result {
        0 => Err(handle_windows_error(io::Error::last_os_error())),
        _ => Ok(()),
    }
}

impl ClientConnection {
    pub fn client_connect(sockaddr: &str) -> Result<ClientConnection> {
        Ok(ClientConnection::new(sockaddr))
    }

    pub(crate) fn new(sockaddr: &str) -> ClientConnection {       
        ClientConnection {
            address: sockaddr.to_string()
        }
    }

    pub fn ready(&self) -> std::result::Result<Option<()>, io::Error> {
        // Windows is a "completion" based system so "readiness" isn't really applicable 
        Ok(Some(()))
    }

    pub fn get_pipe_connection(&self) -> Result<PipeConnection> {
        let mut opts = OpenOptions::new();
        opts.read(true)
            .write(true)
            .custom_flags(FILE_FLAG_OVERLAPPED);
        match opts.open(self.address.as_str()) {
            Ok(file) => {
                return PipeConnection::new(file.into_raw_handle() as isize)
            }
            Err(e) => {
                return Err(handle_windows_error(e))
            }
        }
    }

    pub fn close_receiver(&self) -> Result<()> {
        // close the pipe from the pipe connection
        Ok(())
    }

    pub fn close(&self) -> Result<()> {
        // close the pipe from the pipe connection
        Ok(())
    }
}

fn handle_windows_error(e: io::Error) -> Error {
    if let Some(raw_os_err) = e.raw_os_error() {
        Error::Windows(raw_os_err)
    } else {
        Error::Others(e.to_string())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;

    #[test]
    fn test_pipe_connection() {
        let client = ClientConnection::new("non_existent_pipe");
        match client.get_pipe_connection() {
            Ok(_) => {
                assert!(false, "should not be able to get a connection to a non existent pipe");
            }
            Err(e) => {
                assert_eq!(e, Error::Windows(ERROR_FILE_NOT_FOUND as i32));
            }
        }
    }

    #[test]
    fn should_accept_new_client() {
        let address = r"\\.\pipe\ttrpc-test-accept";
        let listener = Arc::new(PipeListener::new(address).unwrap());

        let listener_server = listener.clone();
        let thread = std::thread::spawn(move || {
            let quit_flag = Arc::new(AtomicBool::new(false));
            match listener_server.accept(&quit_flag) {
                Ok(Some(_)) => {
                    // pipe is working
                }
                Ok(None) => {
                    assert!(false, "should get a working pipe")
                }
                Err(e) => {
                    assert!(false, "should not get error {}", e.to_string())
                }
            }
        });

        wait_socket_working(address, 10, 5).unwrap();
        thread.join().unwrap();
    }

    #[test]
    fn close_should_cancel_accept() {
        let listener = Arc::new(PipeListener::new(r"\\.\pipe\ttrpc-test-close").unwrap());

        let listener_server = listener.clone();
        let thread = std::thread::spawn(move || {
            let quit_flag = Arc::new(AtomicBool::new(false));
            match listener_server.accept(&quit_flag) {
                Ok(_) => {
                    assert!(false, "should not get pipe on close")
                }
                Err(e) => {
                    assert_eq!(e.to_string(), "closing pipe")
                }
            }
        });

        // sleep for a moment to allow the pipe to start initialize and be ready to accept new connection.
        // this simulates scenario where the thread is asleep and awaiting a connection
        std::thread::sleep(std::time::Duration::from_millis(500));
        listener.close().unwrap();
        thread.join().unwrap();
    }

    fn wait_socket_working(address: &str, interval_in_ms: u64, count: u32) -> Result<()> {
        for _i in 0..count {
            let client = match ClientConnection::client_connect(address) {
                Ok(c) => {
                    c
                }
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(interval_in_ms));
                    continue;
                }
            };

            match client.get_pipe_connection() {
                Ok(_) => {
                    return Ok(());
                }
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(interval_in_ms));
                }
            }
        }
        Err(Error::Others("timed out".to_string()))
    }    
}
