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

use nix::sys::socket::*;
use protocols::sync::{agent, agent_ttrpc, health, health_ttrpc};
use std::collections::HashMap;
use std::thread;
use ttrpc::client::Client;

fn main() {
    let path = "/tmp/1";

    let fd = socket(
        AddressFamily::Unix,
        SockType::Stream,
        SockFlag::empty(),
        None,
    )
    .unwrap();
    let sockaddr = path.to_owned() + &"\x00".to_string();
    let sockaddr = UnixAddr::new_abstract(sockaddr.as_bytes()).unwrap();
    let sockaddr = SockAddr::Unix(sockaddr);
    connect(fd, &sockaddr).unwrap();

    let c = Client::new(fd);
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
            thc.check(&req, default_metadata(), 0),
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

        let show = match tac.list_interfaces(
            &agent::ListInterfacesRequest::new(),
            default_metadata(),
            0,
        ) {
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
    let show = match ac.online_cpu_mem(&agent::OnlineCPUMemRequest::new(), None, 0) {
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
        hc.version(&health::CheckRequest::new(), default_metadata(), 0),
        now.elapsed()
    );

    t.join().unwrap();
    t2.join().unwrap();
}

fn default_metadata() -> Option<HashMap<String, Vec<String>>> {
    let mut md: HashMap<String, Vec<String>> = HashMap::new();
    md.insert("key".to_string(), vec!["v1".to_string(), "v2".to_string()]);
    Some(md)
}
