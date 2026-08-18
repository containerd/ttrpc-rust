#![allow(dead_code)]
use std::fs;
use std::io::Result;
use std::path::Path;

pub const SOCK_ADDR: &str = "unix:///tmp/ttrpc-test";

pub fn remove_if_sock_exist(sock_addr: &str) -> Result<()> {
    let path = sock_addr
        .strip_prefix("unix://")
        .expect("socket address is not expected");

    if Path::new(path).exists() {
        fs::remove_file(&path)?;
    }

    Ok(())
}

pub mod resp {
    use crate::protocols as p;

    fn not_implemented_status(path: &str) -> ttrpc::Status {
        ttrpc::Status {
            code: ttrpc::Code::NOT_FOUND as i32,
            message: format!("{path} is not supported"),
            ..Default::default()
        }
    }

    pub fn online_cpu_mem_not_impl() -> ttrpc::Error {
        ttrpc::Error::RpcStatus(not_implemented_status(
            "/grpc.AgentService/OnlineCPUMem",
        ))
    }

    pub fn sync_agent_list_interfaces() -> ttrpc::Result<p::sync::agent::Interfaces> {
        Ok(p::sync::agent::Interfaces {
            interfaces: vec![
                p::sync::types::Interface {
                    name: "first".to_string(),
                    ..Default::default()
                },
                p::sync::types::Interface {
                    name: "second".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        })
    }

    pub fn sync_health_version() -> ttrpc::Result<p::sync::health::VersionCheckResponse> {
        Ok(p::sync::health::VersionCheckResponse {
            grpc_version: "0.0.1".to_string(),
            agent_version: "mock 0.1".to_string(),
            ..Default::default()
        })
    }

    pub fn async_agent_list_interfaces() -> ttrpc::Result<p::r#async::agent::Interfaces> {
        Ok(p::r#async::agent::Interfaces {
            interfaces: vec![
                p::r#async::types::Interface {
                    name: "first".to_string(),
                    ..Default::default()
                },
                p::r#async::types::Interface {
                    name: "second".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        })
    }

    pub fn async_health_version() -> ttrpc::Result<p::r#async::health::VersionCheckResponse> {
        Ok(p::r#async::health::VersionCheckResponse {
            grpc_version: "0.0.1".to_string(),
            agent_version: "mock 0.1".to_string(),
            ..Default::default()
        })
    }
}
