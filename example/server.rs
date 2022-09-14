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
mod utils;

#[macro_use]
extern crate log;

use log::LevelFilter;
use std::sync::Arc;
use std::thread;

use protocols::sync::{agent, agent_ttrpc, health, health_ttrpc, types};
use ttrpc::error::{Error, Result};
use ttrpc::proto::{Code, Status};
use ttrpc::Server;

struct HealthService;
impl health_ttrpc::Health for HealthService {
    fn check(
        &self,
        _ctx: &::ttrpc::TtrpcContext,
        _req: health::CheckRequest,
    ) -> Result<health::HealthCheckResponse> {
        let mut status = Status::new();
        status.set_code(Code::NOT_FOUND);
        status.set_message("Just for fun".to_string());
        Err(Error::RpcStatus(status))
    }

    fn version(
        &self,
        ctx: &::ttrpc::TtrpcContext,
        req: health::CheckRequest,
    ) -> Result<health::VersionCheckResponse> {
        info!("version {:?}", req);
        info!("ctx {:?}", ctx);
        let mut rep = health::VersionCheckResponse::new();
        rep.agent_version = "mock.0.1".to_string();
        rep.grpc_version = "0.0.1".to_string();
        let mut status = Status::new();
        status.set_code(Code::NOT_FOUND);
        Ok(rep)
    }
}

struct AgentService;
impl agent_ttrpc::AgentService for AgentService {
    fn list_interfaces(
        &self,
        _ctx: &::ttrpc::TtrpcContext,
        _req: agent::ListInterfacesRequest,
    ) -> ::ttrpc::Result<agent::Interfaces> {
        Ok(agent::Interfaces {
            Interfaces: vec![
                types::Interface {
                    name: "first".to_string(),
                    ..Default::default()
                },
                types::Interface {
                    name: "second".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        })
    }
}

fn main() {
    simple_logging::log_to_stderr(LevelFilter::Trace);

    let h = Box::new(HealthService {}) as Box<dyn health_ttrpc::Health + Send + Sync>;
    let h = Arc::new(h);
    let hservice = health_ttrpc::create_health(h);

    let a = Box::new(AgentService {}) as Box<dyn agent_ttrpc::AgentService + Send + Sync>;
    let a = Arc::new(a);
    let aservice = agent_ttrpc::create_agent_service(a);

    utils::remove_if_sock_exist(utils::SOCK_ADDR).unwrap();
    let mut server = Server::new()
        .bind(utils::SOCK_ADDR)
        .unwrap()
        .register_service(hservice)
        .register_service(aservice);

    server.start().unwrap();

    // Hold the main thread until receiving signal SIGTERM
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        ctrlc::set_handler(move || {
            tx.send(()).unwrap();
        })
        .expect("Error setting Ctrl-C handler");
        println!("Server is running, press Ctrl + C to exit");
    });

    rx.recv().unwrap();
}
