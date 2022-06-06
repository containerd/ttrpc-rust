// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use tokio::sync::mpsc;

use crate::error::{Error, Result};
use crate::proto::{Codec, GenMessage, Response, Status};
use crate::MessageHeader;

pub type MessageSender = mpsc::Sender<GenMessage>;
pub type MessageReceiver = mpsc::Receiver<GenMessage>;

pub type ResultSender = mpsc::Sender<Result<GenMessage>>;
pub type ResultReceiver = mpsc::Receiver<Result<GenMessage>>;

pub(crate) async fn respond(tx: MessageSender, stream_id: u32, resp: Response) -> Result<()> {
    let payload = resp
        .encode()
        .map_err(err_to_others_err!(e, "Encode Response failed."))?;
    let msg = GenMessage {
        header: MessageHeader::new_response(stream_id, payload.len() as u32),
        payload,
    };

    tx.send(msg)
        .await
        .map_err(err_to_others_err!(e, "Send packet to sender error "))
}

pub(crate) async fn respond_with_status(tx: MessageSender, stream_id: u32, status: Status) {
    let mut res = Response::new();
    res.set_status(status);

    respond(tx, stream_id, res)
        .await
        .map_err(|e| {
            error!("respond with status got error {:?}", e);
        })
        .ok();
}
