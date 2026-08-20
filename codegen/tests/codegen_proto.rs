use std::path::PathBuf;
use ttrpc_codegen::{Codegen, Customize};

#[test]
fn generate_from_shared_protos() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../example/protocols/protos");
    let protos = [
        dir.join("health.proto"),
        dir.join("agent.proto"),
        dir.join("oci.proto"),
    ];
    let out = std::env::temp_dir().join("ttrpc-codegen-test/grpc-out");
    std::fs::create_dir_all(&out).unwrap();
    Codegen::new()
        .out_dir(&out)
        .inputs(&protos)
        .include(&dir)
        .prost()
        .customize(Customize::default())
        .run()
        .unwrap();
}

#[test]
fn generate_async_from_shared_protos() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../example/protocols/protos");
    let protos = [
        dir.join("health.proto"),
        dir.join("agent.proto"),
        dir.join("oci.proto"),
        dir.join("streaming.proto"),
    ];
    let out = std::env::temp_dir().join("ttrpc-codegen-test/grpc-async-out");
    std::fs::create_dir_all(&out).unwrap();
    Codegen::new()
        .out_dir(&out)
        .inputs(&protos)
        .include(&dir)
        .prost()
        .customize(Customize {
            async_all: true,
            ..Default::default()
        })
        .run()
        .unwrap();
}

#[test]
fn generate_async_no_streaming() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../example/protocols/protos");
    let protos = [
        dir.join("health.proto"),
        dir.join("agent.proto"),
        dir.join("oci.proto"),
    ];
    let out = std::env::temp_dir().join("ttrpc-codegen-test/grpc-async-nostream");
    std::fs::create_dir_all(&out).unwrap();
    Codegen::new()
        .out_dir(&out)
        .inputs(&protos)
        .include(&dir)
        .prost()
        .customize(Customize {
            async_all: true,
            ..Default::default()
        })
        .run()
        .unwrap();
}
