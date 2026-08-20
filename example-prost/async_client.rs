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
        let resp = thc
            .check(
                context::with_duration(core::time::Duration::from_millis(20)),
                &req,
            )
            .await;

        assert_eq!(
            resp,
            Err(ttrpc::Error::Others(
                "Receive packet timeout Elapsed(())".into()
            ))
        );
        println!(
            "Green Thread 1 - {} -> {:?} ended: {:?}",
            "health.check()",
            resp,
            now.elapsed(),
        );
    });

    let t2 = tokio::spawn(async move {
        println!(
            "Green Thread 2 - {} started: {:?}",
            "agent.list_interfaces()",
            now.elapsed(),
        );

        let resp = tac
            .list_interfaces(default_ctx(), &agent::ListInterfacesRequest::default())
            .await;
        let expected_resp = utils::resp::async_agent_list_interfaces();
        assert_eq!(resp, expected_resp);

        println!(
            "Green Thread 2 - {} -> {:?} ended: {:?}",
            "agent.list_interfaces()",
            resp,
            now.elapsed(),
        );
    });

    let t3 = tokio::spawn(async move {
        println!(
            "Green Thread 3 - {} started: {:?}",
            "agent.online_cpu_mem()",
            now.elapsed()
        );

        let resp = ac
            .online_cpu_mem(default_ctx(), &agent::OnlineCpuMemRequest::default())
            .await
            .expect_err("not the expecting error from the example server");
        let expected_resp = utils::resp::online_cpu_mem_not_impl();
        assert_eq!(resp, expected_resp);
        println!(
            "Green Thread 3 - {} -> {:?} ended: {:?}",
            "agent.online_cpu_mem()",
            resp,
            now.elapsed()
        );

        println!(
            "Green Thread 3 - {} started: {:?}",
            "health.version()",
            now.elapsed()
        );
        let resp = hc
            .version(default_ctx(), &health::CheckRequest::default())
            .await;
        let expected_resp = utils::resp::async_health_version();
        assert_eq!(resp, expected_resp);
        println!(
            "Green Thread 3 - {} -> {:?} ended: {:?}",
            "health.version()",
            resp,
            now.elapsed()
        );
    });

    let (r1, r2, r3) = tokio::join!(t1, t2, t3);
    assert!(
        r1.is_ok() && r2.is_ok() && r3.is_ok(),
        "async test is failed because some error occurred"
    );

    println!("***** Async test is OK! *****");
}

fn default_ctx() -> Context {
    let mut ctx = context::with_timeout(0);
    ctx.add("key-1".to_string(), "value-1-1".to_string());
    ctx.add("key-1".to_string(), "value-1-2".to_string());
    ctx.set("key-2".to_string(), vec!["value-2".to_string()]);

    ctx
}
