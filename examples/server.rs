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

mod protocols;

#[macro_use]
extern crate log;

use std::sync::Arc;
use std::env;

use log::LevelFilter;

use ttrpc::server::*;
use ttrpc::ttrpc::{Response, Status, Code};
use ttrpc::error::{Result, Error};

struct healthService;
impl protocols::health_ttrpc::Health for healthService {
    fn check(&self, ctx: &::ttrpc::TtrpcContext, req: protocols::health::CheckRequest) -> Result<protocols::health::HealthCheckResponse> {
        let mut status = Status::new();
        status.set_code(Code::NOT_FOUND);
        status.set_message("Just for fun".to_string());
        Err(Error::RpcStatus(status))
    }
    fn version(&self, ctx: &::ttrpc::TtrpcContext, req: protocols::health::CheckRequest) -> Result<protocols::health::VersionCheckResponse> {
        info!("version {:?}", req);
        let mut rep = protocols::health::VersionCheckResponse::new();
        rep.agent_version = "mock.0.1".to_string();
        rep.grpc_version = "0.0.1".to_string();
        let mut status = Status::new();
        status.set_code(Code::NOT_FOUND);
        Ok(rep)
    }
}

struct agentService;
impl protocols::agent_ttrpc::AgentService for agentService {
    fn list_interfaces(&self, ctx: &::ttrpc::TtrpcContext, req: protocols::agent::ListInterfacesRequest) -> ::ttrpc::Result<protocols::agent::Interfaces> {
        let mut rp = protobuf::RepeatedField::new();

        let mut i = protocols::types::Interface::new();
        i.set_name("first".to_string());
        rp.push(i);
        let mut i = protocols::types::Interface::new();
        i.set_name("second".to_string());
        rp.push(i);

        let mut i = protocols::agent::Interfaces::new();
        i.set_Interfaces(rp);

        Ok(i)
        //Err(::ttrpc::Error::RpcStatus(::ttrpc::get_Status(::ttrpc::Code::NOT_FOUND, "".to_string())))
    }
}

fn main() {
    simple_logging::log_to_stderr(LevelFilter::Trace);

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        panic!("Usage: {} unix_addr", args[0]);
    }

    let h = Box::new(healthService {}) as Box<protocols::health_ttrpc::Health + Send + Sync>;
    let h = Arc::new(h);
    let hservice = protocols::health_ttrpc::create_health(h);

    let a = Box::new(agentService {}) as Box<protocols::agent_ttrpc::AgentService + Send + Sync>;
    let a = Arc::new(a);
    let aservice = protocols::agent_ttrpc::create_agent_service(a);

    let server = Server::new()
        .bind("unix:///tmp/1").unwrap()
        .register_service(hservice)
        .register_service(aservice);

    server.start().unwrap();
}
