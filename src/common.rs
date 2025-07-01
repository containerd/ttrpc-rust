#![cfg(not(windows))]
// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

//! Common functions.

use nix::fcntl::{fcntl, FcntlArg, OFlag};
use nix::sys::socket::*;
use std::str::FromStr;
use std::{env, os::unix::io::RawFd};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Domain {
    Unix,
    Tcp,
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

    if let Some(addr) = addr.strip_prefix("tcp://") {
        return Ok((Domain::Tcp, addr));
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

    if let Some(addr) = addr.strip_prefix("tcp://") {
        return Ok((Domain::Tcp, addr));
    }

    Err(Error::Others(format!("Scheme {addr:?} is not supported")))
}

#[cfg(target_os = "macos")]
pub(crate) fn set_fd_close_exec(fd: RawFd) -> Result<RawFd> {
    use nix::fcntl::FdFlag;
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
        Domain::Vsock | Domain::Tcp => Err(Error::Others(
            "function make_addr does not support create vsock/tcp socket".to_string(),
        )),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn make_addr(_domain: Domain, sockaddr: &str) -> Result<UnixAddr> {
    UnixAddr::new(sockaddr).map_err(err_to_others_err!(e, ""))
}

// addr: cid:port
// return (cid, port)
#[cfg(any(target_os = "linux", target_os = "android"))]
fn parse_vscok(addr: &str) -> Result<(u32, u32)> {
    // vsock://cid:port
    let sockaddr_port_v: Vec<&str> = addr.split(':').collect();
    if sockaddr_port_v.len() != 2 {
        return Err(Error::Others(format!(
            "sockaddr {addr} is not right for vsock"
        )));
    }

    // for -1 need trace to libc::VMADDR_CID_ANY
    let cid: u32 = if sockaddr_port_v[0].trim().eq("-1") {
        libc::VMADDR_CID_ANY
    } else {
        sockaddr_port_v[0].parse().map_err(|e| {
            Error::Others(format!(
                "failed to parse cid from {:?} error: {:?}",
                sockaddr_port_v[0], e
            ))
        })?
    };

    let port: u32 = sockaddr_port_v[1].parse().map_err(|e| {
        Error::Others(format!(
            "failed to parse port from {:?} error: {:?}",
            sockaddr_port_v[1], e
        ))
    })?;
    Ok((cid, port))
}

fn make_socket(sockaddr: &str) -> Result<(RawFd, Domain, Box<dyn SockaddrLike>)> {
    let (domain, sockaddrv) = parse_sockaddr(sockaddr)?;

    let get_unix_addr = |domain, sockaddr| -> Result<(RawFd, Box<dyn SockaddrLike>)> {
        let fd = socket(AddressFamily::Unix, SockType::Stream, SOCK_CLOEXEC, None)
            .map_err(|e| Error::Socket(e.to_string()))?;

        // MacOS doesn't support atomic creation of a socket descriptor with SOCK_CLOEXEC flag,
        // so there is a chance of leak if fork + exec happens in between of these calls.
        #[cfg(target_os = "macos")]
        set_fd_close_exec(fd)?;
        let sockaddr = make_addr(domain, sockaddr)?;
        Ok((fd, Box::new(sockaddr)))
    };
    let get_tcp_addr = |sockaddr: &str| -> Result<(RawFd, Box<dyn SockaddrLike>)> {
        let fd = socket(AddressFamily::Inet, SockType::Stream, SOCK_CLOEXEC, None)
            .map_err(|e| Error::Socket(e.to_string()))?;

        #[cfg(target_os = "macos")]
        set_fd_close_exec(fd)?;
        let sockaddr = SockaddrIn::from_str(sockaddr).map_err(err_to_others_err!(e, ""))?;

        Ok((fd, Box::new(sockaddr)))
    };

    let (fd, sockaddr): (i32, Box<dyn SockaddrLike>) = match domain {
        Domain::Unix => get_unix_addr(domain, sockaddrv)?,
        Domain::Tcp => get_tcp_addr(sockaddrv)?,
        #[cfg(any(target_os = "linux", target_os = "android"))]
        Domain::Vsock => {
            let (cid, port) = parse_vscok(sockaddrv)?;
            let fd = socket(
                AddressFamily::Vsock,
                SockType::Stream,
                SockFlag::SOCK_CLOEXEC,
                None,
            )
            .map_err(|e| Error::Socket(e.to_string()))?;
            let sockaddr = VsockAddr::new(cid, port);
            (fd, Box::new(sockaddr))
        }
    };

    Ok((fd, domain, sockaddr))
}

fn set_socket_opts(fd: RawFd, domain: Domain, is_bind: bool) -> Result<()> {
    if domain != Domain::Tcp {
        return Ok(());
    }

    if is_bind {
        setsockopt(fd, sockopt::ReusePort, &true)?;
    }

    let tcp_nodelay_enabled = match env::var("TTRPC_TCP_NODELAY_ENABLED") {
        Ok(val) if val == "1" || val.eq_ignore_ascii_case("true") => true,
        Ok(val) if val == "0" || val.eq_ignore_ascii_case("false") => false,
        _ => false,
    };
    if tcp_nodelay_enabled {
        setsockopt(fd, sockopt::TcpNoDelay, &true)?;
    }

    Ok(())
}

pub(crate) fn do_bind(sockaddr: &str) -> Result<(RawFd, Domain)> {
    let (fd, domain, sockaddr) = make_socket(sockaddr)?;

    set_socket_opts(fd, domain, true)?;
    bind(fd, sockaddr.as_ref()).map_err(err_to_others_err!(e, ""))?;

    Ok((fd, domain))
}

/// Creates a unix socket for client.
pub(crate) unsafe fn client_connect(sockaddr: &str) -> Result<RawFd> {
    let (fd, domain, sockaddr) = make_socket(sockaddr)?;

    set_socket_opts(fd, domain, false)?;
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
            (
                "tcp://127.0.0.1:65500",
                Some(Domain::Tcp),
                "127.0.0.1:65500",
                true,
            ),
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
            (
                "tcp://127.0.0.1:65500",
                Some(Domain::Tcp),
                "127.0.0.1:65500",
                true,
            ),
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
    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn test_parse_vscok() {
        for i in &[
            ("-1:1024", (libc::VMADDR_CID_ANY, 1024)),
            ("0:1", (0, 1)),
            ("1:2", (1, 2)),
            ("4294967294:3", (4294967294, 3)),
            // 4294967295 = 0xFFFFFFFF
            ("4294967295:4", (libc::VMADDR_CID_ANY, 4)),
        ] {
            let (input, (cid, port)) = (i.0, i.1);
            let r = parse_vscok(input);
            assert_eq!(r.unwrap(), (cid, port), "parse {:?} failed", i);
        }
    }
}
