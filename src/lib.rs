#[macro_use]
extern crate log;

#[macro_use]
pub mod error;
#[macro_use]
mod channel;
pub mod client;
pub mod server;
pub mod ttrpc;

pub use crate::client::Client;
pub use crate::server::{Server, TtrpcContext, MethodHandler, response_to_channel};
pub use crate::ttrpc::{Request, Response, Status, Code};
pub use crate::channel::{write_message, message_header, MESSAGE_TYPE_REQUEST, MESSAGE_TYPE_RESPONSE};
pub use crate::error::{Result, Error, get_Status};
