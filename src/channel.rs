// Copyright (c) 2019 Ant Financial
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use byteorder::{BigEndian, ByteOrder, ReadBytesExt};
use nix::sys::socket::*;
use std::os::unix::io::RawFd;

use crate::error::{get_RpcStatus, Error, Result};
use crate::ttrpc::{Code, Status};

const MESSAGE_HEADER_LENGTH: usize = 10;
const MESSAGE_LENGTH_MAX: usize = 4 << 20;

pub const MESSAGE_TYPE_REQUEST: u8 = 0x1;
pub const MESSAGE_TYPE_RESPONSE: u8 = 0x2;

#[derive(Default, Debug)]
pub struct message_header {
    pub Length: u32,
    pub StreamID: u32,
    pub Type: u8,
    pub Flags: u8,
}

const SOCK_DICONNECTED: &str = "socket disconnected";

fn sock_error_msg(size: usize, msg: String) -> Error {
    if size == 0 {
        return Error::Socket(SOCK_DICONNECTED.to_string());
    }

    get_RpcStatus(Code::INVALID_ARGUMENT, msg)
}

fn read_message_header(fd: RawFd) -> Result<message_header> {
    let mut buf = [0u8; MESSAGE_HEADER_LENGTH];
    let size = recv(fd, &mut buf, MsgFlags::empty()).map_err(|e| Error::Socket(e.to_string()))?;
    if size != MESSAGE_HEADER_LENGTH {
        return Err(sock_error_msg(
            size,
            format!("Message header length {} is too small", size),
        ));
    }

    let mut mh = message_header::default();
    let mut covbuf: &[u8] = &buf[..4];
    mh.Length =
        covbuf
            .read_u32::<BigEndian>()
            .map_err(err_to_RpcStatus!(Code::INVALID_ARGUMENT, e, ""))?;
    let mut covbuf: &[u8] = &buf[4..8];
    mh.StreamID =
        covbuf
            .read_u32::<BigEndian>()
            .map_err(err_to_RpcStatus!(Code::INVALID_ARGUMENT, e, ""))?;
    mh.Type = buf[8];
    mh.Flags = buf[9];

    Ok(mh)
}

pub fn read_message(fd: RawFd) -> Result<(message_header, Vec<u8>)> {
    let mh = read_message_header(fd)?;
    trace!("Got Message header {:?}", mh);

    if mh.Length > MESSAGE_LENGTH_MAX as u32 {
        return Err(get_RpcStatus(
            Code::INVALID_ARGUMENT,
            format!(
                "message length {} exceed maximum message size of {}",
                mh.Length, MESSAGE_LENGTH_MAX
            ),
        ));
    }

    let mut buf: Vec<u8> = Vec::new();
    buf.resize(mh.Length as usize, 0);
    let size = recv(fd, buf.as_mut_slice(), MsgFlags::empty())
        .map_err(|e| Error::Socket(e.to_string()))?;
    if size != mh.Length as usize {
        return Err(sock_error_msg(
            size,
            format!("Message length {} is not {}", size, mh.Length),
        ));
    }
    trace!("Got Message body {:?}", buf);

    Ok((mh, buf))
}

fn write_message_header(fd: RawFd, mh: message_header) -> Result<()> {
    let mut buf = [0u8; MESSAGE_HEADER_LENGTH];

    let mut covbuf: &mut [u8] = &mut buf[..4];
    BigEndian::write_u32(&mut covbuf, mh.Length);
    let mut covbuf: &mut [u8] = &mut buf[4..8];
    BigEndian::write_u32(&mut covbuf, mh.StreamID);
    buf[8] = mh.Type;
    buf[9] = mh.Flags;

    let size = send(fd, &buf, MsgFlags::empty()).map_err(|e| Error::Socket(e.to_string()))?;
    if size != MESSAGE_HEADER_LENGTH {
        return Err(sock_error_msg(
            size,
            format!("Send Message header length size {} is not right", size),
        ));
    }

    Ok(())
}

pub fn write_message(fd: RawFd, mh: message_header, buf: Vec<u8>) -> Result<()> {
    write_message_header(fd, mh)?;

    let size = send(fd, &buf, MsgFlags::empty()).map_err(|e| Error::Socket(e.to_string()))?;
    if size != buf.len() {
        return Err(sock_error_msg(
            size,
            format!("Send Message length size {} is not right", size),
        ));
    }

    Ok(())
}
