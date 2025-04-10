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

use crate::error::{sock_error_msg, Error, Result};
use crate::proto::{check_oversize, MessageHeader, DEFAULT_PAGE_SIZE, MESSAGE_HEADER_LENGTH};
use crate::sync::sys::PipeConnection;

fn read_count(conn: &PipeConnection, count: usize) -> Result<Vec<u8>> {
    let mut v: Vec<u8> = vec![0; count];
    let mut len = 0;

    if count == 0 {
        return Ok(v.to_vec());
    }

    loop {
        match conn.read(&mut v[len..]) {
            Ok(l) => {
                len += l;
                // when socket peer closed, it would return 0.
                if len == count || l == 0 {
                    break;
                }
            }
            Err(e) => {
                return Err(Error::Socket(e.to_string()));
            }
        }
    }

    Ok(v[0..len].to_vec())
}

fn write_count(conn: &PipeConnection, buf: &[u8], count: usize) -> Result<usize> {
    let mut len = 0;

    if count == 0 {
        return Ok(0);
    }

    loop {
        match conn.write(&buf[len..]) {
            Ok(l) => {
                len += l;
                if len == count {
                    break;
                }
            }
            Err(e) => {
                return Err(Error::Socket(e.to_string()));
            }
        }
    }

    Ok(len)
}

fn discard_count(conn: &PipeConnection, count: usize) -> Result<()> {
    let mut need_discard = count;

    while need_discard > 0 {
        let once_discard = std::cmp::min(DEFAULT_PAGE_SIZE, need_discard);
        read_count(conn, once_discard)?;
        need_discard -= once_discard;
    }

    Ok(())
}

fn read_message_header(conn: &PipeConnection) -> Result<MessageHeader> {
    let buf = read_count(conn, MESSAGE_HEADER_LENGTH)?;
    let size = buf.len();
    if size != MESSAGE_HEADER_LENGTH {
        return Err(sock_error_msg(
            size,
            format!("Message header length {size} is too small"),
        ));
    }

    let mh = MessageHeader::from(&buf);

    Ok(mh)
}

pub fn read_message(conn: &PipeConnection) -> Result<(MessageHeader, Result<Vec<u8>>)> {
    let mh = read_message_header(conn)?;
    trace!("Got Message header {:?}", mh);

    let mh_len = mh.length as usize;
    if let Err(e) = check_oversize(mh_len, true) {
        discard_count(conn, mh_len)?;
        return Ok((mh, Err(e)));
    }

    let buf = read_count(conn, mh.length as usize)?;
    let size = buf.len();
    if size != mh.length as usize {
        return Err(sock_error_msg(
            size,
            format!("Message length {} is not {}", size, mh.length),
        ));
    }
    trace!("Got Message body {:?}", buf);

    Ok((mh, Ok(buf)))
}

fn write_message_header(conn: &PipeConnection, mh: MessageHeader) -> Result<()> {
    let buf: Vec<u8> = mh.into();

    let size = write_count(conn, &buf, MESSAGE_HEADER_LENGTH)?;
    if size != MESSAGE_HEADER_LENGTH {
        return Err(sock_error_msg(
            size,
            format!("Send Message header length size {size} is not right"),
        ));
    }

    Ok(())
}

pub fn write_message(conn: &PipeConnection, mh: MessageHeader, buf: Vec<u8>) -> Result<()> {
    write_message_header(conn, mh)?;

    let size = write_count(conn, &buf, buf.len())?;
    if size != buf.len() {
        return Err(sock_error_msg(
            size,
            format!("Send Message length size {size} is not right"),
        ));
    }

    Ok(())
}
