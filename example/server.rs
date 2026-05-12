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

use std::{sync::Arc, thread};

use log::LevelFilter;
use protocols::sync::{agent, agent_ttrpc, health, health_ttrpc};

struct HealthService;
impl health_ttrpc::Health for HealthService {
    fn check(
        &self,
        _ctx: &ttrpc::TtrpcContext,
        _req: health::CheckRequest,
    ) -> ttrpc::Result<health::HealthCheckResponse> {
        // Mock timeout
        thread::sleep(std::time::Duration::from_secs(1));
        // reachable but meanless
        Err(ttrpc::Error::Eof)
    }

    fn version(
        &self,
        _ctx: &ttrpc::TtrpcContext,
        _req: health::CheckRequest,
    ) -> ttrpc::Result<health::VersionCheckResponse> {
        utils::resp::sync::health_version()
    }
}

struct AgentService;
impl agent_ttrpc::AgentService for AgentService {
    fn list_interfaces(
        &self,
        _ctx: &ttrpc::TtrpcContext,
        _req: agent::ListInterfacesRequest,
    ) -> ttrpc::Result<agent::Interfaces> {
        utils::resp::sync::agent_list_interfaces()
    }
}

fn main() {
    simple_logging::log_to_stderr(LevelFilter::Trace);
    let hservice = health_ttrpc::create_health(Arc::new(HealthService {}));
    let aservice = agent_ttrpc::create_agent_service(Arc::new(AgentService {}));

    let sock_addr = utils::get_sock_addr();
    utils::remove_if_sock_exist(sock_addr).unwrap();

    let mut server = ttrpc::Server::new()
        .bind(sock_addr)
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
