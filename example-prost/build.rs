// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use ttrpc_codegen::{Codegen, Customize};

// The schemas are shared with the rust-protobuf based `example` crate; this
// crate only provides the Prost-generated Rust bindings.
const PROTOS_DIR: &str = "../example/protocols/protos";

fn main() {
    let protos = [
        format!("{PROTOS_DIR}/health.proto"),
        format!("{PROTOS_DIR}/agent.proto"),
        format!("{PROTOS_DIR}/oci.proto"),
    ];

    Codegen::new()
        .out_dir("protocols/sync")
        .inputs(&protos)
        .include(PROTOS_DIR)
        .prost()
        .customize(Customize::default())
        .run()
        .unwrap();

    let mut async_protos = protos.to_vec();
    async_protos.push(format!("{PROTOS_DIR}/streaming.proto"));

    Codegen::new()
        .out_dir("protocols/asynchronous")
        .inputs(&async_protos)
        .include(PROTOS_DIR)
        .prost()
        .customize(Customize {
            async_all: true,
            ..Default::default()
        })
        .run()
        .unwrap();
}
