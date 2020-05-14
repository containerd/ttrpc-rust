// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use crate::common::{MessageHeader, MESSAGE_TYPE_RESPONSE};
use crate::error::{Error, Result};
use crate::ttrpc::{Request, Response};
use async_trait::async_trait;
use protobuf::Message;

#[macro_export]
macro_rules! async_request_handler {
    ($class: ident, $ctx: ident, $req: ident, $server: ident, $req_type: ident, $req_fn: ident) => {
        let mut req = super::$server::$req_type::new();
        {
            let mut s = CodedInputStream::from_bytes(&$req.payload);
            req.merge_from(&mut s)
                .map_err(::ttrpc::Err_to_Others!(e, ""))?;
        }

        let mut res = ::ttrpc::Response::new();
        match $class.service.$req_fn(&$ctx, req).await {
            Ok(rep) => {
                res.set_status(::ttrpc::get_status(::ttrpc::Code::OK, "".to_string()));
                res.payload.reserve(rep.compute_size() as usize);
                let mut s = protobuf::CodedOutputStream::vec(&mut res.payload);
                rep.write_to(&mut s)
                    .map_err(::ttrpc::Err_to_Others!(e, ""))?;
                s.flush().map_err(::ttrpc::Err_to_Others!(e, ""))?;
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

        let buf = ::ttrpc::r#async::convert_response_to_buf(res)?;
        return Ok(($ctx.mh.stream_id, buf));
    };
}

#[async_trait]
pub trait MethodHandler {
    async fn handler(&self, ctx: TtrpcContext, req: Request) -> Result<(u32, Vec<u8>)>;
}

#[derive(Debug)]
pub struct TtrpcContext {
    pub fd: std::os::unix::io::RawFd,
    pub mh: MessageHeader,
}

pub fn convert_response_to_buf(res: Response) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(res.compute_size() as usize);
    let mut s = protobuf::CodedOutputStream::vec(&mut buf);
    res.write_to(&mut s).map_err(err_to_Others!(e, ""))?;
    s.flush().map_err(err_to_Others!(e, ""))?;

    Ok(buf)
}

pub fn get_response_header_from_body(stream_id: u32, body: &Vec<u8>) -> MessageHeader {
    MessageHeader {
        length: body.len() as u32,
        stream_id,
        type_: MESSAGE_TYPE_RESPONSE,
        flags: 0,
    }
}
