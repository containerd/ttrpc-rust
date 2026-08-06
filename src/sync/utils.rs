// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use crate::error::{get_status, Error, Result};
use crate::proto::{
    Codec, Code, MessageHeader, Request, Response, MESSAGE_TYPE_RESPONSE,
};
use crate::ConnectionContext;
use crate::security_extension::serialize_aad;
use std::collections::HashMap;
use std::sync::Arc;

/// Send a [`Response`] through the channel with full outbound pipeline.
///
/// Encodes the response, applies the connection transform (if any), and
/// sends the result through `tx`. Handles oversize responses safely:
///
/// 1. **Pre-check**: verifies the raw payload fits within the transform-safe
///    limit (`ConnectionContext::max_raw_payload_len`). If it doesn't, builds
///    a small error response instead — avoiding state advancement of stateful
///    transforms (e.g., AEAD nonce counters) on data that would be rejected.
/// 2. **Transform**: applies `transform_outbound` and the post-transform
///    size guard exactly once.
///
/// This single-pass design prevents the double-transform bug where a
/// stateful cipher would produce undecryptable output on a fallback message.
pub fn send_response(
    stream_id: u32,
    res: Response,
    tx: std::sync::mpsc::Sender<(MessageHeader, Vec<u8>)>,
    ctx: &ConnectionContext,
) -> Result<()> {
    let mut buf = res.encode().map_err(err_to_others_err!(e, ""))?;

    // Pre-check: ensure raw payload fits within the transform-safe limit.
    let max_len = ctx.max_raw_payload_len();
    if buf.len() > max_len {
        let err_msg = format!(
            "response payload {} bytes exceeds safe limit {} bytes (after transform overhead)",
            buf.len(),
            max_len
        );
        let err_resp: Response =
            Error::RpcStatus(get_status(Code::INVALID_ARGUMENT, err_msg)).into();
        buf = err_resp.encode().map_err(err_to_others_err!(e, ""))?;
    }

    // Build the header early so we can serialize AAD for the transform.
    // `length` is excluded from AAD, so the placeholder value doesn't matter.
    let aad = serialize_aad(&MessageHeader::new_response(stream_id, 0));
    let mh = MessageHeader {
        length: 0, // Will be set by send_response_sync after transform.
        stream_id,
        type_: MESSAGE_TYPE_RESPONSE,
        flags: 0,
    };

    // ── Serialize transform + enqueue ──
    ctx.send_response_sync(buf, &aad, &tx, mh)?;

    Ok(())
}

/// Response message through a channel. Eventually the message will be sent
/// to Client.
///
/// # Deprecated
///
/// This helper does **not** apply the connection's payload transform.
/// On an encrypted connection, responses sent through this function
/// **leak as plaintext** and an encrypted client will fail to decode them.
///
/// Use [`TtrpcContext::respond`] (from generated `request_handler!` macros or
/// hand-written handlers) or [`send_response`] with the connection's
/// [`ConnectionContext`] instead.
#[deprecated(
    since = "0.9.0",
    note = "Bypasses payload transform. Use TtrpcContext::respond() or send_response() instead."
)]
pub fn response_to_channel(
    stream_id: u32,
    res: Response,
    tx: std::sync::mpsc::Sender<(MessageHeader, Vec<u8>)>,
) -> Result<()> {
    let ctx = ConnectionContext::default();
    send_response(stream_id, res, tx, &ctx)
}

/// Send an error as a transform-aware response through the channel.
///
/// If the connection has a [`PayloadTransform`](crate::security_extension::PayloadTransform)
/// configured, the error response is encrypted before being sent so the
/// client can decode it on an encrypted connection.
pub fn response_error_to_channel(
    stream_id: u32,
    e: Error,
    tx: std::sync::mpsc::Sender<(MessageHeader, Vec<u8>)>,
    ctx: &ConnectionContext,
) -> Result<()> {
    send_response(stream_id, e.into(), tx, ctx)
}

