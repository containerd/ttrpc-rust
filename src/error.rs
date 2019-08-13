use std::result;
use crate::ttrpc::{Status, Code};

#[derive(Debug)]
pub enum Error {
    Socket(String),
    RpcStatus(Status),
    Others(String),
}

pub type Result<T> = result::Result<T, Error>;

pub fn get_Status(c: Code, msg: String) -> Status {
    let mut status = Status::new();
    status.set_code(c);
    status.set_message(msg);

    status
}

pub fn get_RpcStatus(c: Code, msg: String) -> Error {
    Error::RpcStatus(get_Status(c, msg))
}

macro_rules! err_to_RpcStatus {
    ($c: expr, $e: ident, $s: expr) => {
        |$e| get_RpcStatus($c, $s.to_string() + &$e.to_string())
    };
}

macro_rules! err_to_Others {
    ($e: ident, $s: expr) => {
        |$e| Error::Others($s.to_string() + &$e.to_string())
    };
}

#[macro_export]
macro_rules! Err_to_Others {
    ($e: ident, $s: expr) => {
        |$e| ::ttrpc::Error::Others($s.to_string() + &$e.to_string())
    };
}
