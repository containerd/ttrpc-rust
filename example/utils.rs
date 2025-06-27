#![allow(dead_code)]
use clap::Parser;
use log::warn;
use std::io::Result;

#[cfg(unix)]
pub const SOCK_ADDR_LOCAL: &str = r"unix:///tmp/ttrpc-test";

#[cfg(windows)]
pub const SOCK_ADDR_LOCAL: &str = r"\\.\pipe\ttrpc-test";

pub const SOCK_ADDR_TCP: &str = r"tcp://127.0.0.1:65500";

#[derive(Debug, Default, Parser)]
pub struct Cli {
    #[arg(long = "tcp")]
    #[arg(help = "Use a TCP socket instead of a local one")]
    pub tcp: bool,
}

pub fn get_sock_addr() -> &'static str {
    let cli = Cli::parse();
    if cli.tcp {
        if cfg!(windows) {
            warn!("'--tcp' flag ignored; TCP sockets not supported on Windows");
            return SOCK_ADDR_LOCAL;
        }
        SOCK_ADDR_TCP
    } else {
        SOCK_ADDR_LOCAL
    }
}

#[cfg(unix)]
pub fn remove_if_sock_exist(sock_addr: &str) -> Result<()> {
    if sock_addr.starts_with("tcp://") {
        return Ok(());
    }

    let path = sock_addr
        .strip_prefix("unix://")
        .expect("socket address is not expected");

    if std::path::Path::new(path).exists() {
        std::fs::remove_file(path)?;
    }

    Ok(())
}

#[cfg(windows)]
pub fn remove_if_sock_exist(_sock_addr: &str) -> Result<()> {
    //todo force close file handle?

    Ok(())
}

pub mod resp {
    pub fn online_cpu_mem_not_impl() -> ttrpc::Error {
        let mut status = ttrpc::Status::new();
        status.set_code(ttrpc::Code::NOT_FOUND);
        status.set_message("/grpc.AgentService/OnlineCPUMem is not supported".to_string());

        ttrpc::Error::RpcStatus(status)
    }
    pub mod sync {
        use crate::protocols::sync::{
            agent::Interfaces, health::VersionCheckResponse, types::Interface,
        };

        pub fn health_version() -> ttrpc::Result<VersionCheckResponse> {
            Ok(VersionCheckResponse {
                grpc_version: "0.0.1".into(),
                agent_version: "mock.0.1".into(),
                ..Default::default()
            })
        }

        pub fn agent_list_interfaces() -> ttrpc::Result<Interfaces> {
            Ok(Interfaces {
                Interfaces: vec![
                    Interface {
                        name: "first".into(),
                        ..Default::default()
                    },
                    Interface {
                        name: "second".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            })
        }
    }

    pub mod asynchronous {
        use crate::protocols::asynchronous::{
            agent::Interfaces, health::VersionCheckResponse, types::Interface,
        };

        pub fn health_version() -> ttrpc::Result<VersionCheckResponse> {
            Ok(VersionCheckResponse {
                grpc_version: "0.0.1".into(),
                agent_version: "mock.0.1".into(),
                ..Default::default()
            })
        }

        pub fn agent_list_interfaces() -> ttrpc::Result<Interfaces> {
            Ok(Interfaces {
                Interfaces: vec![
                    Interface {
                        name: "first".into(),
                        ..Default::default()
                    },
                    Interface {
                        name: "second".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            })
        }
    }
}

pub async fn hangup() {
    #[cfg(unix)]
    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
        .unwrap()
        .recv()
        .await
        .unwrap();

    #[cfg(not(unix))]
    std::future::pending::<()>().await;
}

pub async fn interrupt() {
    tokio::signal::ctrl_c().await.unwrap();
}
