// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

mod protocols;
mod utils;

use std::sync::Arc;

use log::LevelFilter;

use protocols::asynchronous::{agent, agent_ttrpc, health, health_ttrpc};
use ttrpc::asynchronous::Server;

use async_trait::async_trait;
use tokio::time::sleep;

struct HealthService;

#[async_trait]
impl health_ttrpc::Health for HealthService {
    async fn check(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        _req: health::CheckRequest,
    ) -> ttrpc::Result<health::HealthCheckResponse> {
        // Mock timeout
        sleep(std::time::Duration::from_secs(1)).await;
        unreachable!();
    }

    async fn version(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        _req: health::CheckRequest,
    ) -> ttrpc::Result<health::VersionCheckResponse> {
        utils::resp::asynchronous::health_version()
    }
}

struct AgentService;
#[async_trait]
impl agent_ttrpc::AgentService for AgentService {
    async fn list_interfaces(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        _req: agent::ListInterfacesRequest,
    ) -> ::ttrpc::Result<agent::Interfaces> {
        utils::resp::asynchronous::agent_list_interfaces()
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    simple_logging::log_to_stderr(LevelFilter::Trace);

    let hservice = health_ttrpc::create_health(Arc::new(HealthService {}));
    let aservice = agent_ttrpc::create_agent_service(Arc::new(AgentService {}));

    utils::remove_if_sock_exist(utils::SOCK_ADDR).unwrap();

    let mut server = Server::new()
        .bind(utils::SOCK_ADDR)
        .unwrap()
        .register_service(hservice)
        .register_service(aservice);

    server.start().await.unwrap();

    tokio::select! {
        _ = utils::hangup() => {
            // test stop_listen -> start
            println!("stop listen");
            server.stop_listen().await;
            println!("start listen");
            server.start().await.unwrap();

            // hold some time for the new test connection.
            sleep(std::time::Duration::from_secs(100)).await;
        }
        _ = utils::interrupt() => {
            // test graceful shutdown
            println!("graceful shutdown");
            server.shutdown().await.unwrap();
        }
    };
}
