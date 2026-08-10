// Copyright 2022 Alibaba Cloud. All rights reserved.
// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::error::Result;
use crate::security_extension::ConnectionData;
use crate::proto::{MessageHeader, Request, Response};

/// Handle request in async mode.
#[macro_export]
macro_rules! async_request_handler {
    ($class: ident, $ctx: ident, $req: ident, $req_type: path, $req_fn: ident) => {
        let mut req = <$req_type>::new();
        {
            let mut s = CodedInputStream::from_bytes(&$req.payload);
            req.merge_from(&mut s)
                .map_err(::ttrpc::err_to_others!(e, ""))?;
        }

        let mut res = ::ttrpc::Response::new();
        match $class.service.$req_fn(&$ctx, req).await {
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

        return Ok(res);
    };
    ($class: ident, $ctx: ident, $req: ident, $server: ident, $req_type: ident, $req_fn: ident) => {
        $crate::async_request_handler!(
            $class,
            $ctx,
            $req,
            super::$server::$req_type,
            $req_fn
        )
    };
}

/// Handle client streaming in async mode.
#[macro_export]
macro_rules! async_client_streamimg_handler {
    ($class: ident, $ctx: ident, $inner: ident, $req_fn: ident) => {
        let stream = ::ttrpc::r#async::ServerStreamReceiver::new($inner);
        let mut res = ::ttrpc::Response::new();
        match $class.service.$req_fn(&$ctx, stream).await {
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
        return Ok(Some(res));
    };
}

/// Handle server streaming in async mode.
#[macro_export]
macro_rules! async_server_streamimg_handler {
    ($class: ident, $ctx: ident, $inner: ident, $req_type: path, $req_fn: ident) => {
        let req_buf = $inner.recv().await?;
        let req = <$req_type as ::ttrpc::proto::Codec>::decode(&req_buf)
            .map_err(|e| ::ttrpc::Error::Others(e.to_string()))?;
        let stream = ::ttrpc::r#async::ServerStreamSender::new($inner);
        match $class.service.$req_fn(&$ctx, req, stream).await {
            Ok(_) => {
                return Ok(None);
            }
            Err(x) => {
                let mut res = ::ttrpc::Response::new();
                match x {
                    ::ttrpc::Error::RpcStatus(s) => {
                        res.set_status(s);
                    }
                    _ => {
                        res.set_status(::ttrpc::get_status(
                            ::ttrpc::Code::UNKNOWN,
                            format!("{:?}", x),
                        ));
                    }
                }
                return Ok(Some(res));
            }
        }
    };
    ($class: ident, $ctx: ident, $inner: ident, $server: ident, $req_type: ident, $req_fn: ident) => {
        $crate::async_server_streamimg_handler!(
            $class,
            $ctx,
            $inner,
            super::$server::$req_type,
            $req_fn
        )
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
                let mut res = ::ttrpc::Response::new();
                match x {
                    ::ttrpc::Error::RpcStatus(s) => {
                        res.set_status(s);
                    }
                    _ => {
                        res.set_status(::ttrpc::get_status(
                            ::ttrpc::Code::UNKNOWN,
                            format!("{:?}", x),
                        ));
                    }
                }
                return Ok(Some(res));
            }
        }
    };
}

/// Send request through async client.
#[macro_export]
macro_rules! async_client_request {
    ($self: ident, $ctx: ident, $req: ident, $server: expr, $method: expr, $cres: ident) => {
        let mut creq = ttrpc::Request {
            service: $server.to_string(),
            method: $method.to_string(),
            timeout_nano: $ctx.timeout_nano,
            metadata: ttrpc::context::to_pb($ctx.metadata),
            payload: Vec::with_capacity($req.compute_size() as usize),
            ..Default::default()
        };

        {
            let mut s = CodedOutputStream::vec(&mut creq.payload);
            $req.write_to(&mut s)
                .map_err(::ttrpc::err_to_others!(e, ""))?;
            s.flush().map_err(::ttrpc::err_to_others!(e, ""))?;
        }

        let res = $self.client.request(creq).await?;
        let mut s = CodedInputStream::from_bytes(&res.payload);
        $cres
            .merge_from(&mut s)
            .map_err(::ttrpc::err_to_others!(e, "Unpack get error "))?;

        return Ok($cres);
    };
}

/// Duplex streaming through async client.
#[macro_export]
macro_rules! async_client_stream {
    ($self: ident, $ctx: ident, $server: expr, $method: expr) => {
        let mut creq = ::ttrpc::Request::new();
        creq.set_service($server.to_string());
        creq.set_method($method.to_string());
        creq.set_timeout_nano($ctx.timeout_nano);
        let md = ::ttrpc::context::to_pb($ctx.metadata);
        creq.set_metadata(md);

        let inner = $self.client.new_stream(creq, true, true).await?;
        let stream = ::ttrpc::r#async::ClientStream::new(inner);

        return Ok(stream);
    };
}

/// Only send streaming through async client.
#[macro_export]
macro_rules! async_client_stream_send {
    ($self: ident, $ctx: ident, $server: expr, $method: expr) => {
        let mut creq = ::ttrpc::Request::new();
        creq.set_service($server.to_string());
        creq.set_method($method.to_string());
        creq.set_timeout_nano($ctx.timeout_nano);
        let md = ::ttrpc::context::to_pb($ctx.metadata);
        creq.set_metadata(md);

        let inner = $self.client.new_stream(creq, true, false).await?;
        let stream = ::ttrpc::r#async::ClientStreamSender::new(inner);

        return Ok(stream);
    };
}

/// Only receive streaming through async client.
#[macro_export]
macro_rules! async_client_stream_receive {
    ($self: ident, $ctx: ident, $req: ident, $server: expr, $method: expr) => {
        let mut creq = ::ttrpc::Request::new();
        creq.set_service($server.to_string());
        creq.set_method($method.to_string());
        creq.set_timeout_nano($ctx.timeout_nano);
        let md = ::ttrpc::context::to_pb($ctx.metadata);
        creq.set_metadata(md);
        creq.payload.reserve($req.compute_size() as usize);
        {
            let mut s = CodedOutputStream::vec(&mut creq.payload);
            $req.write_to(&mut s)
                .map_err(::ttrpc::err_to_others!(e, ""))?;
            s.flush().map_err(::ttrpc::err_to_others!(e, ""))?;
        }

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
