// Copyright (c) 2020 Ant Financial
//
// SPDX-License-Identifier: Apache-2.0
//

use protoc_rust_ttrpc::Customize;
use std::fs::File;
use std::io::{Read, Write};

fn main() {
    let protos = vec![
        "protocols/protos/github.com/kata-containers/agent/pkg/types/types.proto",
        "protocols/protos/agent.proto",
        "protocols/protos/health.proto",
        "protocols/protos/google/protobuf/empty.proto",
        "protocols/protos/oci.proto",
    ];

    // Tell Cargo that if the .proto files changed, to rerun this build script.
    protos
        .iter()
        .for_each(|p| println!("cargo:rerun-if-changed={}", &p));

    protoc_rust_ttrpc::Codegen::new()
        .out_dir("protocols")
        .inputs(&protos)
        .include("protocols/protos")
        .rust_protobuf()
        .customize(Customize {
            async_server: true,
            ..Default::default()
        })
        .run()
        .expect("Codegen failed.");

    // There is a message named 'Box' in oci.proto
    // so there is a struct named 'Box', we should replace Box<Self> to ::std::boxed::Box<Self>
    // to avoid the conflict.
    replace_text_in_file(
        "protocols/oci.rs",
        "self: Box<Self>",
        "self: ::std::boxed::Box<Self>",
    )
    .unwrap();
}

fn replace_text_in_file(file_name: &str, from: &str, to: &str) -> Result<(), std::io::Error> {
    let mut src = File::open(file_name)?;
    let mut contents = String::new();
    src.read_to_string(&mut contents).unwrap();
    drop(src);

    let new_contents = contents.replace(from, to);

    let mut dst = File::create(&file_name)?;
    dst.write(new_contents.as_bytes())?;

    Ok(())
}
