// Copyright 2022 Alibaba Cloud. All rights reserved.
// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::ConnectionContext;

use tokio::sync::mpsc;

use super::Client;
use crate::error::{Error, Result};
use crate::proto::{
    check_oversize, Code, Codec, GenMessage, Response, FLAG_NO_DATA,
    FLAG_REMOTE_CLOSED, MESSAGE_TYPE_DATA, MESSAGE_TYPE_RESPONSE,
};

pub type MessageSender = mpsc::Sender<SendingMessage>;
pub type MessageReceiver = mpsc::Receiver<SendingMessage>;

/// Internal message type for stream channels.
///
pub type ResultSender = mpsc::Sender<Result<GenMessage>>;
pub type ResultReceiver = mpsc::Receiver<Result<GenMessage>>;

#[derive(Debug)]
pub struct SendingMessage {
    pub msg: GenMessage,
    pub result_chan: Option<tokio::sync::oneshot::Sender<Result<()>>>,
}

impl SendingMessage {
    pub fn new(msg: GenMessage) -> Self {
        Self {
            msg,
            result_chan: None,
        }
    }
    pub fn new_with_result(
        msg: GenMessage,
        result_chan: tokio::sync::oneshot::Sender<Result<()>>,
    ) -> Self {
        Self {
            msg,
            result_chan: Some(result_chan),
        }
    }

    pub fn send_result(&mut self, result: Result<()>) {
        if let Some(result_ch) = self.result_chan.take() {
            result_ch.send(result).unwrap_or_default();
        }
    }
}

/// Client-side handle for a bidirectional stream.
///
/// Requests of type `Q` and responses of type `P` can move independently. Use [`ClientStream::split`]
/// when separate tasks should send and receive concurrently.
#[derive(Debug)]
pub struct ClientStream<Q, P> {
    tx: CSSender<Q>,
    rx: CSReceiver<P>,
}

impl<Q, P> ClientStream<Q, P>
where
    Q: Codec,
    P: Codec,
    <Q as Codec>::E: std::fmt::Display,
    <P as Codec>::E: std::fmt::Display,
{
    /// Wraps a low-level stream for use by generated client bindings.
    pub fn new(inner: StreamInner) -> Self {
        let (tx, rx) = inner.split();
        Self {
            tx: CSSender {
                tx,
                _send: PhantomData,
            },
            rx: CSReceiver {
                rx,
                _recv: PhantomData,
            },
        }
    }

    /// Splits the stream into independently owned sending and receiving halves.
    pub fn split(self) -> (CSSender<Q>, CSReceiver<P>) {
        (self.tx, self.rx)
    }

    /// Sends one request message.
    ///
    /// See [`CSSender::send`] for failure cases.
    pub async fn send(&self, req: &Q) -> Result<()> {
        self.tx.send(req).await
    }

    /// Closes the request-sending half of this stream.
    ///
    /// The response half remains available through [`ClientStream::recv`].
    /// See [`CSSender::close_send`] for failure cases.
    pub async fn close_send(&self) -> Result<()> {
        self.tx.close_send().await
    }

    /// Receives and decodes the next response message.
    ///
    /// See [`CSReceiver::recv`] for failure cases.
    pub async fn recv(&mut self) -> Result<P> {
        self.rx.recv().await
    }
}

/// Cloneable request-sending half of a client bidirectional stream.
#[derive(Clone, Debug)]
pub struct CSSender<Q> {
    tx: StreamSender,
    _send: PhantomData<Q>,
}

impl<Q> CSSender<Q>
where
    Q: Codec,
    <Q as Codec>::E: std::fmt::Display,
{
    /// Sends one request message.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding or transport fails, the message is oversized, or the sending
    /// half is closed.
    pub async fn send(&self, req: &Q) -> Result<()> {
        let msg_buf = req
            .encode()
            .map_err(err_to_others_err!(e, "Encode message failed."))?;
        self.tx.send(msg_buf).await
    }

    /// Closes the request-sending half.
    ///
    /// # Errors
    ///
    /// Returns [`Error::LocalClosed`] if already closed, or a transport error if the close frame
    /// cannot be sent.
    pub async fn close_send(&self) -> Result<()> {
        self.tx.close_send().await
    }
}

