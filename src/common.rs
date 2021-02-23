// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

//! Common functions and macros.

#![allow(unused_macros)]

use crate::error::{Error, Result};
use crate::ttrpc::KeyValue;
use nix::fcntl::{fcntl, FcntlArg, FdFlag, OFlag};
use nix::sys::socket::*;
use std::collections::HashMap;
use std::os::unix::io::RawFd;
use std::str::FromStr;

#[derive(Debug)]
pub enum Domain {
    Unix,
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

pub fn do_bind(host: &str) -> Result<(RawFd, Domain)> {
    let (domain, hostv) = parse_host(host)?;

    let sockaddr: SockAddr;
    let fd: RawFd;

    match domain {
        Domain::Unix => {
            fd = socket(
                AddressFamily::Unix,
                SockType::Stream,
                SockFlag::SOCK_CLOEXEC,
                None,
            )
            .map_err(|e| Error::Socket(e.to_string()))?;
            let sockaddr_h = hostv[1].to_owned() + &"\x00".to_string();
            let sockaddr_u =
                UnixAddr::new_abstract(sockaddr_h.as_bytes()).map_err(err_to_others_err!(e, ""))?;
            sockaddr = SockAddr::Unix(sockaddr_u);
        }
        Domain::Vsock => {
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
    };

    setsockopt(fd, sockopt::ReusePort, &true).ok();
    bind(fd, &sockaddr).map_err(err_to_others_err!(e, ""))?;

    Ok((fd, domain))
}

macro_rules! cfg_sync {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "sync")]
            #[cfg_attr(docsrs, doc(cfg(feature = "sync")))]
            #[doc(inline)]
            $item
        )*
    }
}

macro_rules! cfg_async {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "async")]
            #[cfg_attr(docsrs, doc(cfg(feature = "async")))]
            #[doc(inline)]
            $item
        )*
    }
}

pub const MESSAGE_HEADER_LENGTH: usize = 10;
pub const MESSAGE_LENGTH_MAX: usize = 4 << 20;

pub const MESSAGE_TYPE_REQUEST: u8 = 0x1;
pub const MESSAGE_TYPE_RESPONSE: u8 = 0x2;

pub fn parse_metadata(kvs: &protobuf::RepeatedField<KeyValue>) -> HashMap<String, Vec<String>> {
    let mut meta: HashMap<String, Vec<String>> = HashMap::new();
    for kv in kvs {
        if let Some(ref mut vl) = meta.get_mut(&kv.key) {
            vl.push(kv.value.clone());
        } else {
            meta.insert(kv.key.clone(), vec![kv.value.clone()]);
        }
    }
    meta
}

pub fn convert_metadata(
    kvs: &Option<HashMap<String, Vec<String>>>,
) -> protobuf::RepeatedField<KeyValue> {
    let mut meta: protobuf::RepeatedField<KeyValue> = protobuf::RepeatedField::default();
    match kvs {
        Some(kvs) => {
            for (k, vl) in kvs {
                for v in vl {
                    let key = KeyValue {
                        key: k.clone(),
                        value: v.clone(),
                        ..Default::default()
                    };
                    meta.push(key);
                }
            }
        }
        None => {}
    }
    meta
}

#[cfg(test)]
mod tests {
    use super::{convert_metadata, parse_metadata};
    use crate::ttrpc::KeyValue;

    #[test]
    fn test_metadata() {
        // RepeatedField -> HashMap, test parse_metadata()
        let mut src: protobuf::RepeatedField<KeyValue> = protobuf::RepeatedField::default();
        for i in &[
            ("key1", "value1-1"),
            ("key1", "value1-2"),
            ("key2", "value2"),
        ] {
            let key = KeyValue {
                key: i.0.to_string(),
                value: i.1.to_string(),
                ..Default::default()
            };
            src.push(key);
        }

        let dst = parse_metadata(&src);
        assert_eq!(dst.len(), 2);

        assert_eq!(
            dst.get("key1"),
            Some(&vec!["value1-1".to_string(), "value1-2".to_string()])
        );
        assert_eq!(dst.get("key2"), Some(&vec!["value2".to_string()]));
        assert_eq!(dst.get("key3"), None);

        // HashMap -> RepeatedField , test convert_metadata()
        let src = convert_metadata(&Some(dst));
        let mut kvs = src.into_vec();
        kvs.sort_by(|a, b| a.key.partial_cmp(&b.key).unwrap());

        assert_eq!(kvs.len(), 3);

        assert_eq!(kvs[0].key, "key1");
        assert_eq!(kvs[0].value, "value1-1");

        assert_eq!(kvs[1].key, "key1");
        assert_eq!(kvs[1].value, "value1-2");

        assert_eq!(kvs[2].key, "key2");
        assert_eq!(kvs[2].value, "value2");
    }
}
