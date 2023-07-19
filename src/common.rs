#![cfg(not(windows))]
// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

//! Common functions.

#[cfg(any(
    feature = "async",
    not(any(target_os = "linux", target_os = "android"))
))]
use nix::fcntl::FdFlag;
use nix::fcntl::{fcntl, FcntlArg, OFlag};
use nix::sys::socket::*;
use std::os::unix::io::RawFd;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Domain {
    Unix,
    #[cfg(any(target_os = "linux", target_os = "android"))]
    Vsock,
}

pub(crate) fn do_listen(listener: RawFd) -> Result<()> {
    if let Err(e) = fcntl(listener, FcntlArg::F_SETFL(OFlag::O_NONBLOCK)) {
        return Err(Error::Others(format!(
            "failed to set listener fd: {listener} as non block: {e}"
        )));
    }

    listen(listener, 10).map_err(|e| Error::Socket(e.to_string()))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn parse_sockaddr(addr: &str) -> Result<(Domain, &str)> {
    if let Some(addr) = addr.strip_prefix("unix://") {
        return Ok((Domain::Unix, addr));
    }

    if let Some(addr) = addr.strip_prefix("vsock://") {
        return Ok((Domain::Vsock, addr));
    }

    Err(Error::Others(format!("Scheme {addr:?} is not supported")))
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn parse_sockaddr(addr: &str) -> Result<(Domain, &str)> {
    if let Some(addr) = addr.strip_prefix("unix://") {
        if addr.starts_with('@') {
            return Err(Error::Others(
                "Abstract unix domain socket is not support on this platform".to_string(),
            ));
        }
        return Ok((Domain::Unix, addr));
    }

    Err(Error::Others(format!("Scheme {addr:?} is not supported")))
}

#[cfg(any(
    feature = "async",
    not(any(target_os = "linux", target_os = "android"))
))]
pub(crate) fn set_fd_close_exec(fd: RawFd) -> Result<RawFd> {
    if let Err(e) = fcntl(fd, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC)) {
        return Err(Error::Others(format!(
            "failed to set fd: {fd} as close-on-exec: {e}"
        )));
    }
    Ok(fd)
}

// SOCK_CLOEXEC flag is Linux specific
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) const SOCK_CLOEXEC: SockFlag = SockFlag::SOCK_CLOEXEC;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub(crate) const SOCK_CLOEXEC: SockFlag = SockFlag::empty();

