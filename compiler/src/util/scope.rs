//! This module contains functionalities that where previously available in
//! the protobuf / protobuf-codegen crates, but were then removed.
//! The missing functionalities have been reimplemented in this module.

use std::fmt;

use protobuf::descriptor::{DescriptorProto, FileDescriptorProto};

use super::to_snake_case;

const DESCRIPTOR_PROTO_FILE: &str = "google/protobuf/descriptor.proto";

const WELL_KNOWN_TYPE_PROTO_FILES: &[&str] = &[
    "google/protobuf/any.proto",
    "google/protobuf/api.proto",
    "google/protobuf/duration.proto",
    "google/protobuf/empty.proto",
    "google/protobuf/field_mask.proto",
    "google/protobuf/source_context.proto",
    "google/protobuf/struct.proto",
    "google/protobuf/timestamp.proto",
    "google/protobuf/type.proto",
    "google/protobuf/wrappers.proto",
];

// vendored from https://github.com/stepancheg/rust-protobuf/blob/v3.7.2/protobuf-codegen/src/gen/rust/keywords.rs
fn is_rust_keyword(ident: &str) -> bool {
    #[rustfmt::skip]
    static RUST_KEYWORDS: &[&str] = &[
        "_",
        "as",
        "async",
        "await",
        "break",
        "crate",
        "dyn",
        "else",
        "enum",
        "extern",
        "false",
        "fn",
        "for",
        "if",
        "impl",
        "in",
        "let",
        "loop",
        "match",
        "mod",
        "move",
        "mut",
        "pub",
        "ref",
        "return",
        "static",
        "self",
        "Self",
        "struct",
        "super",
        "true",
        "trait",
        "type",
        "unsafe",
        "use",
        "while",
        "continue",
        "box",
        "const",
        "where",
        "virtual",
        "proc",
        "alignof",
        "become",
        "offsetof",
        "priv",
        "pure",
        "sizeof",
        "typeof",
        "unsized",
        "yield",
        "do",
        "abstract",
        "final",
        "override",
        "macro",
    ];
    RUST_KEYWORDS.contains(&ident)
}

// reimplementation based on https://github.com/stepancheg/rust-protobuf/blob/v3.7.2/protobuf-codegen/src/gen/scope.rs#L26
// it only implements the `find_message` method with not extra dependencies
pub struct RootScope<'a> {
    file_descriptors: &'a [FileDescriptorProto],
    files_to_generate: &'a [String],
}

// re-implementation of https://github.com/stepancheg/rust-protobuf/blob/v3.7.2/protobuf-codegen/src/gen/scope.rs#L340
// also based on https://github.com/stepancheg/rust-protobuf/blob/v3.7.2/protobuf-codegen/src/gen/scope.rs#L156
pub struct ScopedMessage<'a> {
    pub fd: &'a FileDescriptorProto,
    pub path: Vec<&'a DescriptorProto>,
    pub msg: &'a DescriptorProto,
}

#[derive(Debug, Eq, PartialEq)]
pub enum RustType {
    Generated { module: String, name: String },
    ProtobufRuntime { path: String },
}

impl fmt::Display for RustType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Generated { module, name } => write!(f, "super::{module}::{name}"),
            Self::ProtobufRuntime { path } => f.write_str(path),
        }
    }
}

impl ScopedMessage<'_> {
    pub fn prefix(&self) -> String {
        let mut prefix = String::new();
        for m in &self.path {
            prefix.push_str(m.name());
            prefix.push('.');
        }
        prefix
    }

    // rust type name prefix for this scope
    pub fn rust_prefix(&self) -> String {
        self.prefix().replace(".", "_")
    }

    // rust type name of this descriptor
    pub fn rust_name(&self) -> String {
        let mut r = self.rust_prefix();
        // Only escape if prefix is not empty
        if r.is_empty() && is_rust_keyword(self.msg.name()) {
            r.push_str("message_");
        }
        r.push_str(self.msg.name());
        r
    }

    fn protobuf_runtime_path(&self) -> String {
        let mut path = String::new();
        for message in &self.path {
            path.push_str(&to_snake_case(message.name()));
            path.push_str("::");
        }
        path.push_str(self.msg.name());
        path
    }
}

impl<'a> RootScope<'a> {
    pub fn new(
        file_descriptors: &'a [FileDescriptorProto],
        files_to_generate: &'a [String],
    ) -> Self {
        Self {
            file_descriptors,
            files_to_generate,
        }
    }

    pub fn rust_type(&'a self, fqn: impl AsRef<str>) -> RustType {
        let message = self.find_message(fqn);
        let file_name = message.fd.name();
        let module = super::proto_path_to_rust_mod(file_name);
        let name = message.rust_name();

        if !self.files_to_generate.iter().any(|file| file == file_name) {
            if file_name == DESCRIPTOR_PROTO_FILE {
                return RustType::ProtobufRuntime {
                    path: format!(
                        "::protobuf::descriptor::{}",
                        message.protobuf_runtime_path()
                    ),
                };
            }

            if WELL_KNOWN_TYPE_PROTO_FILES.contains(&file_name) {
                return RustType::ProtobufRuntime {
                    path: format!(
                        "::protobuf::well_known_types::{module}::{}",
                        message.protobuf_runtime_path()
                    ),
                };
            }
        }

        RustType::Generated { module, name }
    }

