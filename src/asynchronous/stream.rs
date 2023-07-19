// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use std::cmp;

use byteorder::{BigEndian, ByteOrder};

use crate::common::{
    check_oversize, convert_msg_to_buf, DEFAULT_PAGE_SIZE, MESSAGE_HEADER_LENGTH,
    MESSAGE_TYPE_RESPONSE,
};
use crate::error::{get_rpc_status, sock_error_msg, Error, Result};
use crate::r#async::utils;
use crate::ttrpc::{Code, Response, Status};
use crate::MessageHeader;
use tokio::io::AsyncReadExt;

async fn receive_count<T>(reader: &mut T, count: usize) -> Result<Vec<u8>>
where
    T: AsyncReadExt + std::marker::Unpin,
{
    let mut content = vec![0u8; count];
    if let Err(e) = reader.read_exact(&mut content).await {
        return Err(Error::Socket(e.to_string()));
    }

    Ok(content)
}

async fn discard_count<T>(reader: &mut T, count: usize) -> Result<()>
where
    T: AsyncReadExt + std::marker::Unpin,
{
    let mut need_discard = count;

    while need_discard > 0 {
        let once_discard = cmp::min(DEFAULT_PAGE_SIZE, need_discard);
        receive_count(reader, once_discard).await?;
        need_discard -= once_discard;
    }

    Ok(())
}

async fn receive_header<T>(reader: &mut T) -> Result<MessageHeader>
where
    T: AsyncReadExt + std::marker::Unpin,
{
    let buf = receive_count(reader, MESSAGE_HEADER_LENGTH).await?;
    let size = buf.len();
    if size != MESSAGE_HEADER_LENGTH {
        return Err(sock_error_msg(
            size,
            format!("Message header length {} is too small", size),
        ));
    }

    let mut mh = MessageHeader::default();
    let mut covbuf: &[u8] = &buf[..4];
    mh.length = byteorder::ReadBytesExt::read_u32::<BigEndian>(&mut covbuf)
        .map_err(err_to_rpc_err!(Code::INVALID_ARGUMENT, e, ""))?;
    let mut covbuf: &[u8] = &buf[4..8];
    mh.stream_id = byteorder::ReadBytesExt::read_u32::<BigEndian>(&mut covbuf)
        .map_err(err_to_rpc_err!(Code::INVALID_ARGUMENT, e, ""))?;
    mh.type_ = buf[8];
    mh.flags = buf[9];

    Ok(mh)
}

pub async fn receive<T>(reader: &mut T) -> Result<(MessageHeader, Result<Vec<u8>>)>
where
    T: AsyncReadExt + std::marker::Unpin,
{
    let mh = receive_header(reader).await?;
    trace!("Got Message header {:?}", mh);

    let mh_len = mh.length as usize;
    if let Err(e) = check_oversize(mh_len, true) {
        discard_count(reader, mh_len).await?;
        return Ok((mh, Err(e)));
    }

    let buf = receive_count(reader, mh.length as usize).await?;
    let size = buf.len();
    if size != mh.length as usize {
        return Err(sock_error_msg(
            size,
            format!("Message length {} is not {}", size, mh.length),
        ));
    }
    trace!("Got Message body {:?}", buf);

    Ok((mh, Ok(buf)))
}

fn header_to_buf(mh: MessageHeader) -> Vec<u8> {
    let mut buf = vec![0u8; MESSAGE_HEADER_LENGTH];

    let covbuf: &mut [u8] = &mut buf[..4];
    BigEndian::write_u32(covbuf, mh.length);
    let covbuf: &mut [u8] = &mut buf[4..8];
    BigEndian::write_u32(covbuf, mh.stream_id);
    buf[8] = mh.type_;
    buf[9] = mh.flags;

    buf
}

pub fn to_req_buf(stream_id: u32, mut body: Vec<u8>) -> Vec<u8> {
    let header = utils::get_request_header_from_body(stream_id, &body);
    let mut buf = header_to_buf(header);
    buf.append(&mut body);

    buf
}

pub fn to_res_buf(stream_id: u32, mut body: Vec<u8>) -> Vec<u8> {
    let header = utils::get_response_header_from_body(stream_id, &body);
    let mut buf = header_to_buf(header);
    buf.append(&mut body);

    buf
}

pub async fn respond(
    tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    stream_id: u32,
    body: Vec<u8>,
) -> Result<()> {
    let buf = to_res_buf(stream_id, body);

    tx.send(buf)
        .await
        .map_err(err_to_others_err!(e, "Send packet to sender error "))
}

pub async fn respond_with_status(
    tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    stream_id: u32,
    status: Status,
) -> Result<()> {
    let mut res = Response::new();
    res.set_status(status);
    let mut body = convert_msg_to_buf(&res)?;

    let mh = MessageHeader {
        length: body.len() as u32,
        stream_id,
        type_: MESSAGE_TYPE_RESPONSE,
        flags: 0,
    };

    let mut buf = header_to_buf(mh);
    buf.append(&mut body);

    tx.send(buf)
        .await
        .map_err(err_to_others_err!(e, "Send packet to sender error "))
}

pub(crate) async fn respond_error(
    tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    stream_id: u32,
    e: Error,
) -> Result<()> {
    let status = if let Error::RpcStatus(stat) = e {
        stat
    } else {
        Status {
            code: Code::UNKNOWN,
            message: format!("{:?}", e),
            ..Default::default()
        }
    };

    respond_with_status(tx, stream_id, status).await
}
