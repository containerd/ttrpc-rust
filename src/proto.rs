// Copyright 2022 Alibaba Cloud. All rights reserved.
// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

//! Low-level wire framing and codec primitives.
//!
//! Generated clients and servers handle these types automatically. They are public primarily for
//! custom transports, generated bindings, and protocol tooling.

#[allow(soft_unstable, clippy::type_complexity, clippy::too_many_arguments)]
mod compiled {
    include!(concat!(env!("OUT_DIR"), "/mod.rs"));
}
// The schema keeps `package grpc`; the generated module differs by backend
// (protobuf-codegen names output after the input file, prost-build after
// the package), so re-export the backend-specific module uniformly.
#[cfg(feature = "prost")]
pub use compiled::grpc::*;
#[cfg(feature = "rustprotobuf")]
pub use compiled::ttrpc::*;

use byteorder::{BigEndian, ByteOrder};
#[cfg(feature = "rustprotobuf")]
use protobuf::{CodedInputStream, CodedOutputStream};

use crate::error::{get_rpc_status, Error, Result as TtResult};

/// Encoded length of a ttrpc message header, in bytes.
pub const MESSAGE_HEADER_LENGTH: usize = 10;
/// Maximum accepted payload length, in bytes.
pub const MESSAGE_LENGTH_MAX: usize = 4 << 20;
/// Buffer size used while discarding an oversized payload.
pub const DEFAULT_PAGE_SIZE: usize = 4 << 10;

/// Message type used for a request.
pub const MESSAGE_TYPE_REQUEST: u8 = 0x1;
/// Message type used for a response.
pub const MESSAGE_TYPE_RESPONSE: u8 = 0x2;
/// Message type used for a streaming data frame.
pub const MESSAGE_TYPE_DATA: u8 = 0x3;

/// Indicates that the sending endpoint has closed its stream half.
pub const FLAG_REMOTE_CLOSED: u8 = 0x1;
/// Indicates that the sending endpoint has opened its stream half.
pub const FLAG_REMOTE_OPEN: u8 = 0x2;
/// Indicates that a frame contains no payload.
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

/// The fixed-width header that precedes every ttrpc payload.
///
/// # Examples
///
/// ```
/// use ttrpc::proto::{MessageHeader, MESSAGE_TYPE_REQUEST};
///
/// let header = MessageHeader::new_request(1, 128);
/// assert_eq!(header.stream_id, 1);
/// assert_eq!(header.length, 128);
/// assert_eq!(header.type_, MESSAGE_TYPE_REQUEST);
/// ```
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageHeader {
    /// Length of the payload following this header, in bytes.
    pub length: u32,
    /// Identifier used to correlate frames belonging to the same RPC or stream.
    pub stream_id: u32,
    /// Frame kind; one of the `MESSAGE_TYPE_*` constants.
    pub type_: u8,
    /// Bitset composed from the `FLAG_*` constants.
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

/// A ttrpc frame with an untyped byte payload.
///
/// This type is constructed internally by the ttrpc runtime and is not normally built directly by
/// applications.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct GenMessage {
    /// Wire header describing the payload.
    pub header: MessageHeader,
    /// Unencoded frame payload.
    pub payload: Vec<u8>,
}

/// Errors that can occur while reading an untyped ttrpc frame.
#[derive(Debug, PartialEq)]
pub enum GenMessageError {
    /// An I/O, transport, or protocol error that should be handled locally.
    InternalError(Error),
    /// A protocol error that should be returned to the peer for the associated header.
    ReturnError(MessageHeader, Error),
}

impl From<Error> for GenMessageError {
    fn from(e: Error) -> Self {
        Self::InternalError(e)
    }
}

#[cfg(feature = "async")]
impl GenMessage {
    /// Create a DATA message.
    pub(crate) fn new_data(stream_id: u32, payload: Vec<u8>) -> Self {
        Self {
            header: MessageHeader::new_data(stream_id, payload.len() as u32),
            payload,
        }
    }

