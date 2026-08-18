// Copyright 2022 Alibaba Cloud. All rights reserved.
// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::error::Result;
use crate::proto::{MessageHeader, Request, Response};
use crate::security_extension::ConnectionData;

/// Handle request in async mode.
///
/// Both the historical six-argument rust-protobuf form
/// (`super::$server::$req_type`) and the five-argument Prost form
/// (path-aware `$req_type`) are supported and share one backend-neutral
/// implementation built on [`Codec`](crate::proto::Codec).
#[macro_export]
macro_rules! async_request_handler {
    // Prost-style path-aware form, e.g.
    // `async_request_handler!(self, ctx, req, super::types::Foo, check)`.
    ($class: ident, $ctx: ident, $req: ident, $req_type: path, $req_fn: ident) => {
        let req = <$req_type as $crate::proto::Codec>::decode(&$req.payload)
            .map_err($crate::err_to_others!(e, "Unpack request error "))?;

        let res = match $class.service.$req_fn(&$ctx, req).await {
            Ok(rep) => {
                let payload = $crate::proto::Codec::encode(&rep)
                    .map_err($crate::err_to_others!(e, "Encoding response "))?;
                let mut res =
                    $crate::proto::ResponseInit::init_status($crate::get_status(
                        $crate::Code::OK,
                        "".to_string(),
                    ));
                $crate::proto::ResponseInit::set_payload(&mut res, payload);
                res
            }
            Err(x) => match x {
                $crate::Error::RpcStatus(s) => {
                    $crate::proto::ResponseInit::init_status(s)
                }
                _ => $crate::proto::ResponseInit::init_status($crate::get_status(
                    $crate::Code::UNKNOWN,
                    format!("{:?}", x),
                )),
            },
        };

        return Ok(res);
    };
    // rust-protobuf six-argument form: `super::$server::$req_type`.
    ($class: ident, $ctx: ident, $req: ident, $server: ident, $req_type: ident, $req_fn: ident) => {
        $crate::async_request_handler!($class, $ctx, $req, super::$server::$req_type, $req_fn);
    };
}

/// Handle client streaming in async mode.
#[macro_export]
macro_rules! async_client_streamimg_handler {
    ($class: ident, $ctx: ident, $inner: ident, $req_fn: ident) => {
        let stream = ::ttrpc::r#async::ServerStreamReceiver::new($inner);
        let res = match $class.service.$req_fn(&$ctx, stream).await {
            Ok(rep) => {
                let payload = $crate::proto::Codec::encode(&rep)
                    .map_err($crate::err_to_others!(e, "Encoding response "))?;
                let mut res =
                    $crate::proto::ResponseInit::init_status($crate::get_status(
                        $crate::Code::OK,
                        "".to_string(),
                    ));
                $crate::proto::ResponseInit::set_payload(&mut res, payload);
                res
            }
            Err(x) => match x {
                $crate::Error::RpcStatus(s) => {
                    $crate::proto::ResponseInit::init_status(s)
                }
                _ => $crate::proto::ResponseInit::init_status($crate::get_status(
                    $crate::Code::UNKNOWN,
                    format!("{:?}", x),
                )),
            },
        };
        return Ok(Some(res));
    };
}

/// Handle server streaming in async mode.
///
/// Both the historical six-argument rust-protobuf form and the five-argument
/// Prost form (path-aware `$req_type`) are supported.
#[macro_export]
macro_rules! async_server_streamimg_handler {
    // Prost-style path-aware form.
    ($class: ident, $ctx: ident, $inner: ident, $req_type: path, $req_fn: ident) => {
        let req_buf = $inner.recv().await?;
        let req = <$req_type as $crate::proto::Codec>::decode(&req_buf)
            .map_err(|e| $crate::Error::Others(e.to_string()))?;
        let stream = ::ttrpc::r#async::ServerStreamSender::new($inner);
        match $class.service.$req_fn(&$ctx, req, stream).await {
            Ok(_) => {
                return Ok(None);
            }
            Err(x) => {
                let res = match x {
                    $crate::Error::RpcStatus(s) => {
                        $crate::proto::ResponseInit::init_status(s)
                    }
                    _ => $crate::proto::ResponseInit::init_status($crate::get_status(
                        $crate::Code::UNKNOWN,
                        format!("{:?}", x),
                    )),
                };
                return Ok(Some(res));
            }
        }
    };
    // rust-protobuf six-argument form: `super::$server::$req_type`.
    ($class: ident, $ctx: ident, $inner: ident, $server: ident, $req_type: ident, $req_fn: ident) => {
        $crate::async_server_streamimg_handler!(
            $class,
            $ctx,
            $inner,
            super::$server::$req_type,
            $req_fn
        );
    };
}

