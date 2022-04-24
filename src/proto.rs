// Copyright 2022 Alibaba Cloud. All rights reserved.
// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

#[allow(soft_unstable, clippy::type_complexity, clippy::too_many_arguments)]
mod compiled {
    include!(concat!(env!("OUT_DIR"), "/mod.rs"));
}
pub use compiled::ttrpc::*;

use byteorder::{BigEndian, ByteOrder};

pub const MESSAGE_HEADER_LENGTH: usize = 10;
pub const MESSAGE_LENGTH_MAX: usize = 4 << 20;

pub const MESSAGE_TYPE_REQUEST: u8 = 0x1;
pub const MESSAGE_TYPE_RESPONSE: u8 = 0x2;

/// Message header of ttrpc.
#[derive(Default, Debug)]
pub struct MessageHeader {
    pub length: u32,
    pub stream_id: u32,
    pub type_: u8,
    pub flags: u8,
}

impl<T> From<T> for MessageHeader
where
    T: AsRef<[u8]>,
{
    fn from(buf: T) -> Self {
        let buf = buf.as_ref();
        debug_assert!(buf.len() >= MESSAGE_HEADER_LENGTH);
        Self {
            length: BigEndian::read_u32(&buf[..4]),
            stream_id: BigEndian::read_u32(&buf[4..8]),
            type_: buf[8],
            flags: buf[9],
        }
    }
}

impl From<MessageHeader> for Vec<u8> {
    fn from(mh: MessageHeader) -> Self {
        let mut buf = vec![0u8; MESSAGE_HEADER_LENGTH];
        mh.into_buf(&mut buf);
        buf
    }
}

impl MessageHeader {
    /// Creates a request MessageHeader from stream_id and len.
    /// Use the default message type MESSAGE_TYPE_REQUEST, and default flags 0.
    pub fn new_request(stream_id: u32, len: u32) -> Self {
        Self {
            length: len,
            stream_id,
            type_: MESSAGE_TYPE_REQUEST,
            flags: 0,
        }
    }

    /// Creates a response MessageHeader from stream_id and len.
    /// Use the default message type MESSAGE_TYPE_REQUEST, and default flags 0.
    pub fn new_response(stream_id: u32, len: u32) -> Self {
        Self {
            length: len,
            stream_id,
            type_: MESSAGE_TYPE_RESPONSE,
            flags: 0,
        }
    }

    /// Set the flags of message using the given flags.
    pub fn set_flags(&mut self, flags: u8) {
        self.flags = flags;
    }

    /// Add a new flags to the message.
    pub fn add_flags(&mut self, flags: u8) {
        self.flags |= flags;
    }

    pub(crate) fn into_buf(self, mut buf: impl AsMut<[u8]>) {
        let buf = buf.as_mut();
        debug_assert!(buf.len() >= MESSAGE_HEADER_LENGTH);

        let covbuf: &mut [u8] = &mut buf[..4];
        BigEndian::write_u32(covbuf, self.length);
        let covbuf: &mut [u8] = &mut buf[4..8];
        BigEndian::write_u32(covbuf, self.stream_id);
        buf[8] = self.type_;
        buf[9] = self.flags;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_header() {
        let buf = vec![
            0x10, 0x0, 0x0, 0x0, // length
            0x0, 0x0, 0x0, 0x03, // stream_id
            0x2,  // type_
            0xef, // flags
        ];
        let mh = MessageHeader::from(&buf);
        assert_eq!(mh.length, 0x1000_0000);
        assert_eq!(mh.stream_id, 0x3);
        assert_eq!(mh.type_, MESSAGE_TYPE_RESPONSE);
        assert_eq!(mh.flags, 0xef);

        let mut buf2 = vec![0; MESSAGE_HEADER_LENGTH];
        mh.into_buf(&mut buf2);
        assert_eq!(&buf, &buf2);
    }
}