/// Handle request in sync mode.
#[macro_export]
macro_rules! request_handler {
    ($class: ident, $ctx: ident, $req: ident, $server: ident, $req_type: ident, $req_fn: ident) => {
        let mut s = CodedInputStream::from_bytes(&$req.payload);
        let mut req = super::$server::$req_type::new();
        req.merge_from(&mut s)
            .map_err(::ttrpc::err_to_others!(e, ""))?;

        let mut res = ::ttrpc::Response::new();
        match $class.service.$req_fn(&$ctx, req) {
            Ok(rep) => {
                res.set_status(::ttrpc::get_status(::ttrpc::Code::OK, "".to_string()));
                res.payload.reserve(rep.compute_size() as usize);
                let mut s = protobuf::CodedOutputStream::vec(&mut res.payload);
                rep.write_to(&mut s)
                    .map_err(::ttrpc::err_to_others!(e, ""))?;
                s.flush().map_err(::ttrpc::err_to_others!(e, ""))?;
            }
            Err(x) => match x {
                ::ttrpc::Error::RpcStatus(s) => {
                    res.set_status(s);
                }
                _ => {
                    res.set_status(::ttrpc::get_status(
                        ::ttrpc::Code::UNKNOWN,
                        format!("{:?}", x),
                    ));
                }
            },
        }
        $ctx.respond($ctx.mh.stream_id, res)?
    };
}

/// Send request through sync client.
#[macro_export]
macro_rules! client_request {
    ($self: ident, $ctx: ident, $req: ident, $server: expr, $method: expr, $cres: ident) => {
        let mut creq = ::ttrpc::Request::new();
        creq.set_service($server.to_string());
        creq.set_method($method.to_string());
        creq.set_timeout_nano($ctx.timeout_nano);
        let md = ::ttrpc::context::to_pb($ctx.metadata);
        creq.set_metadata(md);
        creq.payload.reserve($req.compute_size() as usize);
        let mut s = CodedOutputStream::vec(&mut creq.payload);
        $req.write_to(&mut s)
            .map_err(::ttrpc::err_to_others!(e, ""))?;
        s.flush().map_err(::ttrpc::err_to_others!(e, ""))?;

        drop(s);

        let res = $self.client.request(creq)?;
        let mut s = CodedInputStream::from_bytes(&res.payload);
        $cres
            .merge_from(&mut s)
            .map_err(::ttrpc::err_to_others!(e, "Unpack get error "))?;
    };
}

/// Server-side context for a synchronous request.
///
/// Generated service traits receive this value by reference. Application handlers commonly use
/// [`TtrpcContext::metadata`] and [`TtrpcContext::timeout_nano`]; the remaining fields support the
/// generated dispatch layer.
#[derive(Debug)]
pub struct TtrpcContext {
    /// File descriptor for the client connection handling this request.
    #[cfg(unix)]
    pub fd: std::os::unix::io::RawFd,
    /// Native connection handle for the client handling this request.
    #[cfg(windows)]
    pub fd: i32,
    /// Receives a notification when the connection is cancelled.
    pub cancel_rx: crossbeam::channel::Receiver<()>,
    /// Wire header associated with the request.
    pub mh: MessageHeader,
    /// Channel used by generated handlers to send the response frame.
    pub res_tx: std::sync::mpsc::Sender<(MessageHeader, Vec<u8>)>,
    /// Request metadata grouped by key.
    pub metadata: HashMap<String, Vec<String>>,
    /// Client-provided timeout in nanoseconds, or zero if no timeout was set.
    pub timeout_nano: i64,
    /// Per-connection extension context (opaque data + optional payload transform).
    /// Immutable after accept. Default (empty data, no transform) when no hook is configured.
    pub conn_ctx: Arc<ConnectionContext>,
}

impl TtrpcContext {
    /// Encode, optionally transform, and send a response through this context's channel.
    ///
    /// This is the preferred way to send responses from within a [`MethodHandler`]
    /// implementation. The response is encoded, the connection's
    /// [`PayloadTransform`](crate::security_extension::PayloadTransform) is applied (if any),
    /// an oversize check is performed, and the result is sent to the client.
    pub fn respond(&self, stream_id: u32, res: Response) -> Result<()> {
        send_response(stream_id, res, self.res_tx.clone(), self.conn_ctx.as_ref())
    }
}

/// Dispatches a request to a synchronous service method.
///
/// This trait is implemented by generated service bindings.
pub trait MethodHandler {
    /// Handles one decoded ttrpc request.
    ///
    /// # Errors
    ///
    /// Returns an error if the request cannot be decoded, dispatched, or answered.
    fn handler(&self, ctx: TtrpcContext, req: Request) -> Result<()>;
}
