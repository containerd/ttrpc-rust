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
use std::time::Duration;
use ttrpc::context::{self, Context};
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
    let expected_fd_count = get_fd_count();
    connect_once();
    // Give some time for fd to be released in the other thread
    thread::sleep(Duration::from_secs(1));
    let current_fd_count = get_fd_count();
    assert_eq!(current_fd_count, expected_fd_count, "check fd count");

    println!("***** Sync test is OK! *****");
}

fn connect_once() {
    simple_logging::log_to_stderr(LevelFilter::Trace);

    let sock_addr = utils::get_sock_addr();
    let c = Client::connect(sock_addr).unwrap();

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

        let resp = thc.check(
            context::with_duration(core::time::Duration::from_millis(20)),
            &req,
        );

        assert_eq!(
            resp,
            Err(ttrpc::Error::Others(
                "Receive packet from Receiver timeout: timed out waiting on channel".into()
            ))
        );

        println!(
            "OS Thread {:?} - health.check() -> {:?} ended: {:?}",
            std::thread::current().id(),
            resp,
            now.elapsed(),
        );
    });

    let t2 = thread::spawn(move || {
        println!(
            "OS Thread {:?} - agent.list_interfaces() started: {:?}",
            std::thread::current().id(),
            now.elapsed(),
        );

        let resp = tac.list_interfaces(default_ctx(), &agent::ListInterfacesRequest::new());
        let expected_resp = utils::resp::sync::agent_list_interfaces();
        assert_eq!(resp, expected_resp);

        println!(
            "OS Thread {:?} - agent.list_interfaces() -> {} ended: {:?}",
            std::thread::current().id(),
            "{resp:?}",
            now.elapsed(),
        );
    });

    println!(
        "Main OS Thread - agent.online_cpu_mem() started: {:?}",
        now.elapsed()
    );
    let resp = ac
        .online_cpu_mem(default_ctx(), &agent::OnlineCPUMemRequest::new())
        .expect_err("not the expecting error from the example server");
    let expected_resp = utils::resp::online_cpu_mem_not_impl();
    assert_eq!(resp, expected_resp);

    println!(
        "Main OS Thread - agent.online_cpu_mem() -> {:?} ended: {:?}",
        resp,
        now.elapsed()
    );

    let version = hc.version(default_ctx(), &health::CheckRequest::new());
    let expected_version_resp = utils::resp::sync::health_version();
    assert_eq!(version, expected_version_resp);

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
