// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

mod protocols;

use nix::sys::socket::*;
use tokio;
use ttrpc::r#async::Client;

#[tokio::main(core_threads = 1)]
async fn main() {
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
    let mut hc = protocols::health_ttrpc::HealthClient::new(c.clone());
    let mut ac = protocols::agent_ttrpc::AgentServiceClient::new(c);

    let mut thc = hc.clone();
    let mut tac = ac.clone();

    let now = std::time::Instant::now();

    let t1 = tokio::spawn(async move {
        let req = protocols::health::CheckRequest::new();
        println!(
            "Green Thread 1 - {} started: {:?}",
            "health.check()",
            now.elapsed(),
        );
        println!(
            "Green Thread 1 - {} -> {:?} ended: {:?}",
            "health.check()",
            thc.check(&req, 0).await,
            now.elapsed(),
        );
    });

    let t2 = tokio::spawn(async move {
        println!(
            "Green Thread 2 - {} started: {:?}",
            "agent.list_interfaces()",
            now.elapsed(),
        );

        let show = match tac
            .list_interfaces(&protocols::agent::ListInterfacesRequest::new(), 0)
            .await
        {
            Err(e) => format!("{:?}", e),
            Ok(s) => format!("{:?}", s),
        };

        println!(
            "Green Thread 2 - {} -> {} ended: {:?}",
            "agent.list_interfaces()",
            show,
            now.elapsed(),
        );
    });

    let t3 = tokio::spawn(async move {
        println!(
            "Green Thread 3 - {} started: {:?}",
            "agent.online_cpu_mem()",
            now.elapsed()
        );

        let show = match ac
            .online_cpu_mem(&protocols::agent::OnlineCPUMemRequest::new(), 0)
            .await
        {
            Err(e) => format!("{:?}", e),
            Ok(s) => format!("{:?}", s),
        };
        println!(
            "Green Thread 3 - {} -> {} ended: {:?}",
            "agent.online_cpu_mem()",
            show,
            now.elapsed()
        );

        println!(
            "Green Thread 3 - {} started: {:?}",
            "health.version()",
            now.elapsed()
        );
        println!(
            "Green Thread 3 - {} -> {:?} ended: {:?}",
            "health.version()",
            hc.version(&protocols::health::CheckRequest::new(), 0).await,
            now.elapsed()
        );
    });

    let _ = tokio::join!(t1, t2, t3);
}
