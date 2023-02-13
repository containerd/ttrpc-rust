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
    prost_build::Config::new()
        .out_dir(out_dir)
        .compile_well_known_types()
        .protoc_arg("--experimental_allow_proto3_optional")
        .compile_protos(&["src/ttrpc.proto"], &["src"])
        .expect("Codegen failed")
}
