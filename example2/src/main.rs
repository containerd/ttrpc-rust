use std::{
    fs::File,
    io::{Read, Write},
};

use ttrpc_codegen::{AsyncMode, CodegenBuilder};

// fn main() {
//     let protos = vec!["protocols/protos/test.proto"];
//     let includes = vec!["protocols/protos"];

//     let codegen = CodegenBuilder::new()
//         .set_out_dir(&"protocols/sync")
//         .set_protos(&protos)
//         .set_includes(&includes)
//         .set_serde(true)
//         .set_async_mode(AsyncMode::All)
//         .build()
//         .unwrap();

//     codegen.generate().unwrap();
// }

fn main() {
    let mut protos = vec![
        "protocols/protos/github.com/gogo/protobuf/gogoproto/gogo.proto",
        "protocols/protos/github.com/kata-containers/agent/pkg/types/types.proto",
        "protocols/protos/google/protobuf/empty.proto",
        "protocols/protos/oci.proto",
        "protocols/protos/health.proto",
        "protocols/protos/agent.proto",
    ];

    let includes = vec!["protocols/protos"];

    let codegen = CodegenBuilder::new()
        .set_out_dir(&"protocols/sync")
        .set_protos(&protos)
        .set_includes(&includes)
        .set_serde(true)
        .set_async_mode(AsyncMode::None)
        .build()
        .unwrap();

    codegen.generate().unwrap();

    // Only async support stream currently.
    protos.push("protocols/protos/streaming.proto");

    let codegen = CodegenBuilder::new()
        .set_out_dir(&"protocols/asynchronous")
        .set_protos(&protos)
        .set_includes(&includes)
        .set_serde(true)
        .set_async_mode(AsyncMode::All)
        .build()
        .unwrap();
    codegen.generate().unwrap();

    // // There is a message named 'Box' in oci.proto
    // // so there is a struct named 'Box', we should replace Box<Self> to ::std::boxed::Box<Self>
    // // to avoid the conflict.
    // replace_text_in_file(
    //     "protocols/sync/oci.rs",
    //     "self: Box<Self>",
    //     "self: ::std::boxed::Box<Self>",
    // )
    // .unwrap();

    // replace_text_in_file(
    //     "protocols/asynchronous/oci.rs",
    //     "self: Box<Self>",
    //     "self: ::std::boxed::Box<Self>",
    // )
    // .unwrap();
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
