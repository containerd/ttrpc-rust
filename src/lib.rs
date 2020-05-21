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
pub mod common;
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub mod ttrpc;

pub use crate::common::MessageHeader;
pub use crate::error::{get_status, Error, Result};
pub use crate::ttrpc::{Code, Request, Response, Status};

cfg_sync! {
    pub mod sync;
    pub use crate::sync::channel::{write_message};
    pub use crate::sync::utils::{response_to_channel, MethodHandler, TtrpcContext};
    pub use crate::sync::client;
    pub use crate::sync::client::Client;
    pub use crate::sync::server;
    pub use crate::sync::server::Server;
}

cfg_async! {
    pub mod asynchronous;
    pub use crate::asynchronous as r#async;
}
