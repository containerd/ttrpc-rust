use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let path: PathBuf = [out_dir.clone(), "mod.rs".to_string()].iter().collect();
    fs::write(path, "pub mod ttrpc;").unwrap();

    generate_ttrpc(&out_dir);
}

#[cfg(not(feature = "prost"))]
fn generate_ttrpc(out_dir: &str) {
    let customize = protobuf_codegen::Customize::default()
        .gen_mod_rs(false)
        .generate_accessors(true);

    protobuf_codegen::Codegen::new()
        .pure()
        .out_dir(out_dir)
        .inputs(["src/ttrpc.proto"])
        .include("src")
        .customize(customize)
        .run()
        .expect("Codegen failed.");
}

#[cfg(feature = "prost")]
fn generate_ttrpc(out_dir: &str) {
    let mut config = prost_build::Config::new();
    config
        .out_dir(out_dir)
        .compile_well_known_types()
        .protoc_arg("--experimental_allow_proto3_optional")
        .enum_attribute("Code", "#[allow(non_camel_case_types)]")
        .compile_protos(&["src/ttrpc.proto"], &["src"])
        .expect("Codegen failed");

    // read ttrpc.rs
    let ttrpc_path = format!("{}/ttrpc.rs", out_dir);
    let content = fs::read_to_string(&ttrpc_path).expect("Failed to read ttrpc.rs");

    // define the enum value name pairs
    let replacements = [
        ("Ok", "OK"),
        ("Cancelled", "CANCELLED"),
        ("Unknown", "UNKNOWN"),
        ("InvalidArgument", "INVALID_ARGUMENT"),
        ("DeadlineExceeded", "DEADLINE_EXCEEDED"),
        ("NotFound", "NOT_FOUND"),
        ("AlreadyExists", "ALREADY_EXISTS"),
        ("PermissionDenied", "PERMISSION_DENIED"),
        ("Unauthenticated", "UNAUTHENTICATED"),
        ("ResourceExhausted", "RESOURCE_EXHAUSTED"),
        ("FailedPrecondition", "FAILED_PRECONDITION"),
        ("Aborted", "ABORTED"),
        ("OutOfRange", "OUT_OF_RANGE"),
        ("Unimplemented", "UNIMPLEMENTED"),
        ("Internal", "INTERNAL"),
        ("Unavailable", "UNAVAILABLE"),
        ("DataLoss", "DATA_LOSS"),
    ];

    // replace the enum value in the file
    let mut modified_content = content.clone();

    // replace the enum definition
    for (pascal_case, upper_case) in &replacements {
        // replace the enum definition line
        let enum_pattern = format!("    {} = ", pascal_case);
        let enum_replacement = format!("    {} = ", upper_case);
        modified_content = modified_content.replace(&enum_pattern, &enum_replacement);

        // replace the as_str_name function
        let match_pattern = format!("            Self::{} => ", pascal_case);
        let match_replacement = format!("            Self::{} => ", upper_case);
        modified_content = modified_content.replace(&match_pattern, &match_replacement);

        // replace the from_str_name function
        let from_str_pattern = format!(
            "            \"{}\" => Some(Self::{})",
            upper_case, pascal_case
        );
        let from_str_replacement = format!(
            "            \"{}\" => Some(Self::{})",
            upper_case, upper_case
        );
        modified_content = modified_content.replace(&from_str_pattern, &from_str_replacement);
    }

    // write the modified content back to the file
    fs::write(&ttrpc_path, modified_content).expect("Failed to write modified ttrpc.rs");
}
