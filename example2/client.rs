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

use protocols::sync::{agent, agent_ttrpc, health, health_ttrpc};
use std::thread;
use ttrpc::context::{self, Context};
use ttrpc::Client;

fn main() {
    let c = Client::connect(utils::SOCK_ADDR).unwrap();
    let hc = health_ttrpc::HealthClient::new(c.clone());
    let ac = agent_ttrpc::AgentServiceClient::new(c);

    let thc = hc.clone();
    let tac = ac.clone();

    let now = std::time::Instant::now();

    let t = thread::spawn(move || {
        let req = health::CheckRequest::new();
        println!(
            "OS Thread {:?} - {} started: {:?}",
            std::thread::current().id(),
            "health.check()",
            now.elapsed(),
        );
        println!(
            "OS Thread {:?} - {} -> {:?} ended: {:?}",
            std::thread::current().id(),
            "health.check()",
            thc.check(default_ctx(), &req),
            now.elapsed(),
        );
    });

    let t2 = thread::spawn(move || {
        println!(
            "OS Thread {:?} - {} started: {:?}",
            std::thread::current().id(),
            "agent.list_interfaces()",
            now.elapsed(),
        );

        let show = match tac.list_interfaces(default_ctx(), &agent::ListInterfacesRequest::new()) {
            Err(e) => format!("{:?}", e),
            Ok(s) => format!("{:?}", s),
        };

        println!(
            "OS Thread {:?} - {} -> {} ended: {:?}",
            std::thread::current().id(),
            "agent.list_interfaces()",
            show,
            now.elapsed(),
        );
    });

    println!(
        "Main OS Thread - {} started: {:?}",
        "agent.online_cpu_mem()",
        now.elapsed()
    );
    let show = match ac.online_cpu_mem(default_ctx(), &agent::OnlineCPUMemRequest::new()) {
        Err(e) => format!("{:?}", e),
        Ok(s) => format!("{:?}", s),
    };
    println!(
        "Main OS Thread - {} -> {} ended: {:?}",
        "agent.online_cpu_mem()",
        show,
        now.elapsed()
    );

    println!("\nsleep 2 seconds ...\n");
    thread::sleep(std::time::Duration::from_secs(2));
    println!(
        "Main OS Thread - {} started: {:?}",
        "health.version()",
        now.elapsed()
    );
    println!(
        "Main OS Thread - {} -> {:?} ended: {:?}",
        "health.version()",
        hc.version(default_ctx(), &health::CheckRequest::new()),
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
