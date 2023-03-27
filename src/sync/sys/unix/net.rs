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
use nix::sys::socket::*;
use std::io::{self};
use std::os::unix::io::RawFd;
use std::os::unix::prelude::AsRawFd;
use nix::Error;

use nix::unistd::*;
use std::sync::{Arc};
use std::sync::atomic::{AtomicBool, Ordering};
use crate::common::{self, client_connect, SOCK_CLOEXEC};
#[cfg(target_os = "macos")] 
use crate::common::set_fd_close_exec;
use nix::sys::socket::{self};

pub struct PipeListener {
    fd: RawFd,
    monitor_fd: (RawFd, RawFd),
}

impl AsRawFd for PipeListener {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl PipeListener {
    pub(crate) fn new(sockaddr: &str) -> Result<PipeListener> {
        let (fd, _) = common::do_bind(sockaddr)?;
        common::do_listen(fd)?;

        let fds = PipeListener::new_monitor_fd()?;

        Ok(PipeListener {
            fd,
            monitor_fd: fds,
        })
    }

    pub(crate) fn new_from_fd(fd: RawFd) -> Result<PipeListener> {
        let fds = PipeListener::new_monitor_fd()?;

        Ok(PipeListener {
            fd,
            monitor_fd: fds,
        })
    }

    fn new_monitor_fd() ->  Result<(i32, i32)> {
        #[cfg(target_os = "linux")]
        let fds = pipe2(nix::fcntl::OFlag::O_CLOEXEC)?;
 
        
        #[cfg(target_os = "macos")] 
        let fds = {
            let (rfd, wfd) = pipe()?;
            set_fd_close_exec(rfd)?;
            set_fd_close_exec(wfd)?;
            (rfd, wfd)
        };

        Ok(fds)
    }

    // accept returns:
    // - Ok(Some(PipeConnection)) if a new connection is established
    // - Ok(None) if spurious wake up with no new connection
    // - Err(io::Error) if there is an error and listener loop should be shutdown
    pub(crate) fn accept( &self, quit_flag: &Arc<AtomicBool>) ->  std::result::Result<Option<PipeConnection>, io::Error> {
        if quit_flag.load(Ordering::SeqCst) {
            return Err(io::Error::new(io::ErrorKind::Other, "listener shutdown for quit flag"));
        }
        
        let mut pollers = vec![
            libc::pollfd {
                fd: self.monitor_fd.0,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: self.fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];

        let returned = unsafe {
            let pollers: &mut [libc::pollfd] = &mut pollers;
            libc::poll(
                pollers as *mut _ as *mut libc::pollfd,
                pollers.len() as _,
                -1,
            )
        };

        if returned == -1 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                return Err(err);
            }

            error!("fatal error in listener_loop:{:?}", err);
            return Err(err);
        } else if returned < 1 {
            return Ok(None)
        }

        if pollers[0].revents != 0 || pollers[pollers.len() - 1].revents == 0 {
            return Ok(None);
        }

        if quit_flag.load(Ordering::SeqCst) {
            return Err(io::Error::new(io::ErrorKind::Other, "listener shutdown for quit flag"));
        }

        #[cfg(target_os = "linux")]
        let fd = match accept4(self.fd, SockFlag::SOCK_CLOEXEC) {
            Ok(fd) => fd,
            Err(e) => {
                error!("failed to accept error {:?}", e);
                return Err(std::io::Error::from_raw_os_error(e as i32));
            }
        };

        // Non Linux platforms do not support accept4 with SOCK_CLOEXEC flag, so instead
        // use accept and call fcntl separately to set SOCK_CLOEXEC.
        // Because of this there is chance of the descriptor leak if fork + exec happens in between.
        #[cfg(target_os = "macos")] 
        let fd = match accept(self.fd) {
            Ok(fd) => {
                if let Err(err) = set_fd_close_exec(fd) {
                    error!("fcntl failed after accept: {:?}", err);
                    return Err(io::Error::new(io::ErrorKind::Other, format!("{err:?}")));
                };
                fd
            }
            Err(e) => {
                error!("failed to accept error {:?}", e);
                return Err(std::io::Error::from_raw_os_error(e as i32));
            }
        };


