// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use tokio::sync::mpsc;

use crate::error::Result;
use crate::proto::GenMessage;

pub type MessageSender = mpsc::Sender<GenMessage>;
pub type MessageReceiver = mpsc::Receiver<GenMessage>;

pub type ResultSender = mpsc::Sender<Result<GenMessage>>;
pub type ResultReceiver = mpsc::Receiver<Result<GenMessage>>;
