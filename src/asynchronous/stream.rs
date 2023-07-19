// Copyright 2022 Alibaba Cloud. All rights reserved.
// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::error::{Error, Result};
use crate::proto::{
    Code, Codec, GenMessage, MessageHeader, Response, FLAG_NO_DATA, FLAG_REMOTE_CLOSED,
    MESSAGE_TYPE_DATA, MESSAGE_TYPE_RESPONSE,
};

pub type MessageSender = mpsc::Sender<GenMessage>;
pub type MessageReceiver = mpsc::Receiver<GenMessage>;

pub type ResultSender = mpsc::Sender<Result<GenMessage>>;
pub type ResultReceiver = mpsc::Receiver<Result<GenMessage>>;

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

    pub fn split(self) -> (CSSender<Q>, CSReceiver<P>) {
        (self.tx, self.rx)
    }

    pub async fn send(&self, req: &Q) -> Result<()> {
        self.tx.send(req).await
    }

    pub async fn close_send(&self) -> Result<()> {
        self.tx.close_send().await
    }

    pub async fn recv(&mut self) -> Result<P> {
        self.rx.recv().await
    }
}

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
    pub async fn send(&self, req: &Q) -> Result<()> {
        let msg_buf = req
            .encode()
            .map_err(err_to_others_err!(e, "Encode message failed."))?;
        self.tx.send(msg_buf).await
    }

    pub async fn close_send(&self) -> Result<()> {
        self.tx.close_send().await
    }
}

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
    pub async fn recv(&mut self) -> Result<P> {
        let msg_buf = self.rx.recv().await?;
        P::decode(msg_buf).map_err(err_to_others_err!(e, "Decode message failed."))
    }
}

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

    pub fn split(self) -> (SSSender<P>, SSReceiver<Q>) {
        (self.tx, self.rx)
    }

    pub async fn send(&self, resp: &P) -> Result<()> {
        self.tx.send(resp).await
    }

    pub async fn recv(&mut self) -> Result<Option<Q>> {
        self.rx.recv().await
    }
}

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
    pub async fn send(&self, resp: &P) -> Result<()> {
        let msg_buf = resp
            .encode()
            .map_err(err_to_others_err!(e, "Encode message failed."))?;
        self.tx.send(msg_buf).await
    }
}

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
    pub fn new(inner: StreamInner) -> Self {
        Self {
            inner,
            _send: PhantomData,
            _recv: PhantomData,
        }
    }

    pub async fn send(&self, req: &Q) -> Result<()> {
        let msg_buf = req
            .encode()
            .map_err(err_to_others_err!(e, "Encode message failed."))?;
        self.inner.send(msg_buf).await
    }

    pub async fn close_and_recv(&mut self) -> Result<P> {
        self.inner.close_send().await?;
        let msg_buf = self.inner.recv().await?;
        P::decode(msg_buf).map_err(err_to_others_err!(e, "Decode message failed."))
    }
}

pub struct ServerStreamSender<P> {
    inner: StreamSender,
    _send: PhantomData<P>,
}

impl<P> ServerStreamSender<P>
where
    P: Codec,
    <P as Codec>::E: std::fmt::Display,
{
    pub fn new(inner: StreamInner) -> Self {
        Self {
            inner: inner.split().0,
            _send: PhantomData,
        }
    }

    pub async fn send(&self, resp: &P) -> Result<()> {
        let msg_buf = resp
            .encode()
            .map_err(err_to_others_err!(e, "Encode message failed."))?;
        self.inner.send(msg_buf).await
    }
}

pub struct ClientStreamReceiver<P> {
    inner: StreamReceiver,
    _recv: PhantomData<P>,
}

impl<P> ClientStreamReceiver<P>
where
    P: Codec,
    <P as Codec>::E: std::fmt::Display,
{
    pub fn new(inner: StreamInner) -> Self {
        Self {
            inner: inner.split().1,
            _recv: PhantomData,
        }
    }

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

pub struct ServerStreamReceiver<Q> {
    inner: StreamReceiver,
    _recv: PhantomData<Q>,
}

impl<Q> ServerStreamReceiver<Q>
where
    Q: Codec,
    <Q as Codec>::E: std::fmt::Display,
{
    pub fn new(inner: StreamInner) -> Self {
        Self {
            inner: inner.split().1,
            _recv: PhantomData,
        }
    }

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

async fn _send(tx: &MessageSender, msg: GenMessage) -> Result<()> {
    tx.send(msg)
        .await
        .map_err(|e| Error::Others(format!("Send data packet to sender error {e:?}")))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Client,
    Server,
}

#[derive(Debug)]
pub struct StreamInner {
    sender: StreamSender,
    receiver: StreamReceiver,
}

impl StreamInner {
    pub fn new(
        stream_id: u32,
        tx: MessageSender,
        rx: ResultReceiver,
        //waiter: shutdown::Waiter,
        sendable: bool,
        recveivable: bool,
        kind: Kind,
        streams: Arc<Mutex<HashMap<u32, ResultSender>>>,
    ) -> Self {
        Self {
            sender: StreamSender {
                tx,
                stream_id,
                sendable,
                local_closed: Arc::new(AtomicBool::new(false)),
                kind,
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

    pub async fn send(&self, buf: Vec<u8>) -> Result<()> {
        self.sender.send(buf).await
    }

    pub async fn close_send(&self) -> Result<()> {
        self.sender.close_send().await
    }

    pub async fn recv(&mut self) -> Result<Vec<u8>> {
        self.receiver.recv().await
    }
}

#[derive(Clone, Debug)]
pub struct StreamSender {
    tx: MessageSender,
    stream_id: u32,
    sendable: bool,
    local_closed: Arc<AtomicBool>,
    kind: Kind,
}

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
    pub async fn send(&self, buf: Vec<u8>) -> Result<()> {
        debug_assert!(self.sendable);
        if self.local_closed.load(Ordering::Relaxed) {
            debug_assert_eq!(self.kind, Kind::Client);
            return Err(Error::LocalClosed);
        }
        let header = MessageHeader::new_data(self.stream_id, buf.len() as u32);
        let msg = GenMessage {
            header,
            payload: buf,
        };

        msg.check()?;

        _send(&self.tx, msg).await?;

        Ok(())
    }

    pub async fn close_send(&self) -> Result<()> {
        debug_assert_eq!(self.kind, Kind::Client);
        debug_assert!(self.sendable);
        if self.local_closed.load(Ordering::Relaxed) {
            return Err(Error::LocalClosed);
        }
        let mut header = MessageHeader::new_data(self.stream_id, 0);
        header.set_flags(FLAG_REMOTE_CLOSED | FLAG_NO_DATA);
        let msg = GenMessage {
            header,
            payload: Vec::new(),
        };
        _send(&self.tx, msg).await?;
        self.local_closed.store(true, Ordering::Relaxed);
        Ok(())
    }
}

impl StreamReceiver {
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
                if (msg.header.flags & FLAG_REMOTE_CLOSED) == FLAG_REMOTE_CLOSED {
                    self.remote_closed = true;
                    if (msg.header.flags & FLAG_NO_DATA) == FLAG_NO_DATA {
                        return Err(Error::Eof);
                    }
                }
                msg.payload
            }
            _ => {
                return Err(Error::Others("not support".to_string()));
            }
        };
        Ok(payload)
    }
}
