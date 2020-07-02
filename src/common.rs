// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

#![allow(unused_macros)]

use crate::error::{Error, Result};
use nix::fcntl::{fcntl, FcntlArg, OFlag};
use nix::sys::socket::*;
use std::os::unix::io::RawFd;
use std::str::FromStr;

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

pub fn do_bind(host: &str) -> Result<RawFd> {
    let hostv: Vec<&str> = host.trim().split("://").collect();
    if hostv.len() != 2 {
        return Err(Error::Others(format!("Host {} is not right", host)));
    }
    let scheme = hostv[0].to_lowercase();

    let sockaddr: SockAddr;
    let fd: RawFd;

    match scheme.as_str() {
        "unix" => {
            fd = socket(
                AddressFamily::Unix,
                SockType::Stream,
                SockFlag::SOCK_CLOEXEC,
                None,
            )
            .map_err(|e| Error::Socket(e.to_string()))?;
            let sockaddr_h = hostv[1].to_owned() + &"\x00".to_string();
            let sockaddr_u =
                UnixAddr::new_abstract(sockaddr_h.as_bytes()).map_err(err_to_Others!(e, ""))?;
            sockaddr = SockAddr::Unix(sockaddr_u);
        }

        "vsock" => {
            let host_port_v: Vec<&str> = hostv[1].split(':').collect();
            if host_port_v.len() != 2 {
                return Err(Error::Others(format!(
                    "Host {} is not right for vsock",
                    host
                )));
            }
            let cid = libc::VMADDR_CID_ANY;
            let port: u32 =
                FromStr::from_str(host_port_v[1]).expect("the vsock port is not an number");
            fd = socket(
                AddressFamily::Vsock,
                SockType::Stream,
                SockFlag::SOCK_CLOEXEC,
                None,
            )
            .map_err(|e| Error::Socket(e.to_string()))?;
            sockaddr = SockAddr::new_vsock(cid, port);
        }
        _ => return Err(Error::Others(format!("Scheme {} is not supported", scheme))),
    };

    bind(fd, &sockaddr).map_err(err_to_Others!(e, ""))?;

    Ok(fd)
}

macro_rules! cfg_sync {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "sync")]
            $item
        )*
    }
}

macro_rules! cfg_async {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "async")]
            $item
        )*
    }
}

pub const MESSAGE_HEADER_LENGTH: usize = 10;
pub const MESSAGE_LENGTH_MAX: usize = 4 << 20;

pub const MESSAGE_TYPE_REQUEST: u8 = 0x1;
pub const MESSAGE_TYPE_RESPONSE: u8 = 0x2;
