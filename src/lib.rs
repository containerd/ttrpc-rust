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
