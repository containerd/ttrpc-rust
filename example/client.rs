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

use log::LevelFilter;
use protocols::sync::{agent, agent_ttrpc, health, health_ttrpc};
use std::thread;
use ttrpc::context::{self, Context};
use ttrpc::error::Error;
use ttrpc::proto::Code;
use ttrpc::Client;

#[cfg(not(target_os = "linux"))]
fn get_fd_count() -> usize {
    // currently not support get fd count
    0
}

#[cfg(target_os = "linux")]
fn get_fd_count() -> usize {
    let path = "/proc/self/fd";
    let count = std::fs::read_dir(path).unwrap().count();
    println!("get fd count {}", count);
    count
}

fn main() {
    connect_once();
    let expected_fd_count = get_fd_count();

    // connect 3 times and check the fd leak.
    for index in 0..3 {
        connect_once();
        let current_fd_count = get_fd_count();
        assert_eq!(
            expected_fd_count, current_fd_count,
            "check fd count in {index}"
        );
    }
}

fn connect_once() {
    simple_logging::log_to_stderr(LevelFilter::Trace);

    let c = Client::connect(utils::SOCK_ADDR).unwrap();
    let hc = health_ttrpc::HealthClient::new(c.clone());
    let ac = agent_ttrpc::AgentServiceClient::new(c);

    let thc = hc.clone();
    let tac = ac.clone();

    let now = std::time::Instant::now();

    let t = thread::spawn(move || {
        let req = health::CheckRequest::new();
        println!(
            "OS Thread {:?} - health.check() started: {:?}",
            std::thread::current().id(),
            now.elapsed(),
        );

        let rsp = thc.check(default_ctx(), &req);
        match rsp.as_ref() {
            Err(Error::RpcStatus(s)) => {
                assert_eq!(Code::NOT_FOUND, s.code());
                assert_eq!("Just for fun".to_string(), s.message())
            }
            Err(e) => {
                panic!("not expecting an error from the example server: {:?}", e)
            }
            Ok(x) => {
                panic!(
                    "not expecting a OK response from the example server: {:?}",
                    x
                )
            }
        }
        println!(
            "OS Thread {:?} - health.check() -> {:?} ended: {:?}",
            std::thread::current().id(),
            rsp,
            now.elapsed(),
        );
    });

    let t2 = thread::spawn(move || {
        println!(
            "OS Thread {:?} - agent.list_interfaces() started: {:?}",
            std::thread::current().id(),
            now.elapsed(),
        );

        let show = match tac.list_interfaces(default_ctx(), &agent::ListInterfacesRequest::new()) {
            Err(e) => {
                panic!("not expecting an error from the example server: {:?}", e)
            }
            Ok(s) => {
                assert_eq!("first".to_string(), s.Interfaces[0].name);
                assert_eq!("second".to_string(), s.Interfaces[1].name);
                format!("{s:?}")
            }
        };

        println!(
            "OS Thread {:?} - agent.list_interfaces() -> {} ended: {:?}",
            std::thread::current().id(),
            show,
            now.elapsed(),
        );
    });

    println!(
        "Main OS Thread - agent.online_cpu_mem() started: {:?}",
        now.elapsed()
    );
    let show = match ac.online_cpu_mem(default_ctx(), &agent::OnlineCPUMemRequest::new()) {
        Err(Error::RpcStatus(s)) => {
            assert_eq!(Code::NOT_FOUND, s.code());
            assert_eq!(
                "/grpc.AgentService/OnlineCPUMem is not supported".to_string(),
                s.message()
            );
            format!("{s:?}")
        }
        Err(e) => {
            panic!("not expecting an error from the example server: {:?}", e)
        }
        Ok(s) => {
            panic!(
                "not expecting a OK response from the example server: {:?}",
                s
            )
        }
    };
    println!(
        "Main OS Thread - agent.online_cpu_mem() -> {} ended: {:?}",
        show,
        now.elapsed()
    );

    println!("\nsleep 2 seconds ...\n");
    thread::sleep(std::time::Duration::from_secs(2));

    let version = hc.version(default_ctx(), &health::CheckRequest::new());
    assert_eq!("mock.0.1", version.as_ref().unwrap().agent_version.as_str());
    assert_eq!("0.0.1", version.as_ref().unwrap().grpc_version.as_str());
    println!(
        "Main OS Thread - health.version() started: {:?}",
        now.elapsed()
    );
    println!(
        "Main OS Thread - health.version() -> {:?} ended: {:?}",
        version,
        now.elapsed()
    );

    t.join().unwrap();
    t2.join().unwrap();
}

fn default_ctx() -> Context {
    let mut ctx = context::with_timeout(0);
    ctx.add("key-1".to_string(), "value-1-1".to_string());
    ctx.add("key-1".to_string(), "value-1-2".to_string());
    ctx.set("key-2".to_string(), vec!["value-2".to_string()]);

    ctx
}
