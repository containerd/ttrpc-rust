// Copyright (c) 2019 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

mod protocols;
mod utils;

use log::LevelFilter;
use protocols::sync::{agent, health};
use std::thread;
use ttrpc::context::{self, Context};
use ttrpc::Client;

fn main() {
    simple_logging::log_to_stderr(LevelFilter::Trace);

    let c = Client::connect(utils::SOCK_ADDR).unwrap();
    let hc = health::HealthClient::new(c.clone());
    let ac = agent::AgentServiceClient::new(c);

    let thc = hc.clone();
    let tac = ac.clone();

    let now = std::time::Instant::now();

    let t = thread::spawn(move || {
        let req = health::CheckRequest::default();
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

        let resp = tac.list_interfaces(default_ctx(), &agent::ListInterfacesRequest::default());
        let expected_resp = utils::resp::sync_agent_list_interfaces();
        assert_eq!(resp, expected_resp);

        println!(
            "OS Thread {:?} - agent.list_interfaces() -> {:?} ended: {:?}",
            std::thread::current().id(),
            resp,
            now.elapsed(),
        );
    });

    println!(
        "Main OS Thread - agent.online_cpu_mem() started: {:?}",
        now.elapsed()
    );
    let resp = ac
        .online_cpu_mem(default_ctx(), &agent::OnlineCpuMemRequest::default())
        .expect_err("not the expecting error from the example server");
    let expected_resp = utils::resp::online_cpu_mem_not_impl();
    assert_eq!(resp, expected_resp);

    println!(
        "Main OS Thread - agent.online_cpu_mem() -> {:?} ended: {:?}",
        resp,
        now.elapsed()
    );

    let version = hc.version(default_ctx(), &health::CheckRequest::default());
    let expected_version_resp = utils::resp::sync_health_version();
    assert_eq!(version, expected_version_resp);

    println!(
        "Main OS Thread - health.version() -> {:?} ended: {:?}",
        version,
        now.elapsed()
    );

    t.join().unwrap();
    t2.join().unwrap();

    println!("***** Sync test is OK! *****");
}

fn default_ctx() -> Context {
    let mut ctx = context::with_timeout(0);
    ctx.add("key-1".to_string(), "value-1-1".to_string());
    ctx.add("key-1".to_string(), "value-1-2".to_string());
    ctx.set("key-2".to_string(), vec!["value-2".to_string()]);

    ctx
}
