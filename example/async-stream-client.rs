// Copyright 2022 Alibaba Cloud. All rights reserved.
//
// SPDX-License-Identifier: Apache-2.0
//

mod protocols;
mod utils;
#[cfg(unix)]
use protocols::r#async::{empty, streaming, streaming_ttrpc};
use ttrpc::context::{self, Context};
#[cfg(unix)]
use ttrpc::r#async::Client;

#[cfg(windows)]
fn main() {
    println!("This example only works on Unix-like OSes");
}

#[cfg(unix)]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    simple_logging::log_to_stderr(log::LevelFilter::Info);

    let c = Client::connect(utils::SOCK_ADDR).unwrap();
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

    let t6 = tokio::spawn(echo_null_stream(sc));

    let _ = tokio::join!(t1, t2, t3, t4, t5, t6);
}

fn default_ctx() -> Context {
    let mut ctx = context::with_timeout(0);
    ctx.add("key-1".to_string(), "value-1-1".to_string());
    ctx.add("key-1".to_string(), "value-1-2".to_string());
    ctx.set("key-2".to_string(), vec!["value-2".to_string()]);

    ctx
}

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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
