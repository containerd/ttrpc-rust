// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

mod protocols;
mod utils;

#[macro_use]
extern crate log;

use std::sync::Arc;

use log::LevelFilter;

use protocols::r#async::{agent, health, types};
use ttrpc::asynchronous::Server;
use ttrpc::error::Result;

use async_trait::async_trait;
use tokio::signal::unix::{signal, SignalKind};
use tokio::time::sleep;

struct HealthService;

#[async_trait]
impl health::Health for HealthService {
    async fn check(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        _req: health::CheckRequest,
    ) -> Result<health::HealthCheckResponse> {
        // Mock timeout
        sleep(std::time::Duration::from_secs(1)).await;
        unreachable!()
    }

    async fn version(
        &self,
        ctx: &::ttrpc::r#async::TtrpcContext,
        req: health::CheckRequest,
    ) -> Result<health::VersionCheckResponse> {
        info!("version {:?}", req);
        info!("ctx {:?}", ctx);
        Ok(health::VersionCheckResponse {
            agent_version: "mock 0.1".to_string(),
            grpc_version: "0.0.1".to_string(),
        })
    }
}

struct AgentService;

#[async_trait]
impl agent::AgentService for AgentService {
    async fn list_interfaces(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        _req: agent::ListInterfacesRequest,
    ) -> ::ttrpc::Result<agent::Interfaces> {
        Ok(agent::Interfaces {
            interfaces: vec![
                types::Interface {
                    name: "first".to_string(),
                    ..Default::default()
                },
                types::Interface {
                    name: "second".to_string(),
                    ..Default::default()
                },
            ],
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    simple_logging::log_to_stderr(LevelFilter::Trace);

    let hservice = health::create_health(Arc::new(HealthService {}));
    let aservice = agent::create_agent_service(Arc::new(AgentService {}));

    utils::remove_if_sock_exist(utils::SOCK_ADDR).unwrap();

    let mut server = Server::new()
        .bind(utils::SOCK_ADDR)
        .unwrap()
        .register_service(hservice)
        .register_service(aservice);

    let mut hangup = signal(SignalKind::hangup()).unwrap();
    let mut interrupt = signal(SignalKind::interrupt()).unwrap();
    server.start().await.unwrap();

    tokio::select! {
        _ = hangup.recv() => {
            // test stop_listen -> start
            println!("stop listen");
            server.stop_listen().await;
            println!("start listen");
            server.start().await.unwrap();

            // hold some time for the new test connection.
            sleep(std::time::Duration::from_secs(100)).await;
        }
        _ = interrupt.recv() => {
            // test graceful shutdown
            println!("graceful shutdown");
            server.shutdown().await.unwrap();
        }
    };
}