#[cfg(any(target_os = "linux", target_os = "android"))]
fn make_addr(domain: Domain, sockaddr: &str) -> Result<UnixAddr> {
    match domain {
        Domain::Unix => {
            if let Some(sockaddr) = sockaddr.strip_prefix('@') {
                UnixAddr::new_abstract(sockaddr.as_bytes()).map_err(err_to_others_err!(e, ""))
            } else {
                UnixAddr::new(sockaddr).map_err(err_to_others_err!(e, ""))
            }
        }
        Domain::Vsock => Err(Error::Others(
            "function make_addr does not support create vsock socket".to_string(),
        )),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn make_addr(_domain: Domain, sockaddr: &str) -> Result<UnixAddr> {
    UnixAddr::new(sockaddr).map_err(err_to_others_err!(e, ""))
}

fn make_socket(addr: (&str, u32)) -> Result<(RawFd, Domain, Box<dyn SockaddrLike>)> {
    let (sockaddr, _) = addr;
    let (domain, sockaddrv) = parse_sockaddr(sockaddr)?;

    let get_sock_addr = |domain, sockaddr| -> Result<(RawFd, Box<dyn SockaddrLike>)> {
        let fd = socket(AddressFamily::Unix, SockType::Stream, SOCK_CLOEXEC, None)
            .map_err(|e| Error::Socket(e.to_string()))?;

        // MacOS doesn't support atomic creation of a socket descriptor with SOCK_CLOEXEC flag,
        // so there is a chance of leak if fork + exec happens in between of these calls.
        #[cfg(target_os = "macos")]
        set_fd_close_exec(fd)?;
        let sockaddr = make_addr(domain, sockaddr)?;
        Ok((fd, Box::new(sockaddr)))
    };

    let (fd, sockaddr): (i32, Box<dyn SockaddrLike>) = match domain {
        Domain::Unix => get_sock_addr(domain, sockaddrv)?,
        #[cfg(any(target_os = "linux", target_os = "android"))]
        Domain::Vsock => {
            let sockaddr_port_v: Vec<&str> = sockaddrv.split(':').collect();
            if sockaddr_port_v.len() != 2 {
                return Err(Error::Others(format!(
                    "sockaddr {sockaddr} is not right for vsock"
                )));
            }
            let port: u32 = sockaddr_port_v[1]
                .parse()
                .expect("the vsock port is not an number");
            let fd = socket(
                AddressFamily::Vsock,
                SockType::Stream,
                SockFlag::SOCK_CLOEXEC,
                None,
            )
            .map_err(|e| Error::Socket(e.to_string()))?;
            let cid = addr.1;
            let sockaddr = VsockAddr::new(cid, port);
            (fd, Box::new(sockaddr))
        }
    };

    Ok((fd, domain, sockaddr))
}

// Vsock is not supported on non Linux.
#[cfg(any(target_os = "linux", target_os = "android"))]
use libc::VMADDR_CID_ANY;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const VMADDR_CID_ANY: u32 = 0;
#[cfg(any(target_os = "linux", target_os = "android"))]
use libc::VMADDR_CID_HOST;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const VMADDR_CID_HOST: u32 = 0;

pub(crate) fn do_bind(sockaddr: &str) -> Result<(RawFd, Domain)> {
    let (fd, domain, sockaddr) = make_socket((sockaddr, VMADDR_CID_ANY))?;

    setsockopt(fd, sockopt::ReusePort, &true)?;
    bind(fd, sockaddr.as_ref()).map_err(err_to_others_err!(e, ""))?;

    Ok((fd, domain))
}

/// Creates a unix socket for client.
pub(crate) unsafe fn client_connect(sockaddr: &str) -> Result<RawFd> {
    let (fd, _, sockaddr) = make_socket((sockaddr, VMADDR_CID_HOST))?;

    connect(fd, sockaddr.as_ref())?;

    Ok(fd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn test_parse_sockaddr() {
        for i in &[
            (
                "unix:///run/a.sock",
                Some(Domain::Unix),
                "/run/a.sock",
                true,
            ),
            ("vsock://8:1024", Some(Domain::Vsock), "8:1024", true),
            ("Vsock://8:1025", Some(Domain::Vsock), "8:1025", false),
            (
                "unix://@/run/b.sock",
                Some(Domain::Unix),
                "@/run/b.sock",
                true,
            ),
            ("abc:///run/c.sock", None, "", false),
        ] {
            let (input, domain, addr, success) = (i.0, i.1, i.2, i.3);
            let r = parse_sockaddr(input);
            if success {
                let (rd, ra) = r.unwrap();
                assert_eq!(rd, domain.unwrap());
                assert_eq!(ra, addr);
            } else {
                assert!(r.is_err());
            }
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    #[test]
    fn test_parse_sockaddr() {
        for i in &[
            (
                "unix:///run/a.sock",
                Some(Domain::Unix),
                "/run/a.sock",
                true,
            ),
            ("vsock:///run/c.sock", None, "", false),
            ("Vsock:///run/c.sock", None, "", false),
            ("unix://@/run/b.sock", None, "", false),
            ("abc:///run/c.sock", None, "", false),
        ] {
            let (input, domain, addr, success) = (i.0, i.1, i.2, i.3);
            let r = parse_sockaddr(input);
            if success {
                let (rd, ra) = r.unwrap();
                assert_eq!(rd, domain.unwrap());
                assert_eq!(ra, addr);
            } else {
                assert!(r.is_err());
            }
        }
    }
}