        Ok(Some(PipeConnection { fd }))
    }

    pub fn close(&self) -> Result<()> {
        close(self.monitor_fd.1).unwrap_or_else(|e| {
            warn!(
                "failed to close notify fd: {} with error: {}",
                self.monitor_fd.1, e
            )
        });
        Ok(())
    }
}


pub struct PipeConnection {
    fd: RawFd,
}

impl PipeConnection {
    pub(crate) fn new(fd: RawFd) -> PipeConnection {
        PipeConnection { fd }
    }

    pub(crate) fn id(&self) -> i32 {
        self.fd
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize> {
        loop {
            match  recv(self.fd, buf, MsgFlags::empty()) {
                Ok(l) => return Ok(l),
                Err(e) if retryable(e) => {
                    // Should retry
                    continue;
                }
                Err(e) => {
                    return Err(crate::Error::Nix(e));
                }
            }
        }
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize> {
        loop {
            match send(self.fd, buf, MsgFlags::empty()) {
                Ok(l) => return Ok(l),
                Err(e) if retryable(e) => {
                    // Should retry
                    continue;
                }
                Err(e) => {
                    return Err(crate::Error::Nix(e));
                }
            }
        }
    }

    pub fn close(&self) -> Result<()> {
        match close(self.fd) {
            Ok(_) => Ok(()),
            Err(e) => Err(crate::Error::Nix(e))
        }
    }

    pub fn shutdown(&self) -> Result<()> {
        match socket::shutdown(self.fd, Shutdown::Read) {
            Ok(_) => Ok(()),
            Err(e) => Err(crate::Error::Nix(e))
        }
    }
}

pub struct ClientConnection {
    fd: RawFd,
    socket_pair: (RawFd, RawFd),
}

impl ClientConnection {
    pub fn client_connect(sockaddr: &str)-> Result<ClientConnection>   {
        let fd = unsafe { client_connect(sockaddr)? };
        Ok(ClientConnection::new(fd))
    }

    pub(crate) fn new(fd: RawFd) -> ClientConnection {
        let (recver_fd, close_fd) =
            socketpair(AddressFamily::Unix, SockType::Stream, None, SOCK_CLOEXEC).unwrap();

        // MacOS doesn't support descriptor creation with SOCK_CLOEXEC automically,
        // so there is a chance of leak if fork + exec happens in between of these calls.
        #[cfg(target_os = "macos")]
        {
            set_fd_close_exec(recver_fd).unwrap();
            set_fd_close_exec(close_fd).unwrap();
        }


        ClientConnection { 
            fd, 
            socket_pair: (recver_fd, close_fd) 
        }
    }

    pub fn ready(&self) -> std::result::Result<Option<()>, io::Error> {
        let mut pollers = vec![
            libc::pollfd {
                fd: self.socket_pair.0,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: self.fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];

        let returned = unsafe {
            let pollers: &mut [libc::pollfd] = &mut pollers;
            libc::poll(
                pollers as *mut _ as *mut libc::pollfd,
                pollers.len() as _,
                -1,
            )
        };

        if returned == -1 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                return Ok(None)
            }

            error!("fatal error in process reaper:{}", err);
            return Err(err);
        } else if returned < 1 {
            return Ok(None)
        }

        if pollers[0].revents != 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "pipe closed"));
        }

        if pollers[pollers.len() - 1].revents == 0 {
            return Ok(None)
        }

        Ok(Some(()))
    }

    pub fn get_pipe_connection(&self) -> Result<PipeConnection> {
        Ok(PipeConnection::new(self.fd))
    }

    pub fn close_receiver(&self) -> Result<()> {
        match close(self.socket_pair.0) {
            Ok(_) => Ok(()),
            Err(e) => Err(crate::Error::Nix(e))
        }
    }

    pub fn close(&self) -> Result<()> {
        match close(self.socket_pair.1) {
            Ok(_) => {},
            Err(e) => return Err(crate::Error::Nix(e))
        };

        match close(self.fd) {
            Ok(_) => Ok(()),
            Err(e) => Err(crate::Error::Nix(e))
        }
    }
}

fn retryable(e: nix::Error) -> bool {
    e == Error::EINTR || e == Error::EAGAIN
}