    /// Create a RESPONSE message.
    pub(crate) fn new_response(stream_id: u32, payload: Vec<u8>) -> Self {
        Self {
            header: MessageHeader::new_response(stream_id, payload.len() as u32),
            payload,
        }
    }

    /// Create a DATA close message (FLAG_REMOTE_CLOSED | FLAG_NO_DATA).
    pub(crate) fn new_close(stream_id: u32) -> Self {
        let mut header = MessageHeader::new_data(stream_id, 0);
        header.set_flags(FLAG_REMOTE_CLOSED | FLAG_NO_DATA);
        Self {
            header,
            payload: Vec::new(),
        }
    }

    /// Writes the frame header and payload to an asynchronous writer.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Socket`] if the header or payload cannot be written.
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

    /// Reads a frame header and payload from an asynchronous reader.
    ///
    /// # Errors
    ///
    /// Returns [`GenMessageError::InternalError`] for I/O failures. Oversized payloads are
    /// discarded and returned as [`GenMessageError::ReturnError`].
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

    /// Validates the payload length declared by the frame header.
    ///
    /// # Errors
    ///
    /// Returns an `INVALID_ARGUMENT` RPC status if the payload exceeds [`MESSAGE_LENGTH_MAX`].
    pub fn check(&self) -> TtResult<()> {
        check_oversize(self.header.length as usize, true)
    }
}

/// Encodes and decodes values carried by ttrpc frames.
///
/// The crate implements this trait for all [`protobuf::Message`] types.
pub trait Codec {
    /// Error returned while encoding or decoding the value.
    type E;

    /// Returns the encoded size of this value in bytes.
    fn size(&self) -> u32;
    /// Encodes this value into a newly allocated byte buffer.
    fn encode(&self) -> Result<Vec<u8>, Self::E>;
    /// Decodes a value from `buf`.
    fn decode(buf: impl AsRef<[u8]>) -> Result<Self, Self::E>
    where
        Self: Sized;
    /// Merges encoded bytes into an existing value.
    fn merge(&mut self, buf: impl AsRef<[u8]>) -> Result<(), Self::E>;
}

/// Backend-neutral [`Request`] builders used by the codegen macros.
///
/// This is an implementation detail of the generated-code pipeline and is
/// not part of the stable public API.
#[doc(hidden)]
pub trait RequestInit: Default {
    /// Creates a request with the routing and metadata fields set.
    fn init_request(
        service: String,
        method: String,
        timeout_nano: i64,
        metadata: Vec<KeyValue>,
    ) -> Self;
    /// Replaces the request payload.
    fn set_payload(&mut self, payload: Vec<u8>);
}

/// Backend-neutral [`Response`] builders used by the codegen macros.
///
/// This is an implementation detail of the generated-code pipeline and is
/// not part of the stable public API.
#[doc(hidden)]
pub trait ResponseInit: Default {
    /// Creates a response carrying the given status.
    fn init_status(status: Status) -> Self;
    /// Replaces the response payload.
    fn set_payload(&mut self, payload: Vec<u8>);
    /// Replaces the response status.
    fn set_status(&mut self, status: Status);
    /// Returns the response status when it is present and not `OK`.
    fn non_ok(&self) -> Option<Status>;
}

#[cfg(feature = "rustprotobuf")]
impl RequestInit for Request {
    fn init_request(
        service: String,
        method: String,
        timeout_nano: i64,
        metadata: Vec<KeyValue>,
    ) -> Self {
        let mut req = Request::new();
        req.set_service(service);
        req.set_method(method);
        req.set_timeout_nano(timeout_nano);
        req.set_metadata(metadata);
        req
    }

    fn set_payload(&mut self, payload: Vec<u8>) {
        self.payload = payload;
    }
}