/// Response-receiving half of a client bidirectional stream.
#[derive(Debug)]
pub struct CSReceiver<P> {
    rx: StreamReceiver,
    _recv: PhantomData<P>,
}

impl<P> CSReceiver<P>
where
    P: Codec,
    <P as Codec>::E: std::fmt::Display,
{
    /// Receives and decodes the next response message.
    ///
    /// # Errors
    ///
    /// Returns an error if the peer closes the stream, reports a non-OK status, or sends a payload
    /// that cannot be decoded as `P`.
    pub async fn recv(&mut self) -> Result<P> {
        let msg_buf = self.rx.recv().await?;
        P::decode(msg_buf).map_err(err_to_others_err!(e, "Decode message failed."))
    }
}

/// Server-side handle for a bidirectional stream.
///
/// Responses of type `P` and requests of type `Q` can move independently. Use
/// [`ServerStream::split`] when separate tasks should send and receive concurrently.
#[derive(Debug)]
pub struct ServerStream<P, Q> {
    tx: SSSender<P>,
    rx: SSReceiver<Q>,
}

impl<P, Q> ServerStream<P, Q>
where
    P: Codec,
    Q: Codec,
    <P as Codec>::E: std::fmt::Display,
    <Q as Codec>::E: std::fmt::Display,
{
    /// Wraps a low-level stream for use by generated server bindings.
    pub fn new(inner: StreamInner) -> Self {
        let (tx, rx) = inner.split();
        Self {
            tx: SSSender {
                tx,
                _send: PhantomData,
            },
            rx: SSReceiver {
                rx,
                _recv: PhantomData,
            },
        }
    }

    /// Splits the stream into independently owned sending and receiving halves.
    pub fn split(self) -> (SSSender<P>, SSReceiver<Q>) {
        (self.tx, self.rx)
    }

    /// Sends one response message.
    ///
    /// See [`SSSender::send`] for failure cases.
    pub async fn send(&self, resp: &P) -> Result<()> {
        self.tx.send(resp).await
    }

    /// Receives and decodes the next request message.
    ///
    /// Returns `Ok(None)` after a graceful client close.
    /// See [`SSReceiver::recv`] for failure cases.
    pub async fn recv(&mut self) -> Result<Option<Q>> {
        self.rx.recv().await
    }
}

/// Cloneable response-sending half of a server bidirectional stream.
#[derive(Clone, Debug)]
pub struct SSSender<P> {
    tx: StreamSender,
    _send: PhantomData<P>,
}

impl<P> SSSender<P>
where
    P: Codec,
    <P as Codec>::E: std::fmt::Display,
{
    /// Sends one response message.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding or transport fails or the message is oversized.
    pub async fn send(&self, resp: &P) -> Result<()> {
        let msg_buf = resp
            .encode()
            .map_err(err_to_others_err!(e, "Encode message failed."))?;
        self.tx.send(msg_buf).await
    }
}

/// Request-receiving half of a server bidirectional stream.
#[derive(Debug)]
pub struct SSReceiver<Q> {
    rx: StreamReceiver,
    _recv: PhantomData<Q>,
}

impl<Q> SSReceiver<Q>
where
    Q: Codec,
    <Q as Codec>::E: std::fmt::Display,
{
    /// Receives and decodes the next request message.
    ///
    /// Returns `Ok(None)` after a graceful client close.
    ///
    /// # Errors
    ///
    /// Returns an error if the stream fails or a payload cannot be decoded as `Q`.
    pub async fn recv(&mut self) -> Result<Option<Q>> {
        let res = self.rx.recv().await;

        if matches!(res, Err(Error::Eof)) {
            return Ok(None);
        }
        let msg_buf = res?;
        Q::decode(msg_buf)
            .map_err(err_to_others_err!(e, "Decode message failed."))
            .map(Some)
    }
}

