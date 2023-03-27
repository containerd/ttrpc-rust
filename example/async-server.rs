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

#[cfg(unix)]
use protocols::r#async::{agent, agent_ttrpc, health, health_ttrpc, types};
#[cfg(unix)]
use ttrpc::asynchronous::Server;
use ttrpc::error::{Error, Result};
use ttrpc::proto::{Code, Status};

#[cfg(unix)]
use async_trait::async_trait;
#[cfg(unix)]
use tokio::signal::unix::{signal, SignalKind};
use tokio::time::sleep;

struct HealthService;

#[cfg(unix)]
#[async_trait]
impl health_ttrpc::Health for HealthService {
    async fn check(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        _req: health::CheckRequest,
    ) -> Result<health::HealthCheckResponse> {
        let mut status = Status::new();

        status.set_code(Code::NOT_FOUND);
        status.set_message("Just for fun".to_string());

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
        let mut rep = health::VersionCheckResponse::new();
        rep.agent_version = "mock.0.1".to_string();
        rep.grpc_version = "0.0.1".to_string();
        let mut status = Status::new();
        status.set_code(Code::NOT_FOUND);
        Ok(rep)
    }
}

struct AgentService;
#[cfg(unix)]
#[async_trait]
impl agent_ttrpc::AgentService for AgentService {
    async fn list_interfaces(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        _req: agent::ListInterfacesRequest,
    ) -> ::ttrpc::Result<agent::Interfaces> {
        let mut rp = Vec::new();

        let mut i = types::Interface::new();
        i.name = "first".to_string();
        rp.push(i);
        let mut i = types::Interface::new();
        i.name = "second".to_string();
        rp.push(i);

        let mut i = agent::Interfaces::new();
        i.Interfaces = rp;

        Ok(i)
    }
}

#[cfg(windows)]
fn main() {
    println!("This example only works on Unix-like OSes");
}

#[cfg(unix)]
#[tokio::main(flavor = "current_thread")]
async fn main() {
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
