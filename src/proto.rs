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

use crate::error::{get_rpc_status, Error, Result as TtResult};

pub const MESSAGE_HEADER_LENGTH: usize = 10;
pub const MESSAGE_LENGTH_MAX: usize = 4 << 20;
pub const DEFAULT_PAGE_SIZE: usize = 4 << 10;

pub const MESSAGE_TYPE_REQUEST: u8 = 0x1;
pub const MESSAGE_TYPE_RESPONSE: u8 = 0x2;
pub const MESSAGE_TYPE_DATA: u8 = 0x3;

pub const FLAG_REMOTE_CLOSED: u8 = 0x1;
pub const FLAG_REMOTE_OPEN: u8 = 0x2;
pub const FLAG_NO_DATA: u8 = 0x4;

pub(crate) fn check_oversize(len: usize, return_rpc_error: bool) -> TtResult<()> {
    if len > MESSAGE_LENGTH_MAX {
        let msg = format!(
            "message length {} exceed maximum message size of {}",
            len, MESSAGE_LENGTH_MAX
        );
        let e = if return_rpc_error {
            get_rpc_status(Code::INVALID_ARGUMENT, msg)
        } else {
            Error::Others(msg)
        };

        return Err(e);
    }

    Ok(())
}

// Discard the unwanted message body
#[cfg(feature = "async")]
async fn discard_message_body(
    mut reader: impl tokio::io::AsyncReadExt + Unpin,
    header: &MessageHeader,
) -> TtResult<()> {
    let mut need_discard = header.length as usize;

    while need_discard > 0 {
        let once_discard = std::cmp::min(DEFAULT_PAGE_SIZE, need_discard);
        let mut content = vec![0; once_discard];
        reader
            .read_exact(&mut content)
            .await
            .map_err(|e| Error::Socket(e.to_string()))?;
        need_discard -= once_discard;
    }

    Ok(())
}

