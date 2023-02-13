# Ttrpc-rust Codegen

## Getting started

Please ensure that the protoc has been installed on your local environment. Then
write the following code into "build.rs".

```rust
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
```

Add ttrpc-codegen to "build-dependencies" section in "Cargo.toml".

```toml
[build-dependencies]
ttrpc-codegen = "1.0"
```
