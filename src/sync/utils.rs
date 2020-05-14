// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use crate::common::{MessageHeader, MESSAGE_TYPE_RESPONSE};
use crate::error::{Error, Result};
use crate::ttrpc::{Request, Response};
use protobuf::Message;

pub fn response_to_channel(
    stream_id: u32,
    res: Response,
    tx: std::sync::mpsc::Sender<(MessageHeader, Vec<u8>)>,
) -> Result<()> {
    let mut buf = Vec::with_capacity(res.compute_size() as usize);
    let mut s = protobuf::CodedOutputStream::vec(&mut buf);
    res.write_to(&mut s).map_err(err_to_Others!(e, ""))?;
    s.flush().map_err(err_to_Others!(e, ""))?;

    let mh = MessageHeader {
        length: buf.len() as u32,
        stream_id,
        type_: MESSAGE_TYPE_RESPONSE,
        flags: 0,
    };
    tx.send((mh, buf)).map_err(err_to_Others!(e, ""))?;

    Ok(())
}

#[macro_export]
macro_rules! request_handler {
    ($class: ident, $ctx: ident, $req: ident, $server: ident, $req_type: ident, $req_fn: ident) => {
        let mut s = CodedInputStream::from_bytes(&$req.payload);
        let mut req = super::$server::$req_type::new();
        req.merge_from(&mut s)
            .map_err(::ttrpc::Err_to_Others!(e, ""))?;

        let mut res = ::ttrpc::Response::new();
        match $class.service.$req_fn(&$ctx, req) {
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
        ::ttrpc::response_to_channel($ctx.mh.stream_id, res, $ctx.res_tx)?
    };
}

#[derive(Debug)]
pub struct TtrpcContext {
    pub fd: std::os::unix::io::RawFd,
    pub mh: MessageHeader,
    pub res_tx: std::sync::mpsc::Sender<(MessageHeader, Vec<u8>)>,
}

pub trait MethodHandler {
    fn handler(&self, ctx: TtrpcContext, req: Request) -> Result<(u32, Vec<u8>)>;
}