/// Client-side handle for a client-streaming RPC.
///
/// Send zero or more requests of type `Q`, then call [`ClientStreamSender::close_and_recv`] to
/// close the sending half and receive the single response of type `P`.
pub struct ClientStreamSender<Q, P> {
    inner: StreamInner,
    _send: PhantomData<Q>,
    _recv: PhantomData<P>,
}

impl<Q, P> ClientStreamSender<Q, P>
where
    Q: Codec,
    P: Codec,
    <Q as Codec>::E: std::fmt::Display,
    <P as Codec>::E: std::fmt::Display,
{
    /// Wraps a low-level stream for use by generated client bindings.
    pub fn new(inner: StreamInner) -> Self {
        Self {
            inner,
            _send: PhantomData,
            _recv: PhantomData,
        }
    }

    /// Sends one request message.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding or transport fails, the message is oversized, or the sending
    /// half is closed.
    pub async fn send(&self, req: &Q) -> Result<()> {
        let msg_buf = req
            .encode()
            .map_err(err_to_others_err!(e, "Encode message failed."))?;
        self.inner.send(msg_buf).await
    }

    /// Closes the request stream and waits for the server's single response.
    ///
    /// # Errors
    ///
    /// Returns an error if closing or receiving fails, the server reports a non-OK status, or the
    /// response cannot be decoded as `P`.
    pub async fn close_and_recv(&mut self) -> Result<P> {
        self.inner.close_send().await?;
        let msg_buf = self.inner.recv().await?;
        P::decode(msg_buf).map_err(err_to_others_err!(e, "Decode message failed."))
    }
}

/// Server-side response sender for a server-streaming RPC.
///
/// The generated handler receives this value after the single request has been decoded.
pub struct ServerStreamSender<P> {
    inner: StreamSender,
    _send: PhantomData<P>,
}

impl<P> ServerStreamSender<P>
where
    P: Codec,
    <P as Codec>::E: std::fmt::Display,
{
    /// Wraps a low-level stream for use by generated server bindings.
    pub fn new(inner: StreamInner) -> Self {
        Self {
            inner: inner.split().0,
            _send: PhantomData,
        }
    }

    /// Sends one response message.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding or transport fails or the message is oversized.
    pub async fn send(&self, resp: &P) -> Result<()> {
        let msg_buf = resp
            .encode()
            .map_err(err_to_others_err!(e, "Encode message failed."))?;
        self.inner.send(msg_buf).await
    }
}

/// Client-side response receiver for a server-streaming RPC.
///
/// This type retains a client clone so the connection remains alive while responses are pending.
pub struct ClientStreamReceiver<P> {
    inner: StreamReceiver,
    _recv: PhantomData<P>,
    // Hold the req_tx in Client to keep receiver task running
    _client_guard: Client,
}

impl<P> ClientStreamReceiver<P>
where
    P: Codec,
    <P as Codec>::E: std::fmt::Display,
{
    /// Wraps a low-level stream and retains `client_guard` for its lifetime.
    pub fn new(inner: StreamInner, _client_guard: Client) -> Self {
        Self {
            inner: inner.split().1,
            _recv: PhantomData,
            _client_guard,
        }
    }

    /// Receives and decodes the next response message.
    ///
    /// Returns `Ok(None)` after a graceful server close.
    ///
    /// # Errors
    ///
    /// Returns an error if the stream fails, the server reports a non-OK status, or a payload
    /// cannot be decoded as `P`.
    pub async fn recv(&mut self) -> Result<Option<P>> {
        let res = self.inner.recv().await;
        if matches!(res, Err(Error::Eof)) {
            return Ok(None);
        }
        let msg_buf = res?;
        P::decode(msg_buf)
            .map_err(err_to_others_err!(e, "Decode message failed."))
            .map(Some)
    }
}

/// Server-side request receiver for a client-streaming RPC.
pub struct ServerStreamReceiver<Q> {
    inner: StreamReceiver,
    _recv: PhantomData<Q>,
}