/// Handle duplex streaming in async mode.
#[macro_export]
macro_rules! async_duplex_streamimg_handler {
    ($class: ident, $ctx: ident, $inner: ident, $req_fn: ident) => {
        let stream = ::ttrpc::r#async::ServerStream::new($inner);
        match $class.service.$req_fn(&$ctx, stream).await {
            Ok(_) => {
                return Ok(None);
            }
            Err(x) => {
                let res = match x {
                    $crate::Error::RpcStatus(s) => {
                        $crate::proto::ResponseInit::init_status(s)
                    }
                    _ => $crate::proto::ResponseInit::init_status($crate::get_status(
                        $crate::Code::UNKNOWN,
                        format!("{:?}", x),
                    )),
                };
                return Ok(Some(res));
            }
        }
    };
}

/// Send request through async client.
#[macro_export]
macro_rules! async_client_request {
    ($self: ident, $ctx: ident, $req: ident, $server: expr, $method: expr, $cres: ident) => {
        let payload = $crate::proto::Codec::encode($req)
            .map_err($crate::err_to_others!(e, "Encoding request "))?;
        let mut creq = $crate::proto::RequestInit::init_request(
            $server.to_string(),
            $method.to_string(),
            $ctx.timeout_nano,
            $crate::context::to_pb($ctx.metadata),
        );
        $crate::proto::RequestInit::set_payload(&mut creq, payload);

        let res = $self.client.request(creq).await?;
        $crate::proto::Codec::merge(&mut $cres, &res.payload)
            .map_err($crate::err_to_others!(e, "Unpack get error "))?;

        return Ok($cres);
    };
}

/// Duplex streaming through async client.
#[macro_export]
macro_rules! async_client_stream {
    ($self: ident, $ctx: ident, $server: expr, $method: expr) => {
        let creq = $crate::proto::RequestInit::init_request(
            $server.to_string(),
            $method.to_string(),
            $ctx.timeout_nano,
            $crate::context::to_pb($ctx.metadata),
        );

        let inner = $self.client.new_stream(creq, true, true).await?;
        let stream = ::ttrpc::r#async::ClientStream::new(inner);

        return Ok(stream);
    };
}

/// Only send streaming through async client.
#[macro_export]
macro_rules! async_client_stream_send {
    ($self: ident, $ctx: ident, $server: expr, $method: expr) => {
        let creq = $crate::proto::RequestInit::init_request(
            $server.to_string(),
            $method.to_string(),
            $ctx.timeout_nano,
            $crate::context::to_pb($ctx.metadata),
        );

        let inner = $self.client.new_stream(creq, true, false).await?;
        let stream = ::ttrpc::r#async::ClientStreamSender::new(inner);

        return Ok(stream);
    };
}

/// Only receive streaming through async client.
#[macro_export]
macro_rules! async_client_stream_receive {
    ($self: ident, $ctx: ident, $req: ident, $server: expr, $method: expr) => {
        let payload = $crate::proto::Codec::encode($req)
            .map_err($crate::err_to_others!(e, "Encoding request "))?;
        let mut creq = $crate::proto::RequestInit::init_request(
            $server.to_string(),
            $method.to_string(),
            $ctx.timeout_nano,
            $crate::context::to_pb($ctx.metadata),
        );
        $crate::proto::RequestInit::set_payload(&mut creq, payload);

        let inner = $self.client.new_stream(creq, false, true).await?;
        let stream = ::ttrpc::r#async::ClientStreamReceiver::new(inner, $self.client.clone());

        return Ok(stream);
    };
}

/// Dispatches a request to an asynchronous unary service method.
///
/// This trait is implemented by generated service bindings.
#[async_trait]
pub trait MethodHandler {
    /// Handles one decoded unary request and returns its response.
    async fn handler(&self, ctx: TtrpcContext, req: Request) -> Result<Response>;
}

/// Dispatches a request to an asynchronous streaming service method.
///
/// This trait is implemented by generated service bindings.
#[async_trait]
pub trait StreamHandler {
    /// Handles one streaming request.
    async fn handler(
        &self,
        ctx: TtrpcContext,
        stream: crate::r#async::StreamInner,
    ) -> Result<Option<Response>>;
}

/// Server-side context for an asynchronous request.
///
/// Generated service traits receive this value by reference. It implements [`Default`] so test and
/// mock code can specify only the fields it needs.
#[derive(Debug, Default)]
pub struct TtrpcContext {
    /// Wire header associated with the request.
    pub mh: MessageHeader,
    /// Request metadata grouped by key.
    pub metadata: HashMap<String, Vec<String>>,
    /// Client-provided timeout in nanoseconds, or zero if no timeout was set.
    pub timeout_nano: i64,

    /// Opaque per-connection data supplied by an accept hook. Immutable after accept.
    /// See [`ConnectionData`](crate::security_extension::ConnectionData) for full contract.
    /// Empty when `security_extension` is not enabled.
    pub connection_data: Arc<ConnectionData>,
}

pub(crate) fn get_path(service: &str, method: &str) -> String {
    format!("/{service}/{method}")
}
