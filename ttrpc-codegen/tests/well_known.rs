use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use ttrpc_codegen::{parse_and_typecheck, Codegen, Customize};

fn proto_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/protos")
}

fn proto_input() -> PathBuf {
    proto_dir().join("well_known.proto")
}

#[test]
fn parses_embedded_protobuf_types() {
    let include = proto_dir();
    let input = proto_input();
    let parsed = parse_and_typecheck(&[include.as_path()], &[input.as_path()]).unwrap();

    assert_eq!(vec!["well_known.proto"], parsed.relative_paths);
    let descriptor_names: HashSet<_> = parsed
        .file_descriptors
        .iter()
        .map(|descriptor| descriptor.name())
        .collect();
    assert_eq!(
        HashSet::from([
            "well_known.proto",
            "google/protobuf/descriptor.proto",
            "google/protobuf/timestamp.proto",
        ]),
        descriptor_names
    );
}

#[test]
fn generates_runtime_paths_for_embedded_types() {
    for async_all in [false, true] {
        let output = TempDir::new().unwrap();
        Codegen::new()
            .out_dir(output.path())
            .input(proto_input())
            .include(proto_dir())
            .rust_protobuf()
            .customize(Customize {
                async_all,
                ..Default::default()
            })
            .run()
            .unwrap();

        let messages = fs::read_to_string(output.path().join("well_known.rs")).unwrap();
        let services = fs::read_to_string(output.path().join("well_known_ttrpc.rs")).unwrap();
        let timestamp = "::protobuf::well_known_types::timestamp::Timestamp";
        let descriptor = "::protobuf::descriptor::FileDescriptorProto";
        let nested_descriptor = "::protobuf::descriptor::descriptor_proto::ExtensionRange";

        assert!(messages.contains(timestamp));
        assert!(services.contains(timestamp));
        assert!(services.contains(descriptor));
        assert!(services.contains(nested_descriptor));
        assert!(!output.path().join("descriptor.rs").exists());
        assert!(!output.path().join("timestamp.rs").exists());
    }
}

#[test]
fn missing_non_well_known_import_is_an_error() {
    let fixture = TempDir::new().unwrap();
    let input = fixture.path().join("missing.proto");
    fs::write(
        &input,
        r#"syntax = "proto3";
import "example/missing.proto";
message Request {}
"#,
    )
    .unwrap();

    let error = match parse_and_typecheck(&[fixture.path()], &[input.as_path()]) {
        Ok(_) => panic!("missing import unexpectedly succeeded"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("example/missing.proto"));
}