    pub fn find_message(&'a self, fqn: impl AsRef<str>) -> ScopedMessage<'a> {
        let Some(fqn1) = fqn.as_ref().strip_prefix(".") else {
            panic!("name must start with dot: {}", fqn.as_ref())
        };
        for fd in self.file_descriptors {
            let mut fqn2 = match fqn1.strip_prefix(fd.package()) {
                Some(rest) if fd.package().is_empty() => rest,
                Some(rest) if rest.starts_with(".") => &rest[1..],
                _ => continue,
            };

            assert!(!fqn2.starts_with("."));

            let mut pending = Some(fd.message_type.as_slice());
            let mut path = vec![];
            while let Some(msgs) = pending.take() {
                for msg in msgs {
                    fqn2 = match fqn2.strip_prefix(msg.name()) {
                        Some("") => return ScopedMessage { msg, path, fd },
                        Some(rest) if rest.starts_with(".") => &rest[1..],
                        _ => continue,
                    };
                    path.push(msg);
                    pending = Some(&msg.nested_type);
                    break;
                }
            }
        }
        panic!("message not found by name: {}", fqn.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_descriptor(name: &str, package: &str, message: &str) -> FileDescriptorProto {
        let mut descriptor = FileDescriptorProto::new();
        descriptor.set_name(name.to_owned());
        descriptor.set_package(package.to_owned());

        let mut message_descriptor = DescriptorProto::new();
        message_descriptor.set_name(message.to_owned());
        descriptor.message_type.push(message_descriptor);
        descriptor
    }

    #[test]
    fn well_known_dependencies_use_protobuf_runtime() {
        let cases = [
            ("any.proto", "Any", "any"),
            ("api.proto", "Api", "api"),
            ("duration.proto", "Duration", "duration"),
            ("empty.proto", "Empty", "empty"),
            ("field_mask.proto", "FieldMask", "field_mask"),
            ("source_context.proto", "SourceContext", "source_context"),
            ("struct.proto", "Struct", "struct_"),
            ("timestamp.proto", "Timestamp", "timestamp"),
            ("type.proto", "Type", "type_"),
            ("wrappers.proto", "StringValue", "wrappers"),
        ];
        let files_to_generate = ["service.proto".to_owned()];

        for (proto, message, module) in cases {
            let descriptors = [file_descriptor(
                &format!("google/protobuf/{proto}"),
                "google.protobuf",
                message,
            )];
            let scope = RootScope::new(&descriptors, &files_to_generate);

            assert_eq!(
                format!("::protobuf::well_known_types::{module}::{message}"),
                scope
                    .rust_type(format!(".google.protobuf.{message}"))
                    .to_string()
            );
        }
    }

    #[test]
    fn explicitly_generated_well_known_type_uses_local_module() {
        let descriptors = [file_descriptor(
            "google/protobuf/timestamp.proto",
            "google.protobuf",
            "Timestamp",
        )];
        let files_to_generate = ["google/protobuf/timestamp.proto".to_owned()];
        let scope = RootScope::new(&descriptors, &files_to_generate);

        assert_eq!(
            "super::timestamp::Timestamp",
            scope.rust_type(".google.protobuf.Timestamp").to_string()
        );
    }

    #[test]
    fn descriptor_dependency_uses_protobuf_runtime() {
        let descriptors = [file_descriptor(
            DESCRIPTOR_PROTO_FILE,
            "google.protobuf",
            "FileDescriptorProto",
        )];
        let files_to_generate = ["service.proto".to_owned()];
        let scope = RootScope::new(&descriptors, &files_to_generate);

        assert_eq!(
            "::protobuf::descriptor::FileDescriptorProto",
            scope
                .rust_type(".google.protobuf.FileDescriptorProto")
                .to_string()
        );
    }

    #[test]
    fn nested_descriptor_dependency_uses_protobuf_runtime() {
        let mut descriptor =
            file_descriptor(DESCRIPTOR_PROTO_FILE, "google.protobuf", "DescriptorProto");
        let mut nested = DescriptorProto::new();
        nested.set_name("ExtensionRange".to_owned());
        descriptor.message_type[0].nested_type.push(nested);
        let descriptors = [descriptor];
        let files_to_generate = ["service.proto".to_owned()];
        let scope = RootScope::new(&descriptors, &files_to_generate);

        assert_eq!(
            "::protobuf::descriptor::descriptor_proto::ExtensionRange",
            scope
                .rust_type(".google.protobuf.DescriptorProto.ExtensionRange")
                .to_string()
        );
    }

    #[test]
    fn non_well_known_google_type_uses_local_module() {
        let descriptors = [file_descriptor(
            "google/protobuf/custom.proto",
            "google.protobuf",
            "Custom",
        )];
        let files_to_generate = ["service.proto".to_owned()];
        let scope = RootScope::new(&descriptors, &files_to_generate);

        assert_eq!(
            "super::custom::Custom",
            scope.rust_type(".google.protobuf.Custom").to_string()
        );
    }
}