impl<Q> ServerStreamReceiver<Q>
where
    Q: Codec,
    <Q as Codec>::E: std::fmt::Display,
{
    /// Wraps a low-level stream for use by generated server bindings.
    pub fn new(inner: StreamInner) -> Self {
        Self {
            inner: inner.split().1,
            _recv: PhantomData,
        }
    }

    /// Receives and decodes the next request message.
    ///
    /// Returns `Ok(None)` after a graceful client close.
    ///
    /// # Errors
    ///
    /// Returns an error if the stream fails or a payload cannot be decoded as `Q`.
    pub async fn recv(&mut self) -> Result<Option<Q>> {
        let res = self.inner.recv().await;
        if matches!(res, Err(Error::Eof)) {
            return Ok(None);
        }
        let msg_buf = res?;
        Q::decode(msg_buf)
            .map_err(err_to_others_err!(e, "Decode message failed."))
            .map(Some)
    }
}

async fn _recv(rx: &mut ResultReceiver) -> Result<GenMessage> {
    rx.recv().await.unwrap_or_else(|| {
        Err(Error::Others(
            "Receive packet from Receiver error".to_string(),
        ))
    })
}

/// Endpoint role for a low-level stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Client-side stream behavior.
    Client,
    /// Server-side stream behavior.
    Server,
}

/// Untyped state shared by generated streaming bindings.
///
/// Most applications should use the typed stream wrappers rather than constructing this type
/// directly.
#[derive(Debug)]
pub struct StreamInner {
    sender: StreamSender,
    receiver: StreamReceiver,
}

impl StreamInner {
    /// Creates low-level state for a registered stream.
    ///
    /// `sendable` and `recveivable` describe the permitted directions, while `kind` controls the
    /// client- or server-side close semantics. This constructor is used by the ttrpc runtime.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        stream_id: u32,
        tx: MessageSender,
        rx: ResultReceiver,
        //waiter: shutdown::Waiter,
        sendable: bool,
        recveivable: bool,
        kind: Kind,
        streams: Arc<Mutex<HashMap<u32, ResultSender>>>,
        conn_ctx: Arc<ConnectionContext>,
    ) -> Self {
        Self {
            sender: StreamSender {
                tx,
                stream_id,
                sendable,
                local_closed: Arc::new(AtomicBool::new(false)),
                kind,
                conn_ctx,
            },
            receiver: StreamReceiver {
                rx,
                stream_id,
                recveivable,
                remote_closed: false,
                kind,
                streams,
            },
        }
    }

    fn split(self) -> (StreamSender, StreamReceiver) {
        (self.sender, self.receiver)
    }

    /// Sends one untyped payload.
    ///
    /// See [`StreamSender::send`] for failure cases.
    pub async fn send(&self, buf: Vec<u8>) -> Result<()> {
        self.sender.send(buf).await
    }

    /// Closes the sending half of this client-side stream.
    ///
    /// See [`StreamSender::close_send`] for failure cases.
    pub async fn close_send(&self) -> Result<()> {
        self.sender.close_send().await
    }

    /// Receives the next untyped payload.
    ///
    /// See [`StreamReceiver::recv`] for failure cases.
    pub async fn recv(&mut self) -> Result<Vec<u8>> {
        self.receiver.recv().await
    }
}

/// Untyped, cloneable sending half of a stream.
///
/// This is a low-level building block for generated bindings. When the extension framework is
/// enabled, each [`StreamSender::send`] call applies the connection's outbound payload transform
/// before framing the payload as a DATA message.
#[derive(Clone, Debug)]
pub struct StreamSender {
    tx: MessageSender,
    stream_id: u32,
    sendable: bool,
    local_closed: Arc<AtomicBool>,
    kind: Kind,
    conn_ctx: Arc<ConnectionContext>,
}

/// Untyped receiving half of a stream.
///
/// Inbound transforms are applied in wire order before messages reach this receiver. Dropping this
/// value unregisters the stream from its connection.
#[derive(Debug)]
pub struct StreamReceiver {
    rx: ResultReceiver,
    stream_id: u32,
    recveivable: bool,
    remote_closed: bool,
    kind: Kind,
    streams: Arc<Mutex<HashMap<u32, ResultSender>>>,
}