/// Message header of ttrpc.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
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
    ///
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
    ///
    /// Use the MESSAGE_TYPE_RESPONSE message type, and default flags 0.
    pub fn new_response(stream_id: u32, len: u32) -> Self {
        Self {
            length: len,
            stream_id,
            type_: MESSAGE_TYPE_RESPONSE,
            flags: 0,
        }
    }

    /// Creates a data MessageHeader from stream_id and len.
    ///
    /// Use the MESSAGE_TYPE_DATA message type, and default flags 0.
    pub fn new_data(stream_id: u32, len: u32) -> Self {
        Self {
            length: len,
            stream_id,
            type_: MESSAGE_TYPE_DATA,
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
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct GenMessage {
    pub header: MessageHeader,
    pub payload: Vec<u8>,
}

#[derive(Debug, PartialEq)]
pub enum GenMessageError {
    InternalError(Error),
    ReturnError(MessageHeader, Error),
}

impl From<Error> for GenMessageError {
    fn from(e: Error) -> Self {
        Self::InternalError(e)
    }
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
    pub async fn read_from(
        mut reader: impl tokio::io::AsyncReadExt + Unpin,
    ) -> std::result::Result<Self, GenMessageError> {
        let header = MessageHeader::read_from(&mut reader)
            .await
            .map_err(|e| Error::Socket(e.to_string()))?;

        if let Err(e) = check_oversize(header.length as usize, true) {
            discard_message_body(reader, &header).await?;
            return Err(GenMessageError::ReturnError(header, e));
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

    pub fn check(&self) -> TtResult<()> {
        check_oversize(self.header.length as usize, true)
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
    type E = protobuf::Error;

    fn size(&self) -> u32 {
        self.compute_size() as u32
    }

    fn encode(&self) -> Result<Vec<u8>, Self::E> {
        let mut buf = vec![0; self.compute_size() as usize];
        let mut s = CodedOutputStream::bytes(&mut buf);
        self.write_to(&mut s)?;
        s.flush()?;
        drop(s);
        Ok(buf)
    }

    fn decode(buf: impl AsRef<[u8]>) -> Result<Self, Self::E> {
        let mut s = CodedInputStream::from_bytes(buf.as_ref());
        M::parse_from(&mut s)
    }
}

/// Message of ttrpc.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
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
    pub fn new_request(stream_id: u32, message: C) -> TtResult<Self> {
        check_oversize(message.size() as usize, false)?;

        Ok(Self {
            header: MessageHeader::new_request(stream_id, message.size()),
            payload: message,
        })
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

        if check_oversize(header.length as usize, true).is_err() {
            discard_message_body(reader, &header).await?;
            return Ok(Self {
                header,
                payload: C::decode("").map_err(err_to_others_err!(e, "Decode payload failed."))?,
            });
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

#[cfg(test)]
mod tests {
    use std::convert::{TryFrom, TryInto};

    use super::*;

    static MESSAGE_HEADER: [u8; MESSAGE_HEADER_LENGTH] = [
        0x10, 0x0, 0x0, 0x0, // length
        0x0, 0x0, 0x0, 0x03, // stream_id
        0x2,  // type_
        0xef, // flags
    ];

    #[test]
    fn message_header() {
        let mh = MessageHeader::from(&MESSAGE_HEADER);
        assert_eq!(mh.length, 0x1000_0000);
        assert_eq!(mh.stream_id, 0x3);
        assert_eq!(mh.type_, MESSAGE_TYPE_RESPONSE);
        assert_eq!(mh.flags, 0xef);

        let mut buf2 = vec![0; MESSAGE_HEADER_LENGTH];
        mh.into_buf(&mut buf2);
        assert_eq!(&MESSAGE_HEADER, &buf2[..]);

        let mh = MessageHeader::from(&PROTOBUF_MESSAGE_HEADER);
        assert_eq!(mh.length as usize, TEST_PAYLOAD_LEN);
    }

    #[rustfmt::skip]
    static PROTOBUF_MESSAGE_HEADER: [u8; MESSAGE_HEADER_LENGTH] = [
        0x00, 0x0, 0x0, TEST_PAYLOAD_LEN as u8, // length
        0x0, 0x12, 0x34, 0x56, // stream_id
        0x1,  // type_
        0xef, // flags
    ];

    const TEST_PAYLOAD_LEN: usize = 67;
    static PROTOBUF_REQUEST: [u8; TEST_PAYLOAD_LEN] = [
        10, 17, 103, 114, 112, 99, 46, 84, 101, 115, 116, 83, 101, 114, 118, 105, 99, 101, 115, 18,
        4, 84, 101, 115, 116, 26, 9, 1, 2, 3, 4, 5, 6, 7, 8, 9, 32, 128, 218, 196, 9, 42, 24, 10,
        9, 116, 101, 115, 116, 95, 107, 101, 121, 49, 18, 11, 116, 101, 115, 116, 95, 118, 97, 108,
        117, 101, 49,
    ];

    fn new_protobuf_request() -> Request {
        let mut creq = Request::new();
        creq.set_service("grpc.TestServices".to_string());
        creq.set_method("Test".to_string());
        creq.set_timeout_nano(20 * 1000 * 1000);
        let meta = vec![KeyValue {
            key: "test_key1".to_string(),
            value: "test_value1".to_string(),
            ..Default::default()
        }];
        creq.set_metadata(meta);
        creq.payload = vec![0x1, 0x2, 0x3, 0x4, 0x5, 0x6, 0x7, 0x8, 0x9];
        creq
    }

    #[test]
    fn protobuf_codec() {
        let creq = new_protobuf_request();
        let buf = creq.encode().unwrap();
        assert_eq!(&buf, &PROTOBUF_REQUEST);
        let dreq = Request::decode(&buf).unwrap();
        assert_eq!(creq, dreq);
        let dreq2 = Request::decode(PROTOBUF_REQUEST).unwrap();
        assert_eq!(creq, dreq2);
    }

    #[test]
    fn gen_message_to_message() {
        let req = new_protobuf_request();
        let msg = Message::new_request(3, req).unwrap();
        let msg_clone = msg.clone();
        let gen: GenMessage = msg.try_into().unwrap();
        let dmsg = Message::<Request>::try_from(gen).unwrap();
        assert_eq!(msg_clone, dmsg);
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn async_message_header() {
        use std::io::Cursor;
        let mut buf = vec![];
        let mut io = Cursor::new(&mut buf);
        let mh = MessageHeader::from(&MESSAGE_HEADER);
        mh.write_to(&mut io).await.unwrap();
        assert_eq!(buf, &MESSAGE_HEADER);

        let dmh = MessageHeader::read_from(&buf[..]).await.unwrap();
        assert_eq!(mh, dmh);
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn async_gen_message() {
        // Test packet which exceeds maximum message size
        let mut buf = Vec::from(MESSAGE_HEADER);
        let header = MessageHeader::read_from(&*buf).await.expect("read header");
        buf.append(&mut vec![0x0; header.length as usize]);

        match GenMessage::read_from(&*buf).await {
            Err(GenMessageError::ReturnError(h, Error::RpcStatus(s))) => {
                if h != header || s.code() != crate::proto::Code::INVALID_ARGUMENT {
                    panic!("got invalid error when the size exceeds limit");
                }
            }
            _ => {
                panic!("got invalid error when the size exceeds limit");
            }
        }

        let mut buf = Vec::from(PROTOBUF_MESSAGE_HEADER);
        buf.extend_from_slice(&PROTOBUF_REQUEST);
        buf.extend_from_slice(&[0x0, 0x0]);
        let gen = GenMessage::read_from(&*buf).await.unwrap();
        assert_eq!(gen.header.length as usize, TEST_PAYLOAD_LEN);
        assert_eq!(gen.header.length, gen.payload.len() as u32);
        assert_eq!(gen.header.stream_id, 0x123456);
        assert_eq!(gen.header.type_, MESSAGE_TYPE_REQUEST);
        assert_eq!(gen.header.flags, 0xef);
        assert_eq!(&gen.payload, &PROTOBUF_REQUEST);
        assert_eq!(
            &buf[MESSAGE_HEADER_LENGTH + TEST_PAYLOAD_LEN..],
            &[0x0, 0x0]
        );

        let mut dbuf = vec![];
        let mut io = std::io::Cursor::new(&mut dbuf);
        gen.write_to(&mut io).await.unwrap();
        assert_eq!(&*dbuf, &buf[..MESSAGE_HEADER_LENGTH + TEST_PAYLOAD_LEN]);
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn async_message() {
        // Test packet which exceeds maximum message size
        let mut buf = Vec::from(MESSAGE_HEADER);
        let header = MessageHeader::read_from(&*buf).await.expect("read header");
        buf.append(&mut vec![0x0; header.length as usize]);

        let gen = Message::<Request>::read_from(&*buf)
            .await
            .expect("read message");

        assert_eq!(gen.header, header);
        assert_eq!(protobuf::Message::compute_size(&gen.payload), 0);

        let mut buf = Vec::from(PROTOBUF_MESSAGE_HEADER);
        buf.extend_from_slice(&PROTOBUF_REQUEST);
        buf.extend_from_slice(&[0x0, 0x0]);
        let msg = Message::<Request>::read_from(&*buf).await.unwrap();
        assert_eq!(msg.header.length, 67);
        assert_eq!(msg.header.length, msg.payload.size());
        assert_eq!(msg.header.stream_id, 0x123456);
        assert_eq!(msg.header.type_, MESSAGE_TYPE_REQUEST);
        assert_eq!(msg.header.flags, 0xef);
        assert_eq!(&msg.payload.service, "grpc.TestServices");
        assert_eq!(&msg.payload.method, "Test");
        assert_eq!(
            msg.payload.payload,
            vec![0x1, 0x2, 0x3, 0x4, 0x5, 0x6, 0x7, 0x8, 0x9]
        );
        assert_eq!(msg.payload.timeout_nano, 20 * 1000 * 1000);
        assert_eq!(msg.payload.metadata.len(), 1);
        assert_eq!(&msg.payload.metadata[0].key, "test_key1");
        assert_eq!(&msg.payload.metadata[0].value, "test_value1");

        let req = new_protobuf_request();
        let mut dmsg = Message::new_request(u32::MAX, req).unwrap();
        dmsg.header.set_stream_id(0x123456);
        dmsg.header.set_flags(0xe0);
        dmsg.header.add_flags(0x0f);
        let mut dbuf = vec![];
        let mut io = std::io::Cursor::new(&mut dbuf);
        dmsg.write_to(&mut io).await.unwrap();
        assert_eq!(&dbuf, &buf[..MESSAGE_HEADER_LENGTH + TEST_PAYLOAD_LEN]);
    }
}
