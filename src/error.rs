// Copyright (c) 2019 Ant Financial
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Error handling for clients, servers, transports, and streams.

use crate::proto::{Code, Response, Status};
use std::result;
use thiserror::Error;

/// The error type for ttrpc.
#[derive(Error, Debug, Clone, PartialEq)]
pub enum Error {
    /// A transport or socket operation failed.
    #[error("socket err: {0}")]
    Socket(String),

    /// The remote endpoint returned a non-OK ttrpc status.
    #[error("rpc status: {0:?}")]
    RpcStatus(Status),

    /// A Unix platform operation failed.
    #[cfg(unix)]
    #[error("Nix error: {0}")]
    Nix(#[from] nix::Error),

    /// A Windows platform operation failed with the contained error code.
    #[cfg(windows)]
    #[error("Windows error: {0}")]
    Windows(i32),

    /// The local half of a stream has already been closed.
    #[error("ttrpc err: local stream closed")]
    LocalClosed,

    /// The remote half of a stream has already been closed.
    #[error("ttrpc err: remote stream closed")]
    RemoteClosed,

    /// The remote endpoint closed a stream without another message.
    #[error("eof")]
    Eof,

    /// An error that does not fit a more specific category.
    #[error("ttrpc err: {0}")]
    Others(String),
}

impl From<Error> for Response {
    fn from(e: Error) -> Self {
        let status = if let Error::RpcStatus(stat) = e {
            stat
        } else {
            get_status(Code::UNKNOWN, e)
        };

        let mut res = Response::new();
        res.set_status(status);
        res
    }
}

/// A specialized Result type for ttrpc.
pub type Result<T> = result::Result<T, Error>;

/// Creates a ttrpc [`Status`] from a status [`Code`] and message.
pub fn get_status(c: Code, msg: impl ToString) -> Status {
    let mut status = Status::new();
    status.set_code(c);
    status.set_message(msg.to_string());

    status
}

/// Creates an [`Error::RpcStatus`] from a status code and message.
pub fn get_rpc_status(c: Code, msg: impl ToString) -> Error {
    Error::RpcStatus(get_status(c, msg))
}

const SOCK_DICONNECTED: &str = "socket disconnected";
/// Converts a low-level socket read result into a ttrpc error.
///
/// A zero-byte read is reported as a disconnected socket. Other failures become an
/// `INVALID_ARGUMENT` RPC status containing `msg`.
pub fn sock_error_msg(size: usize, msg: String) -> Error {
    if size == 0 {
        return Error::Socket(SOCK_DICONNECTED.to_string());
    }

    get_rpc_status(Code::INVALID_ARGUMENT, msg)
}

macro_rules! err_to_others_err {
    ($e: ident, $s: expr) => {
        |$e| Error::Others($s.to_string() + &$e.to_string())
    };
}

/// Convert to ttrpc::Error::Others.
#[macro_export]
macro_rules! err_to_others {
    ($e: ident, $s: expr) => {
        |$e| ::ttrpc::Error::Others($s.to_string() + &$e.to_string())
    };
}
