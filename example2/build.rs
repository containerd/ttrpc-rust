// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

// use ttrpc_codegen::CodegenBuilder;

use ttrpc_codegen::{AsyncMode, CodegenBuilder};

fn main() {
    let mut protos = vec![
        "protocols/protos/health.proto",
        "protocols/protos/agent.proto",
        "protocols/protos/oci.proto",
    ];

    let includes = vec!["protocols/protos"];

    let codegen = CodegenBuilder::new()
        .set_out_dir(&"protocols/sync")
        .set_protos(&protos)
        .set_includes(&includes)
        .set_serde(true)
        .set_async_mode(AsyncMode::None)
        .set_generate_service(true)
        .build()
        .unwrap();
    codegen.generate().unwrap();

    protos.push("protocols/protos/streaming.proto");

    let codegen = CodegenBuilder::new()
        .set_out_dir(&"protocols/asynchronous")
        .set_protos(&protos)
        .set_includes(&includes)
        .set_serde(true)
        .set_async_mode(AsyncMode::All)
        .set_generate_service(true)
        .build()
        .unwrap();
    codegen.generate().unwrap();
}
