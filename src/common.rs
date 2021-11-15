// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

//! Common functions and macros.

#![allow(unused_macros)]

use crate::error::{Error, Result};
use nix::fcntl::{fcntl, FcntlArg, FdFlag, OFlag};
use nix::sys::socket::*;
use std::os::unix::io::RawFd;

#[derive(Debug)]
pub enum Domain {
    Unix,
    #[cfg(target_os = "linux")]
    Vsock,
}

/// Message header of ttrpc.
#[derive(Default, Debug)]
pub struct MessageHeader {
    pub length: u32,
    pub stream_id: u32,
    pub type_: u8,
    pub flags: u8,
}

pub fn do_listen(listener: RawFd) -> Result<()> {
    if let Err(e) = fcntl(listener, FcntlArg::F_SETFL(OFlag::O_NONBLOCK)) {
        return Err(Error::Others(format!(
            "failed to set listener fd: {} as non block: {}",
            listener, e
        )));
    }

    listen(listener, 10).map_err(|e| Error::Socket(e.to_string()))
}

pub fn parse_host(host: &str) -> Result<(Domain, Vec<&str>)> {
    let hostv: Vec<&str> = host.trim().split("://").collect();
    if hostv.len() != 2 {
        return Err(Error::Others(format!("Host {} is not right", host)));
    }

    let domain = match &hostv[0].to_lowercase()[..] {
        "unix" => Domain::Unix,
        #[cfg(target_os = "linux")]
        "vsock" => Domain::Vsock,
        x => return Err(Error::Others(format!("Scheme {:?} is not supported", x))),
    };

    Ok((domain, hostv))
}

pub fn set_fd_close_exec(fd: RawFd) -> Result<RawFd> {
    if let Err(e) = fcntl(fd, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC)) {
        return Err(Error::Others(format!(
            "failed to set fd: {} as close-on-exec: {}",
            fd, e
        )));
    }
    Ok(fd)
}

// SOCK_CLOEXEC flag is Linux specific
#[cfg(target_os = "linux")]
pub(crate) const SOCK_CLOEXEC: SockFlag = SockFlag::SOCK_CLOEXEC;
#[cfg(not(target_os = "linux"))]
pub(crate) const SOCK_CLOEXEC: SockFlag = SockFlag::empty();

#[cfg(target_os = "linux")]
fn make_addr(host: &str) -> Result<UnixAddr> {
    UnixAddr::new_abstract(host.as_bytes()).map_err(err_to_others_err!(e, ""))
}

#[cfg(not(target_os = "linux"))]
fn make_addr(host: &str) -> Result<UnixAddr> {
    UnixAddr::new(host).map_err(err_to_others_err!(e, ""))
}

fn make_socket(addr: (&str, u32)) -> Result<(RawFd, Domain, SockAddr)> {
    let (host, _) = addr;
    let (domain, hostv) = parse_host(host)?;

    let sockaddr: SockAddr;
    let fd: RawFd;

    match domain {
        Domain::Unix => {
            fd = socket(AddressFamily::Unix, SockType::Stream, SOCK_CLOEXEC, None)
                .map_err(|e| Error::Socket(e.to_string()))?;

            // MacOS doesn't support atomic creation of a socket descriptor with SOCK_CLOEXEC flag,
            // so there is a chance of leak if fork + exec happens in between of these calls.
            #[cfg(target_os = "macos")]
            set_fd_close_exec(fd)?;

            sockaddr = SockAddr::Unix(make_addr(hostv[1])?);
        }
        #[cfg(target_os = "linux")]
        Domain::Vsock => {
            let host_port_v: Vec<&str> = hostv[1].split(':').collect();
            if host_port_v.len() != 2 {
                return Err(Error::Others(format!(
                    "Host {} is not right for vsock",
                    host
                )));
            }
            let port: u32 = host_port_v[1]
                .parse()
                .expect("the vsock port is not an number");
            fd = socket(
                AddressFamily::Vsock,
                SockType::Stream,
                SockFlag::SOCK_CLOEXEC,
                None,
            )
            .map_err(|e| Error::Socket(e.to_string()))?;
            let cid = addr.1;
            sockaddr = SockAddr::new_vsock(cid, port);
        }
    };

    Ok((fd, domain, sockaddr))
}

// Vsock is not supported on non Linux.
#[cfg(target_os = "linux")]
use libc::VMADDR_CID_ANY;
#[cfg(not(target_os = "linux"))]
const VMADDR_CID_ANY: u32 = 0;
#[cfg(target_os = "linux")]
use libc::VMADDR_CID_HOST;
#[cfg(not(target_os = "linux"))]
const VMADDR_CID_HOST: u32 = 0;

pub fn do_bind(host: &str) -> Result<(RawFd, Domain)> {
    let (fd, domain, sockaddr) = make_socket((host, VMADDR_CID_ANY))?;

    setsockopt(fd, sockopt::ReusePort, &true)?;
    bind(fd, &sockaddr).map_err(err_to_others_err!(e, ""))?;

    Ok((fd, domain))
}

/// Creates a unix socket for client.
pub(crate) unsafe fn client_connect(host: &str) -> Result<RawFd> {
    let (fd, _, sockaddr) = make_socket((host, VMADDR_CID_HOST))?;

    connect(fd, &sockaddr)?;

    Ok(fd)
}

macro_rules! cfg_sync {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "sync")]
            #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
            $item
        )*
    }
}

macro_rules! cfg_async {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "async")]
            #[cfg_attr(docsrs, doc(cfg(feature = "async")))]
            $item
        )*
    }
}

pub const MESSAGE_HEADER_LENGTH: usize = 10;
pub const MESSAGE_LENGTH_MAX: usize = 4 << 20;

pub const MESSAGE_TYPE_REQUEST: u8 = 0x1;
pub const MESSAGE_TYPE_RESPONSE: u8 = 0x2;
