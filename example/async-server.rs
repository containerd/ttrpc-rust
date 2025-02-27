// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

mod protocols;
mod utils;

use std::sync::Arc;

use log::LevelFilter;

#[cfg(unix)]
use protocols::asynchronous::{agent, agent_ttrpc, health, health_ttrpc};
#[cfg(unix)]
use ttrpc::asynchronous::Server;

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
#[cfg(unix)]
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

#[cfg(windows)]
fn main() {
    println!("This example only works on Unix-like OSes");
}

#[cfg(unix)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    pub fn is_socket_in_use(sock_path: &str) -> bool {
        use std::process::Command;

        let output = Command::new("lsof")
            .arg(sock_path)
            .output()
            .expect("Failed to execute lsof command");

        output.status.success()
    }

    #[cfg(unix)]
    #[tokio::test]
    // Add this test for test thread leak, if the caller forget to call the shutdown function.
    async fn test_server_start() {
        simple_logging::log_to_stderr(LevelFilter::Trace);
        {
            let hservice = health_ttrpc::create_health(Arc::new(HealthService {}));
            utils::remove_if_sock_exist(utils::SOCK_ADDR).unwrap();
            let mut server = Server::new()
                .bind(utils::SOCK_ADDR)
                .unwrap()
                .register_service(hservice);
            server.start().await.unwrap();
        }
        sleep(std::time::Duration::from_secs(1)).await;
        // judge utils::SOCK_ADDR if still occupied
        let addr = utils::SOCK_ADDR
            .strip_prefix("unix://")
            .expect("socket address is not expected");
        // It should be true, because the server's thread is not stopped.
        assert!(is_socket_in_use(addr));
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn test_server_serve() {
        simple_logging::log_to_stderr(LevelFilter::Trace);

        let hservice = health_ttrpc::create_health(Arc::new(HealthService {}));
        let _ = utils::remove_if_sock_exist(utils::SOCK_ADDR);
        let mut server = Server::new()
            .bind(utils::SOCK_ADDR)
            .unwrap()
            .register_service(hservice);

        let (tx, rx) = tokio::sync::oneshot::channel();
        // Move the server to a new task
        let server_handle = tokio::spawn(async move {
            tokio::select! {
                _ = rx => {
                    // Receive the stop signal, and execute the graceful shutdown
                    server.shutdown().await.unwrap();
                }
                result = server.serve() => {
                    result.unwrap();
                }
            }
        });

        // Wait for the server to start
        sleep(std::time::Duration::from_secs(1)).await;
        // Send the stop signal
        tx.send(()).unwrap();

        server_handle.await.unwrap();
        let addr = utils::SOCK_ADDR
            .strip_prefix("unix://")
            .expect("socket address is not expected");
        // It should be false, because the server is stopped.
        assert!(!is_socket_in_use(addr));
    }
}