impl Drop for StreamReceiver {
    fn drop(&mut self) {
        self.streams.lock().unwrap().remove(&self.stream_id);
    }
}

impl StreamSender {
    /// Sends one untyped payload.
    ///
    /// The connection's outbound payload transform is applied before the payload is framed as a
    /// DATA message and sent to the transport.
    ///
    /// # Errors
    ///
    /// Returns an error if the stream is closed, the payload is oversized, transformation fails,
    /// or transport fails.
    pub async fn send(&self, buf: Vec<u8>) -> Result<()> {
        debug_assert!(self.sendable);
        if self.local_closed.load(Ordering::Relaxed) {
            debug_assert_eq!(self.kind, Kind::Client);
            return Err(Error::LocalClosed);
        }

        let mut msg = GenMessage::new_data(self.stream_id, buf);
        // ── Injection Point 9/10: streaming DATA transform_outbound ──
        self.conn_ctx.transform_send(&mut msg, &self.tx, false, true).await?;

        Ok(())
    }

    /// Closes the sending half of a client-side stream.
    ///
    /// # Errors
    ///
    /// Returns [`Error::LocalClosed`] if already closed, or a transport error if the close frame
    /// cannot be sent.
    pub async fn close_send(&self) -> Result<()> {
        debug_assert_eq!(self.kind, Kind::Client);
        debug_assert!(self.sendable);
        if self.local_closed.load(Ordering::Relaxed) {
            return Err(Error::LocalClosed);
        }
        let mut msg = GenMessage::new_close(self.stream_id);
        self.conn_ctx.transform_send(&mut msg, &self.tx, false, true).await?;
        self.local_closed.store(true, Ordering::Relaxed);
        Ok(())
    }
}

impl StreamReceiver {
    /// Receives the next untyped payload or terminal response.
    ///
    /// Inbound transforms are applied in wire order before messages reach this receiver.
    ///
    /// # Errors
    ///
    /// Returns [`Error::RemoteClosed`] after the remote half is closed, [`Error::Eof`] for a
    /// graceful end-of-stream frame, [`Error::RpcStatus`] for a non-OK response, or another error
    /// for malformed frames, transformation failures, and transport failures.
    pub async fn recv(&mut self) -> Result<Vec<u8>> {
        if self.remote_closed {
            return Err(Error::RemoteClosed);
        }
        let msg = _recv(&mut self.rx).await?;

        let payload = match msg.header.type_ {
            MESSAGE_TYPE_RESPONSE => {
                debug_assert_eq!(self.kind, Kind::Client);
                self.remote_closed = true;
                let resp = Response::decode(&msg.payload)
                    .map_err(err_to_others_err!(e, "Decode message failed."))?;
                if let Some(status) = resp.status.as_ref() {
                    if status.code() != Code::OK {
                        return Err(Error::RpcStatus((*status).clone()));
                    }
                }
                resp.payload
            }
            MESSAGE_TYPE_DATA => {
                if !self.recveivable {
                    self.remote_closed = true;
                    return Err(Error::Others(
                        "received data from non-streaming server.".to_string(),
                    ));
                }
                // Close-flag checks on pre-decoded payload.
                // Transform was applied in wire order by the connection reader.
                // A tampered close would have failed AEAD verification there
                // and never reached this point.
                if (msg.header.flags & FLAG_REMOTE_CLOSED) == FLAG_REMOTE_CLOSED {
                    self.remote_closed = true;
                    if (msg.header.flags & FLAG_NO_DATA) == FLAG_NO_DATA {
                        // Enforce protocol invariant: close frame must carry
                        // no payload after decryption. Prevents a peer from
                        // smuggling data on a close frame.
                        if !msg.payload.is_empty() {
                            return Err(Error::Others(format!(
                                "stream {}: close message cannot include data",
                                self.stream_id
                            )));
                        }
                        return Err(Error::Eof);
                    }
                }
                check_oversize(msg.payload.len(), false)?;
                msg.payload
            }
            _ => {
                return Err(Error::Others("not support".to_string()));
            }
        };
        Ok(payload)
    }
}