#[cfg(feature = "rustprotobuf")]
impl ResponseInit for Response {
    fn init_status(status: Status) -> Self {
        let mut res = Response::new();
        res.set_status(status);
        res
    }

    fn set_payload(&mut self, payload: Vec<u8>) {
        self.payload = payload;
    }

    fn set_status(&mut self, status: Status) {
        self.status = ::protobuf::MessageField::some(status);
    }

    fn non_ok(&self) -> Option<Status> {
        let status = self.status();
        if status.code() != Code::OK {
            Some((*status).clone())
        } else {
            None
        }
    }
}

#[cfg(feature = "prost")]
impl RequestInit for Request {
    fn init_request(
        service: String,
        method: String,
        timeout_nano: i64,
        metadata: Vec<KeyValue>,
    ) -> Self {
        Request {
            service,
            method,
            timeout_nano,
            metadata,
            ..Default::default()
        }
    }

    fn set_payload(&mut self, payload: Vec<u8>) {
        self.payload = payload;
    }
}

#[cfg(feature = "prost")]
impl ResponseInit for Response {
    fn init_status(status: Status) -> Self {
        Response {
            status: Some(status),
            ..Default::default()
        }
    }

    fn set_payload(&mut self, payload: Vec<u8>) {
        self.payload = payload;
    }

    fn set_status(&mut self, status: Status) {
        self.status = Some(status);
    }

    fn non_ok(&self) -> Option<Status> {
        self.status
            .as_ref()
            .filter(|s| s.code != Code::OK as i32)
            .cloned()
    }
}

#[cfg(feature = "rustprotobuf")]
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

    fn merge(&mut self, buf: impl AsRef<[u8]>) -> Result<(), Self::E> {
        protobuf::Message::merge_from_bytes(self, buf.as_ref())
    }
}

#[cfg(feature = "prost")]
impl<M: prost::Message + Default> Codec for M {
    type E = std::io::Error;

    fn size(&self) -> u32 {
        self.encoded_len() as u32
    }

    fn encode(&self) -> Result<Vec<u8>, Self::E> {
        Ok(self.encode_to_vec())
    }

    fn decode(buf: impl AsRef<[u8]>) -> Result<Self, Self::E>
    where
        Self: Sized,
    {
        prost::Message::decode(buf.as_ref()).map_err(std::io::Error::from)
    }

    fn merge(&mut self, buf: impl AsRef<[u8]>) -> Result<(), Self::E> {
        prost::Message::merge(self, buf.as_ref()).map_err(std::io::Error::from)
    }
}

/// A ttrpc frame with a typed payload.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct Message<C> {
    /// Wire header describing the payload.
    pub header: MessageHeader,
    /// Decoded frame payload.
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
    /// Creates a request frame for `message` on `stream_id`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Others`] if the encoded payload exceeds [`MESSAGE_LENGTH_MAX`].
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
    /// Encodes and writes this typed frame to an asynchronous writer.
    ///
    /// # Errors
    ///
    /// Returns an error if the payload cannot be encoded or the frame cannot be written.
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

    /// Reads and decodes a typed frame from an asynchronous reader.
    ///
    /// # Errors
    ///
    /// Returns an error if the frame cannot be read or its payload cannot be decoded.
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

    #[cfg(feature = "rustprotobuf")]
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

    #[cfg(feature = "prost")]
    fn new_protobuf_request() -> Request {
        let meta = vec![KeyValue {
            key: "test_key1".to_string(),
            value: "test_value1".to_string(),
        }];
        Request {
            service: "grpc.TestServices".to_owned(),
            method: "Test".to_owned(),
            timeout_nano: 20 * 1000 * 1000,
            metadata: meta,
            payload: vec![0x1, 0x2, 0x3, 0x4, 0x5, 0x6, 0x7, 0x8, 0x9],
        }
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
    #[cfg(feature = "rustprotobuf")]
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
    #[cfg(feature = "rustprotobuf")]
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
