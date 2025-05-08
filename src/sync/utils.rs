// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use crate::error::{Error, Result};
#[allow(unused_imports)]
use crate::proto::{
    check_oversize, Codec, MessageHeader, Request, Response, MESSAGE_TYPE_RESPONSE,
};

use std::collections::HashMap;

#[cfg(not(feature = "prost"))]
/// Response message through a channel.
/// Eventually  the message will sent to Client.
pub fn response_to_channel(
    stream_id: u32,
    res: Response,
    tx: std::sync::mpsc::Sender<(MessageHeader, Vec<u8>)>,
) -> Result<()> {
    let mut buf = res.encode().map_err(err_to_others_err!(e, ""))?;

    if let Err(e) = check_oversize(buf.len(), true) {
        let resp: Response = e.into();
        buf = resp.encode().map_err(err_to_others_err!(e, ""))?;
    };

    let mh = MessageHeader {
        length: buf.len() as u32,
        stream_id,
        type_: MESSAGE_TYPE_RESPONSE,
        flags: 0,
    };

    tx.send((mh, buf)).map_err(err_to_others_err!(e, ""))?;

    Ok(())
}

#[cfg(feature = "prost")]
pub fn response_to_channel(
    stream_id: u32,
    res: Response,
    tx: std::sync::mpsc::Sender<(MessageHeader, Vec<u8>)>,
) -> Result<()> {
    let mut buffer = Vec::new();
    <Response as prost::Message>::encode(&res, &mut buffer).map_err(err_to_others_err!(e, ""))?;
    let mh = MessageHeader {
        length: buffer.len() as u32,
        stream_id,
        type_: MESSAGE_TYPE_RESPONSE,
        flags: 0,
    };
    
    tx.send((mh, buffer)).map_err(err_to_others_err!(e, ""))?;
    
    Ok(())
}


pub fn response_error_to_channel(
    stream_id: u32,
    e: Error,
    tx: std::sync::mpsc::Sender<(MessageHeader, Vec<u8>)>,
) -> Result<()> {
    response_to_channel(stream_id, e.into(), tx)
}

/// Handle request in sync mode.
#[macro_export]
#[cfg(not(feature = "prost"))]
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
        ::ttrpc::response_to_channel($ctx.mh.stream_id, res, $ctx.res_tx)?
    };
}

/// Handle request in sync mode.
#[macro_export]
#[cfg(feature = "prost")]
macro_rules! request_handler {
    ($class: ident, $ctx: ident, $req: ident, $req_type: ident, $req_fn: ident) => {
        let mut req = $req_type::default();
        req.merge(&$req.payload as &[u8])
            .map_err(::ttrpc::err_to_others!(e, ""))?;

        let mut res = ::ttrpc::Response::default();
        match $class.service.$req_fn(&$ctx, req) {
            Ok(rep) => {
                res.status = Some(::ttrpc::get_status(::ttrpc::Code::OK, "".to_string()));
                rep.encode(&mut res.payload)
                    .map_err(::ttrpc::err_to_others!(e, "Encoding error "))?;
            }
            Err(x) => match x {
                ::ttrpc::Error::RpcStatus(s) => {
                    res.status = Some(s);
                }
                _ => {
                    res.status = Some(::ttrpc::get_status(
                        ::ttrpc::Code::UNKNOWN,
                        format!("{:?}", x),
                    ));
                }
            },
        }
        ::ttrpc::response_to_channel($ctx.mh.stream_id, res, $ctx.res_tx)?
    };
}

/// Send request through sync client.
#[macro_export]
#[cfg(not(feature = "prost"))]
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

/// Send request through sync client.
#[macro_export]
#[cfg(feature = "prost")]
macro_rules! client_request {
    ($self: ident, $ctx: ident, $req: ident, $server: expr, $method: expr, $cres: ident) => {
        let mut creq = ::ttrpc::Request::default();
        creq.service = $server.to_string();
        creq.method = $method.to_string();
        creq.timeout_nano = $ctx.timeout_nano;
        let md = ::ttrpc::context::to_pb($ctx.metadata);
        creq.metadata = md;
        creq.payload.reserve($req.encoded_len());
        $req.encode(&mut creq.payload).map_err(::ttrpc::err_to_others!(e, "Encoding error "))?;

        let res = $self.client.request(creq)?;
        $cres
            .merge(&res.payload as &[u8])
            .map_err(::ttrpc::err_to_others!(e, "Unpack get error "))?;
    };
}

/// The context of ttrpc (sync).
#[derive(Debug)]
pub struct TtrpcContext {
    #[cfg(unix)]
    pub fd: std::os::unix::io::RawFd,
    #[cfg(windows)]
    pub fd: i32,
    pub cancel_rx: crossbeam::channel::Receiver<()>,
    pub mh: MessageHeader,
    pub res_tx: std::sync::mpsc::Sender<(MessageHeader, Vec<u8>)>,
    pub metadata: HashMap<String, Vec<String>>,
    pub timeout_nano: i64,
}

/// Trait that implements handler which is a proxy to the desired method (sync).
pub trait MethodHandler {
    fn handler(&self, ctx: TtrpcContext, req: Request) -> Result<()>;
}
