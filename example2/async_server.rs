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
use ttrpc::error::{Error, Result};
use ttrpc::proto::{Code, Status};

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
        let mut status = Status::default();

        status.code = Code::NOT_FOUND as i32;
        status.message = "Just for fun".to_string();

        sleep(std::time::Duration::from_secs(10)).await;

        Err(Error::RpcStatus(status))
    }

    async fn version(
        &self,
        ctx: &::ttrpc::r#async::TtrpcContext,
        req: health::CheckRequest,
    ) -> Result<health::VersionCheckResponse> {
        info!("version {:?}", req);
        info!("ctx {:?}", ctx);
        let mut rep = health::VersionCheckResponse::default();
        rep.agent_version = "mock.0.1".to_string();
        rep.grpc_version = "0.0.1".to_string();
        let mut status = Status::default();
        status.code = Code::NOT_FOUND as i32;
        Ok(rep)
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
        let mut rp = Vec::new();

        let mut i = types::Interface::default();
        i.name = "first".to_string();
        rp.push(i);
        let mut i = types::Interface::default();
        i.name = "second".to_string();
        rp.push(i);

        let mut i = agent::Interfaces::default();
        i.interfaces = rp;

        Ok(i)
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    simple_logging::log_to_stderr(LevelFilter::Trace);

    let h = Box::new(HealthService {}) as Box<dyn health::Health + Send + Sync>;
    let h = Arc::new(h);
    let hservice = health::create_health(h);

    let a = Box::new(AgentService {}) as Box<dyn agent::AgentService + Send + Sync>;
    let a = Arc::new(a);
    let aservice = agent::create_agent_service(a);

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
