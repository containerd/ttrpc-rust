// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

mod protocols;
mod utils;

use protocols::r#async::{agent, health};
use ttrpc::context::{self, Context};
use ttrpc::r#async::Client;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let c = Client::connect(utils::SOCK_ADDR).await.unwrap();
    let hc = health::HealthClient::new(c.clone());
    let ac = agent::AgentServiceClient::new(c);

    let thc = hc.clone();
    let tac = ac.clone();

    let now = std::time::Instant::now();

    let t1 = tokio::spawn(async move {
        let req = health::CheckRequest::default();
        println!(
            "Green Thread 1 - {} started: {:?}",
            "health.check()",
            now.elapsed(),
        );
        println!(
            "Green Thread 1 - {} -> {:?} ended: {:?}",
            "health.check()",
            thc.check(context::with_timeout(20 * 1000 * 1000), &req)
                .await,
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
            .list_interfaces(default_ctx(), &agent::ListInterfacesRequest::default())
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
            .online_cpu_mem(default_ctx(), &agent::OnlineCpuMemRequest::default())
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
            hc.version(default_ctx(), &health::CheckRequest::default())
                .await,
            now.elapsed()
        );
    });

    let _ = tokio::join!(t1, t2, t3);
}

fn default_ctx() -> Context {
    let mut ctx = context::with_timeout(0);
    ctx.add("key-1".to_string(), "value-1-1".to_string());
    ctx.add("key-1".to_string(), "value-1-2".to_string());
    ctx.set("key-2".to_string(), vec!["value-2".to_string()]);

    ctx
}
