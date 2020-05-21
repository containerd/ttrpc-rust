// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

mod protocols;

#[macro_use]
extern crate log;

use std::sync::Arc;

use log::LevelFilter;

use protocols::r#async::{agent, agent_ttrpc, health, health_ttrpc, types};
use ttrpc::asynchronous::server::*;
use ttrpc::error::{Error, Result};
use ttrpc::ttrpc::{Code, Status};

use async_trait::async_trait;
use tokio;
use tokio::signal::unix::{signal, SignalKind};

struct HealthService;

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

        let delay = tokio::time::delay_for(std::time::Duration::from_secs(10));
        delay.await;

        Err(Error::RpcStatus(status))
    }
    async fn version(
        &self,
        _ctx: &::ttrpc::r#async::TtrpcContext,
        req: health::CheckRequest,
    ) -> Result<health::VersionCheckResponse> {
        info!("version {:?}", req);
        let mut rep = health::VersionCheckResponse::new();
        rep.agent_version = "mock.0.1".to_string();
        rep.grpc_version = "0.0.1".to_string();
        let mut status = Status::new();
        status.set_code(Code::NOT_FOUND);
        Ok(rep)
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
        let mut rp = protobuf::RepeatedField::new();

        let mut i = types::Interface::new();
        i.set_name("first".to_string());
        rp.push(i);
        let mut i = types::Interface::new();
        i.set_name("second".to_string());
        rp.push(i);

        let mut i = agent::Interfaces::new();
        i.set_Interfaces(rp);

        Ok(i)
    }
}

#[tokio::main(core_threads = 1)]
async fn main() {
    simple_logging::log_to_stderr(LevelFilter::Trace);

    let h = Box::new(HealthService {}) as Box<dyn health_ttrpc::Health + Send + Sync>;
    let h = Arc::new(h);
    let hservice = health_ttrpc::create_health(h);

    let a = Box::new(AgentService {}) as Box<dyn agent_ttrpc::AgentService + Send + Sync>;
    let a = Arc::new(a);
    let aservice = agent_ttrpc::create_agent_service(a);

    let server = Server::new()
        .bind("unix:///tmp/1")
        .unwrap()
        .register_service(hservice)
        .register_service(aservice);

    let mut stream = signal(SignalKind::hangup()).unwrap();
    tokio::select! {
        _ = stream.recv() => {
            println!("signal received")
        }
        _ = server.start() => {
            println!("server exit")
        }
    };
}
