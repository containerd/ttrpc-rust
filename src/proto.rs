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
use protobuf::{CodedInputStream, CodedOutputStream};

#[cfg(feature = "async")]
use crate::error::{get_rpc_status, Error, Result as TtResult};

pub const MESSAGE_HEADER_LENGTH: usize = 10;
pub const MESSAGE_LENGTH_MAX: usize = 4 << 20;

pub const MESSAGE_TYPE_REQUEST: u8 = 0x1;
pub const MESSAGE_TYPE_RESPONSE: u8 = 0x2;

/// Message header of ttrpc.
#[derive(Default, Debug, Clone, Copy, PartialEq)]
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
    /// Use the default message type MESSAGE_TYPE_RESPONSE, and default flags 0.
    pub fn new_response(stream_id: u32, len: u32) -> Self {
        Self {
            length: len,
            stream_id,
            type_: MESSAGE_TYPE_RESPONSE,
            flags: 0,
        }
    }

    /// Set the stream_id of message using the given value.
    pub fn set_stream_id(&mut self, stream_id: u32) {
        self.stream_id = stream_id;
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

#[cfg(feature = "async")]
impl MessageHeader {
    /// Encodes a MessageHeader to writer.
    pub async fn write_to(
        &self,
        mut writer: impl tokio::io::AsyncWriteExt + Unpin,
    ) -> std::io::Result<()> {
        writer.write_u32(self.length).await?;
        writer.write_u32(self.stream_id).await?;
        writer.write_u8(self.type_).await?;
        writer.write_u8(self.flags).await?;
        writer.flush().await
    }

    /// Decodes a MessageHeader from reader.
    pub async fn read_from(
        mut reader: impl tokio::io::AsyncReadExt + Unpin,
    ) -> std::io::Result<MessageHeader> {
        let mut content = vec![0; MESSAGE_HEADER_LENGTH];
        reader.read_exact(&mut content).await?;
        Ok(MessageHeader::from(&content))
    }
}

/// Generic message of ttrpc.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct GenMessage {
    pub header: MessageHeader,
    pub payload: Vec<u8>,
}

#[cfg(feature = "async")]
impl GenMessage {
    /// Encodes a MessageHeader to writer.
    pub async fn write_to(
        &self,
        mut writer: impl tokio::io::AsyncWriteExt + Unpin,
    ) -> TtResult<()> {
        self.header
            .write_to(&mut writer)
            .await
            .map_err(|e| Error::Socket(e.to_string()))?;
        writer
            .write_all(&self.payload)
            .await
            .map_err(|e| Error::Socket(e.to_string()))?;
        Ok(())
    }

    /// Decodes a MessageHeader from reader.
    pub async fn read_from(mut reader: impl tokio::io::AsyncReadExt + Unpin) -> TtResult<Self> {
        let header = MessageHeader::read_from(&mut reader)
            .await
            .map_err(|e| Error::Socket(e.to_string()))?;

        if header.length > MESSAGE_LENGTH_MAX as u32 {
            return Err(get_rpc_status(
                Code::INVALID_ARGUMENT,
                format!(
                    "message length {} exceed maximum message size of {}",
                    header.length, MESSAGE_LENGTH_MAX
                ),
            ));
        }

        let mut content = vec![0; header.length as usize];
        reader
            .read_exact(&mut content)
            .await
            .map_err(|e| Error::Socket(e.to_string()))?;

        Ok(Self {
            header,
            payload: content,
        })
    }
}

/// TTRPC codec, only protobuf is supported.
pub trait Codec {
    type E;

    fn size(&self) -> u32;
    fn encode(&self) -> Result<Vec<u8>, Self::E>;
    fn decode(buf: impl AsRef<[u8]>) -> Result<Self, Self::E>
    where
        Self: Sized;
}

impl<M: protobuf::Message> Codec for M {
    type E = protobuf::error::ProtobufError;

    fn size(&self) -> u32 {
        self.compute_size()
    }

    fn encode(&self) -> Result<Vec<u8>, Self::E> {
        let mut buf = vec![0; self.compute_size() as usize];
        let mut s = CodedOutputStream::bytes(&mut buf);
        self.write_to(&mut s)?;
        s.flush()?;
        Ok(buf)
    }

    fn decode(buf: impl AsRef<[u8]>) -> Result<Self, Self::E> {
        let mut s = CodedInputStream::from_bytes(buf.as_ref());
        M::parse_from(&mut s)
    }
}

/// Message of ttrpc.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct Message<C> {
    pub header: MessageHeader,
    pub payload: C,
}

impl<C> std::convert::TryFrom<GenMessage> for Message<C>
where
    C: Codec,
{
    type Error = C::E;
    fn try_from(gen: GenMessage) -> Result<Self, Self::Error> {
        Ok(Self {
            header: gen.header,
            payload: C::decode(&gen.payload)?,
        })
    }
}

impl<C> std::convert::TryFrom<Message<C>> for GenMessage
where
    C: Codec,
{
    type Error = C::E;
    fn try_from(msg: Message<C>) -> Result<Self, Self::Error> {
        Ok(Self {
            header: msg.header,
            payload: msg.payload.encode()?,
        })
    }
}

impl<C: Codec> Message<C> {
    pub fn new_request(stream_id: u32, message: C) -> Self {
        Self {
            header: MessageHeader::new_request(stream_id, message.size()),
            payload: message,
        }
    }
}

#[cfg(feature = "async")]
impl<C> Message<C>
where
    C: Codec,
    C::E: std::fmt::Display,
{
    /// Encodes a MessageHeader to writer.
    pub async fn write_to(
        &self,
        mut writer: impl tokio::io::AsyncWriteExt + Unpin,
    ) -> TtResult<()> {
        self.header
            .write_to(&mut writer)
            .await
            .map_err(|e| Error::Socket(e.to_string()))?;
        let content = self
            .payload
            .encode()
            .map_err(err_to_others_err!(e, "Encode payload failed."))?;
        writer
            .write_all(&content)
            .await
            .map_err(|e| Error::Socket(e.to_string()))?;
        Ok(())
    }

    /// Decodes a MessageHeader from reader.
    pub async fn read_from(mut reader: impl tokio::io::AsyncReadExt + Unpin) -> TtResult<Self> {
        let header = MessageHeader::read_from(&mut reader)
            .await
            .map_err(|e| Error::Socket(e.to_string()))?;

        if header.length > MESSAGE_LENGTH_MAX as u32 {
            return Err(get_rpc_status(
                Code::INVALID_ARGUMENT,
                format!(
                    "message length {} exceed maximum message size of {}",
                    header.length, MESSAGE_LENGTH_MAX
                ),
            ));
        }

        let mut content = vec![0; header.length as usize];
        reader
            .read_exact(&mut content)
            .await
            .map_err(|e| Error::Socket(e.to_string()))?;
        let payload =
            C::decode(content).map_err(err_to_others_err!(e, "Decode payload failed."))?;
        Ok(Self { header, payload })
    }
}
