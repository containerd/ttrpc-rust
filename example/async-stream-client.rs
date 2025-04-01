// Copyright 2022 Alibaba Cloud. All rights reserved.
//
// SPDX-License-Identifier: Apache-2.0
//

mod protocols;
mod utils;
use protocols::asynchronous::{empty, streaming, streaming_ttrpc};
use ttrpc::context::{self, Context};
use ttrpc::r#async::Client;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    simple_logging::log_to_stderr(log::LevelFilter::Info);

    let c = Client::connect(utils::SOCK_ADDR).await.unwrap();
    let sc = streaming_ttrpc::StreamingClient::new(c);

    let _now = std::time::Instant::now();

    let sc1 = sc.clone();
    let t1 = tokio::spawn(echo_request(sc1));

    let sc1 = sc.clone();
    let t2 = tokio::spawn(echo_stream(sc1));

    let sc1 = sc.clone();
    let t3 = tokio::spawn(sum_stream(sc1));

    let sc1 = sc.clone();
    let t4 = tokio::spawn(divide_stream(sc1));

    let sc1 = sc.clone();
    let t5 = tokio::spawn(echo_null(sc1));

    let sc1 = sc.clone();
    let t6 = tokio::spawn(echo_null_stream(sc1));

    let sc1 = sc.clone();
    let t7 = tokio::spawn(echo_default_value(sc1));

    let t8 = tokio::spawn(server_send_stream(sc));

    let (r1, r2, r3, r4, r5, r6, r7, r8) = tokio::join!(t1, t2, t3, t4, t5, t6, t7, t8);

    assert!(
        r1.is_ok()
            && r2.is_ok()
            && r3.is_ok()
            && r4.is_ok()
            && r5.is_ok()
            && r6.is_ok()
            && r7.is_ok()
            && r8.is_ok(),
        "async-stream test is failed because some error occurred"
    );

    println!("***** Async Stream test is OK! *****");
}

fn default_ctx() -> Context {
    let mut ctx = context::with_timeout(0);
    ctx.add("key-1".to_string(), "value-1-1".to_string());
    ctx.add("key-1".to_string(), "value-1-2".to_string());
    ctx.set("key-2".to_string(), vec!["value-2".to_string()]);

    ctx
}

async fn echo_request(cli: streaming_ttrpc::StreamingClient) {
    let echo1 = streaming::EchoPayload {
        seq: 1,
        msg: "Echo Me".to_string(),
        ..Default::default()
    };
    let resp = cli.echo(default_ctx(), &echo1).await.unwrap();
    assert_eq!(resp.msg, echo1.msg);
    assert_eq!(resp.seq, echo1.seq + 1);
}

async fn echo_stream(cli: streaming_ttrpc::StreamingClient) {
    let mut stream = cli.echo_stream(default_ctx()).await.unwrap();

    let mut i = 0;
    while i < 100 {
        let echo = streaming::EchoPayload {
            seq: i as u32,
            msg: format!("{}: Echo in a stream", i),
            ..Default::default()
        };
        stream.send(&echo).await.unwrap();
        let resp = stream.recv().await.unwrap();
        assert_eq!(resp.msg, echo.msg);
        assert_eq!(resp.seq, echo.seq + 1);

        i += 2;
    }
    stream.close_send().await.unwrap();
    let ret = stream.recv().await;
    assert!(matches!(ret, Err(ttrpc::Error::Eof)));
}

async fn sum_stream(cli: streaming_ttrpc::StreamingClient) {
    let mut stream = cli.sum_stream(default_ctx()).await.unwrap();

    let mut sum = streaming::Sum::new();
    stream.send(&streaming::Part::new()).await.unwrap();

    sum.num += 1;
    let mut i = -99i32;
    while i <= 100 {
        let addi = streaming::Part {
            add: i,
            ..Default::default()
        };
        stream.send(&addi).await.unwrap();
        sum.sum += i;
        sum.num += 1;

        i += 1;
    }
    stream.send(&streaming::Part::new()).await.unwrap();
    sum.num += 1;

    let ssum = stream.close_and_recv().await.unwrap();
    assert_eq!(ssum.sum, sum.sum);
    assert_eq!(ssum.num, sum.num);
}

async fn divide_stream(cli: streaming_ttrpc::StreamingClient) {
    let expected = streaming::Sum {
        sum: 392,
        num: 4,
        ..Default::default()
    };
    let mut stream = cli.divide_stream(default_ctx(), &expected).await.unwrap();

    let mut actual = streaming::Sum::new();

    // NOTE: `for part in stream.recv().await.unwrap()` can't work.
    while let Some(part) = stream.recv().await.unwrap() {
        actual.sum += part.add;
        actual.num += 1;
    }
    assert_eq!(actual.sum, expected.sum);
    assert_eq!(actual.num, expected.num);
}

async fn echo_null(cli: streaming_ttrpc::StreamingClient) {
    let mut stream = cli.echo_null(default_ctx()).await.unwrap();

    for i in 0..100 {
        let echo = streaming::EchoPayload {
            seq: i as u32,
            msg: "non-empty empty".to_string(),
            ..Default::default()
        };
        stream.send(&echo).await.unwrap();
    }
    let res = stream.close_and_recv().await.unwrap();
    assert_eq!(res, empty::Empty::new());
}

async fn echo_null_stream(cli: streaming_ttrpc::StreamingClient) {
    let stream = cli.echo_null_stream(default_ctx()).await.unwrap();

    let (tx, mut rx) = stream.split();

    let task = tokio::spawn(async move {
        loop {
            let ret = rx.recv().await;
            if matches!(ret, Err(ttrpc::Error::Eof)) {
                break;
            }
        }
    });

    for i in 0..100 {
        let echo = streaming::EchoPayload {
            seq: i as u32,
            msg: "non-empty empty".to_string(),
            ..Default::default()
        };
        tx.send(&echo).await.unwrap();
    }

    tx.close_send().await.unwrap();

    tokio::time::timeout(tokio::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
}

async fn echo_default_value(cli: streaming_ttrpc::StreamingClient) {
    let mut stream = cli
        .echo_default_value(default_ctx(), &Default::default()) // send default value to verify #208
        .await
        .unwrap();

    let received = stream.recv().await.unwrap().unwrap();

    assert_eq!(received.seq, 0);
    assert_eq!(received.msg, "");
}

async fn server_send_stream(cli: streaming_ttrpc::StreamingClient) {
    let mut stream = cli
        .server_send_stream(default_ctx(), &Default::default())
        .await
        .unwrap();

    let mut seq = 0;
    while let Some(received) = stream.recv().await.unwrap() {
        assert_eq!(received.seq, seq);
        assert_eq!(received.msg, "hello");
        seq += 1;
    }
}
