// SPDX-License-Identifier: Apache-2.0

#![cfg(all(unix, feature = "sync"))]

use std::collections::HashMap;
use ttrpc::sync::{Client, MethodHandler, Server, TtrpcContext};
use ttrpc::{Code, Error, Request, Result};

struct NeverCalled;

impl MethodHandler for NeverCalled {
    fn handler(&self, _: TtrpcContext, _: Request) -> Result<()> {
        unreachable!("the registered method is never requested by this test")
    }
}

fn assert_unimplemented(client: &Client, service: &str, method: &str) {
    let error = client
        .request(Request {
            service: service.into(),
            method: method.into(),
            timeout_nano: 5_000_000_000,
            ..Default::default()
        })
        .unwrap_err();

    match error {
        Error::RpcStatus(status) => assert_eq!(
            status.code(),
            Code::UNIMPLEMENTED,
            "/{service}/{method} should be reported as unimplemented"
        ),
        other => panic!(
            "expected an RPC status for /{}/{}, got {:?}",
            service, method, other
        ),
    }
}

#[test]
fn unknown_rpcs_return_unimplemented() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rpc.sock");
    let address = format!("unix://{}", path.display());

    let mut methods: HashMap<String, Box<dyn MethodHandler + Send + Sync>> = HashMap::new();
    methods.insert("/test.Echo/Call".to_string(), Box::new(NeverCalled));

    let mut server = Server::new()
        .bind(&address)
        .unwrap()
        .register_service(methods);
    server.start().unwrap();

    let client = Client::connect(&address).unwrap();

    assert_unimplemented(&client, "test.Unknown", "Call");
    assert_unimplemented(&client, "test.Echo", "Missing");
}
